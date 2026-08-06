//! Getting the document off the screen: onto paper, into a PDF, into a page anybody
//! can open.
//!
//! # One machine for paper and for PDF, and one job at the end of it
//!
//! Every delivery but a standalone page takes exactly one road: WebKit paginates the
//! webview the reader is looking at into a PDF, [`crate::numbering`] writes the page
//! numbers into that PDF, and then it is either the file the reader asked for or the
//! file that is sent to their printer. What axiomd prints *is* the PDF it exports, so
//! the two cannot drift, and page numbers reach paper and file alike from one place.
//! Nothing here converts anything and no subprocess is started, ever
//! (`design_decisions.md`).
//!
//! One road also means one job. `webkit_print_operation_run_dialog` — what this used
//! to ask with — starts a job itself the moment the reader confirms, so a caller that
//! then does anything with the operation has printed twice; issue #43's first defect
//! was every page coming out of the printer twice. Probed on WebKitGTK 2.52.5: with
//! the second `print()` deleted the operation still emitted a whole `failed`/`finished`
//! cycle from inside `run_dialog`. There is no way to ask that dialog a question
//! without it printing the answer, so the reader is asked with [`gtk::PrintDialog`]
//! instead, which only asks — and the single job is the one [`Printing`] sends.
//!
//! # Nothing waits for the main loop, and the main loop waits for nothing
//!
//! Pagination happens in the web process and answers through WebKit's own signals;
//! numbering the pages and composing a standalone page — parsing, rendering, reading
//! every picture off the disk — happen on a worker. All of them report back on the
//! main loop when they are done, and the window says so beside the document
//! (invariant 4). The print dialog is asynchronous too, so unlike WebKit's it holds
//! nothing at all: it is a dialog the reader just asked for, which is the only kind
//! axiomd has (`ux_decisions.md`).

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use axiomd_i18n::gettext;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use axiomd_render::Picture;

/// The printer that is not a printer. GTK's file backend registers it under this
/// name, and pointing print settings at it with an output URI is what "print to a
/// file" is — the same path a reader who picks it in the print dialog takes.
const TO_A_FILE: &str = "Print to File";

/// The margins every printed page keeps, in millimetres: down the sheet, and across
/// it.
///
/// Here rather than in the print stylesheet because this engine draws no `@page`
/// margin (measured for #19, and the second photograph in #43 is what it looks like on
/// paper). The page setup a print job carries is margin machinery the engine cannot
/// ignore: `gtk_page_setup_get_page_width` is what WebKit lays a page out inside.
const MARGIN: (f64, f64) = (18.0, 16.0);

/// The document as a delivery needs it: the page the reader is looking at, and the
/// buffer that page was made from (invariant 11 — never the file on disk).
#[derive(Clone)]
pub(crate) struct Document {
    /// What a print job paginates: the rendered page, exactly as it is on screen.
    pub(crate) view: webkit6::WebView,
    /// What a standalone page is rendered from.
    pub(crate) source: String,
    /// What the reader calls this document.
    pub(crate) name: String,
    /// The document's folder — the whole of what its pictures may come from, and
    /// `None` for a document that has never been saved and so has none.
    pub(crate) root: Option<PathBuf>,
    /// The optional capabilities the reader is reading under. What is exported is what
    /// the preview shows, so an exported file has the plugins the window had.
    pub(crate) plugins: axiomd_render::Plugins,
    /// And the engine it was read with, for the same reason: a document exported from
    /// a window whose engine the reader changed is the document they were looking at.
    pub(crate) engine: axiomd_engine::EngineId,
}

/// How a delivery ended, in the words the window has to say about it.
pub(crate) enum Outcome {
    /// It is done — printed, or written to this file.
    Done(Option<PathBuf>),
    /// The reader closed the dialog. Nothing happened, and nothing is said.
    Cancelled,
    /// It did not work, and this is what the reader is told.
    Failed(String),
}

/// One window's way of getting a document onto paper, and what the reader last told
/// it.
///
/// One per window and shared with none (invariant 7). It holds the print dialog, which
/// is where GTK keeps the printer and the paper the reader chose: the second print in
/// a window opens where the first one left off rather than asking everything again.
pub(crate) struct Printing {
    asking: gtk::PrintDialog,
    /// How many jobs this window has sent to a printer. One per confirmed dialog, and
    /// the whole reason [`send`] is the only place a job goes out: a second one is a
    /// second copy of the document coming out of the printer, which is what issue #43
    /// was reported with photographs of.
    jobs: Rc<Cell<u32>>,
}

impl Printing {
    /// A window's printing, asking nothing yet.
    pub(crate) fn new() -> Printing {
        Printing {
            asking: gtk::PrintDialog::new(),
            jobs: Rc::new(Cell::new(0)),
        }
    }

    /// How many print jobs this window has sent — how many copies of the document have
    /// left it for a printer.
    ///
    /// What the reader counts in the output tray, and what the test channel asks for
    /// (`control.rs`): a headless run's only printer writes a file, and a file written
    /// twice looks exactly like a file written once.
    pub(crate) fn jobs(&self) -> u32 {
        self.jobs.get()
    }

    /// Prints the document, through the reader's own print dialog.
    ///
    /// Returns at once: the dialog opens on the next turn of the main loop and `then`
    /// is called when the reader has answered it and the one job — if they asked for
    /// one — has been sent.
    pub(crate) fn print(
        &self,
        document: &Document,
        parent: &gtk::Window,
        then: impl Fn(Outcome) + 'static,
    ) {
        let (asking, jobs) = (self.asking.clone(), self.jobs.clone());
        let (over, document) = (parent.clone(), document.clone());
        let then = Rc::new(then);
        self.asking
            .setup(Some(parent), gio::Cancellable::NONE, move |answer| {
                match answer {
                    Ok(setup) => {
                        // What the reader chose is what this window offers next time.
                        asking.set_print_settings(&setup.print_settings());
                        asking.set_page_setup(&setup.page_setup());
                        let printer = Destination::Printer {
                            asking,
                            setup,
                            parent: over,
                            jobs,
                        };
                        deliver(&document, printer, then);
                    }
                    // A reader who changed their mind is told nothing at all; anything
                    // else went wrong and is said beside the document.
                    Err(trouble) if trouble.matches(gtk::DialogError::Dismissed) => {
                        then(Outcome::Cancelled)
                    }
                    Err(trouble) => then(Outcome::Failed(trouble.to_string())),
                }
            });
    }

    /// Which printer this window's print dialog opens on, as choosing it in that
    /// dialog's list does.
    ///
    /// The test channel's way in (`control.rs`). A headless compositor has no pointer
    /// and GTK's printer list lives inside the dialog rather than in this window, so
    /// the choice arrives here instead — the same seam the scroll wheel and the pinch
    /// arrive at in `zoom.rs`, and everything past it is the application's own doing.
    pub(crate) fn choose(&self, printer: &str) {
        let settings = self.asking.print_settings().unwrap_or_default();
        settings.set_printer(printer);
        self.asking.set_print_settings(&settings);
    }
}

/// Writes the document to `file`, in the format the reader named it with: a
/// standalone page for `.html`, and a PDF for anything else.
///
/// Returns at once; `then` is called when the file is there, or when it could not be.
pub(crate) fn write(document: &Document, file: &Path, then: impl Fn(Outcome) + 'static) {
    if is_a_page(file) {
        write_a_page(document, file, then);
        return;
    }
    deliver(
        document,
        Destination::File(file.to_path_buf()),
        Rc::new(then),
    );
}

/// Where a paginated document ends up.
enum Destination {
    /// The file the reader named in the export chooser.
    File(PathBuf),
    /// The printer they chose in the print dialog, and the dialog that asked — which
    /// is also what sends the job.
    Printer {
        asking: gtk::PrintDialog,
        setup: gtk::PrintSetup,
        parent: gtk::Window,
        jobs: Rc<Cell<u32>>,
    },
}

impl Destination {
    /// The sheet the document is laid out on: the paper the reader chose, with
    /// axiomd's margins on it.
    ///
    /// Never less than the printer can actually reach — a margin narrower than the
    /// hardware's own would be a promise the paper cannot keep — and never the bare
    /// hardware margin either, which is what printing with no page setup at all gave
    /// and what the photographs in #43 are of.
    fn sheet(&self) -> gtk::PageSetup {
        let sheet = match self {
            Destination::File(_) => gtk::PageSetup::new(),
            Destination::Printer { setup, .. } => setup.page_setup().copy(),
        };
        let (down, across) = MARGIN;
        let millimetres = gtk::Unit::Mm;
        sheet.set_top_margin(sheet.top_margin(millimetres).max(down), millimetres);
        sheet.set_bottom_margin(sheet.bottom_margin(millimetres).max(down), millimetres);
        sheet.set_left_margin(sheet.left_margin(millimetres).max(across), millimetres);
        sheet.set_right_margin(sheet.right_margin(millimetres).max(across), millimetres);
        sheet
    }

    /// The file the paginated document is written into: the reader's own file for an
    /// export, and a temporary one for a job on its way to a printer.
    ///
    /// Made by GLib rather than named by this code, so nothing else on the machine can
    /// have arranged to be there first.
    fn paginate_into(&self) -> Result<PathBuf, String> {
        match self {
            Destination::File(file) => Ok(file.clone()),
            Destination::Printer { .. } => gio::File::new_tmp(Some("axiomd-print-XXXXXX.pdf"))
                .map_err(|trouble| trouble.to_string())?
                .0
                .path()
                .ok_or_else(|| gettext("there is nowhere to put the pages")),
        }
    }
}

/// Paginates the document, numbers its pages, and delivers it — once.
///
/// The single place a delivery is started, and the single place a job is sent.
fn deliver(document: &Document, destination: Destination, then: Rc<impl Fn(Outcome) + 'static>) {
    let paginated = match destination.paginate_into() {
        Ok(file) => file,
        Err(trouble) => return then(Outcome::Failed(trouble)),
    };

    let settings = gtk::PrintSettings::new();
    settings.set_printer(TO_A_FILE);
    settings.set(
        gtk::PRINT_SETTINGS_OUTPUT_URI.as_str(),
        Some(&gio::File::for_path(&paginated).uri()),
    );

    let operation = webkit6::PrintOperation::new(&document.view);
    operation.set_print_settings(&settings);
    operation.set_page_setup(&destination.sheet());

    let destination = RefCell::new(Some(destination));
    operate(&operation, move |outcome| match outcome {
        Outcome::Done(_) => {
            if let Some(destination) = destination.borrow_mut().take() {
                number_and_send(destination, paginated.clone(), then.clone());
            }
        }
        // Nothing was paginated, so there is nothing to number and nothing to send.
        outcome => then(outcome),
    });
}

/// Writes the page numbers into the paginated document and hands it on: to the reader,
/// or to their printer.
fn number_and_send(
    destination: Destination,
    paginated: PathBuf,
    then: Rc<impl Fn(Outcome) + 'static>,
) {
    glib::spawn_future_local(async move {
        // On a worker: a hundred-page document is a hundred pages of PDF to read,
        // stamp and write back, and the window stays usable throughout (invariant 4).
        let numbered = {
            let paginated = paginated.clone();
            gio::spawn_blocking(move || crate::numbering::number_the_pages(&paginated)).await
        };
        match numbered {
            Ok(Ok(())) => send(destination, paginated, then),
            Ok(Err(trouble)) => {
                forget(&destination, &paginated);
                then(Outcome::Failed(
                    gettext("the pages could not be numbered: {reason}")
                        .replace("{reason}", &trouble),
                ));
            }
            Err(_) => {
                forget(&destination, &paginated);
                then(Outcome::Failed(gettext(
                    "numbering the pages stopped unexpectedly",
                )));
            }
        }
    });
}

/// Hands the finished PDF to whoever asked for it.
fn send(destination: Destination, paginated: PathBuf, then: Rc<impl Fn(Outcome) + 'static>) {
    match destination {
        Destination::File(file) => then(Outcome::Done(Some(file))),
        Destination::Printer {
            asking,
            setup,
            parent,
            jobs,
        } => {
            jobs.set(jobs.get() + 1);
            asking.print_file(
                Some(&parent),
                Some(&setup),
                &gio::File::for_path(&paginated),
                gio::Cancellable::NONE,
                move |sent| {
                    let _ = std::fs::remove_file(&paginated);
                    match sent {
                        // The printer's name for a job is the reader's business, not the
                        // window's: what it says is that the document was printed.
                        Ok(()) => then(Outcome::Done(None)),
                        Err(trouble) if trouble.matches(gtk::DialogError::Dismissed) => {
                            then(Outcome::Cancelled)
                        }
                        Err(trouble) => then(Outcome::Failed(trouble.to_string())),
                    }
                },
            )
        }
    }
}

/// Takes back the temporary file a job that never went out was staged in. An export's
/// own file is the reader's and is left where it is, with whatever got as far as being
/// written to it.
fn forget(destination: &Destination, paginated: &Path) {
    if matches!(destination, Destination::Printer { .. }) {
        let _ = std::fs::remove_file(paginated);
    }
}

/// Whether the reader named a file they mean to open in a browser.
fn is_a_page(file: &Path) -> bool {
    file.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("html") || extension.eq_ignore_ascii_case("htm")
        })
}

/// Lays the document out into the pages of a PDF, and reports how it ended, exactly
/// once.
///
/// The operation is held here for as long as it lasts and let go of afterwards, on a
/// later turn of the loop: WebKit is still finishing the emission that said the
/// pagination was over, and dropping the object underneath it would be the last thing
/// this process did.
fn operate(operation: &webkit6::PrintOperation, then: impl Fn(Outcome) + 'static) {
    let held: Rc<RefCell<Option<webkit6::PrintOperation>>> =
        Rc::new(RefCell::new(Some(operation.clone())));
    let complained = Rc::new(Cell::new(false));
    let then = Rc::new(then);

    operation.connect_failed({
        let then = then.clone();
        let complained = complained.clone();
        move |_, error| {
            complained.set(true);
            then(Outcome::Failed(error.to_string()));
        }
    });
    operation.connect_finished(move |_| {
        if !complained.get() {
            then(Outcome::Done(None));
        }
        let held = held.clone();
        glib::idle_add_local_once(move || {
            held.borrow_mut().take();
        });
    });
    operation.print();
}

/// Composes and writes a standalone page on a worker.
///
/// Every picture in the document is read from the disk here, which is exactly why it
/// is not on the main loop: a document with a hundred pictures in it would otherwise
/// stop the window for as long as the disk took.
fn write_a_page(document: &Document, file: &Path, then: impl Fn(Outcome) + 'static) {
    let source = document.source.clone();
    let name = document.name.clone();
    let root = document.root.clone();
    let plugins = document.plugins.clone();
    let engine = document.engine;
    let file = file.to_path_buf();

    glib::spawn_future_local(async move {
        let written = {
            let file = file.clone();
            gio::spawn_blocking(move || {
                let page = crate::document::compose_standalone(
                    &source,
                    &name,
                    engine,
                    &plugins,
                    root.as_deref(),
                    &|reference| picture(root.as_deref(), reference),
                );
                std::fs::write(&file, page).map_err(|trouble| trouble.to_string())
            })
            .await
        };
        match written {
            Ok(Ok(())) => then(Outcome::Done(Some(file))),
            Ok(Err(trouble)) => then(Outcome::Failed(trouble)),
            Err(_) => then(Outcome::Failed(gettext("the export stopped unexpectedly"))),
        }
    });
}

/// One picture the document names, as the bytes an exported page carries.
///
/// The same containment rule the app serves a document's pictures under
/// (`scheme.rs`): a reference that leaves the document's own folder is answered with
/// nothing here too, so exporting a document can read no more of the disk than
/// displaying it can.
fn picture(root: Option<&Path>, reference: &str) -> Option<Picture> {
    let root = root?;
    let reference = reference.split(['?', '#']).next().unwrap_or_default();
    let decoded = glib::Uri::unescape_string(reference, None)?;
    let path = crate::scheme::path_under(root, decoded.as_str())?;
    Some(Picture {
        bytes: std::fs::read(&path).ok()?,
        content_type: crate::scheme::content_type(&path).to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::ScratchDir;

    /// The reader chooses the format by naming the file, in the chooser they are
    /// already in — never in a question afterwards (`ux_decisions.md`).
    #[test]
    fn the_name_the_reader_gives_the_file_is_the_format_they_chose() {
        assert!(is_a_page(Path::new("/home/reader/notes.html")));
        assert!(is_a_page(Path::new("/home/reader/notes.HTM")));
        assert!(!is_a_page(Path::new("/home/reader/notes.pdf")));
        assert!(
            !is_a_page(Path::new("/home/reader/notes")),
            "a file with no extension is the PDF the chooser offered",
        );
    }

    /// An exported page carries the document's own pictures and nothing else: the
    /// folder above it is as unreachable here as it is on screen.
    #[test]
    fn a_picture_travels_only_if_it_is_the_documents_own() {
        let scratch = ScratchDir::new("export-picture");
        scratch.write("inner/notes.md", "# Notes\n");
        scratch.write("inner/images/logo.png", b"\x89PNG\r\n\x1a\n");
        scratch.write("secret.txt", "credentials");
        let root = scratch.path().join("inner");

        assert_eq!(
            picture(Some(&root), "images/logo.png"),
            Some(Picture {
                bytes: b"\x89PNG\r\n\x1a\n".to_vec(),
                content_type: "image/png".to_owned(),
            }),
        );
        assert_eq!(picture(Some(&root), "../secret.txt"), None);
        assert_eq!(picture(Some(&root), "%2e%2e/secret.txt"), None);
        assert_eq!(picture(Some(&root), "images/missing.png"), None);
        assert_eq!(
            picture(None, "images/logo.png"),
            None,
            "a document that has never been saved has no folder to read",
        );
    }

    /// A picture whose name had to be escaped to travel in a document is still that
    /// picture on the disk.
    #[test]
    fn a_picture_with_a_space_in_its_name_is_still_found() {
        let scratch = ScratchDir::new("export-escaped");
        scratch.write("my picture.png", b"\x89PNG");

        assert_eq!(
            picture(Some(scratch.path()), "my%20picture.png"),
            Some(Picture {
                bytes: b"\x89PNG".to_vec(),
                content_type: "image/png".to_owned(),
            }),
        );
    }
}
