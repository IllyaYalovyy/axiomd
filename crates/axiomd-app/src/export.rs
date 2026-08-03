//! Getting the document off the screen: onto paper, into a PDF, into a page anybody
//! can open.
//!
//! # One machine for paper and for PDF
//!
//! A PDF is a print job whose printer is a file. Both go through [`operate`] below,
//! over the very webview the reader is looking at, so the two cannot drift: whatever
//! the print stylesheet does to a printed page it has already done to the exported
//! one. Nothing here converts anything — WebKit paginates the document it is already
//! showing, and no subprocess is started, ever (`design_decisions.md`).
//!
//! # Nothing waits for the main loop, and the main loop waits for nothing
//!
//! Pagination happens in the web process and answers through WebKit's own signals;
//! composing a standalone page — parsing, rendering, reading every picture off the
//! disk — happens on a worker. Both report back on the main loop when they are done,
//! and the window says so beside the document (invariant 4).
//!
//! The one thing that does hold the loop is the reader's print dialog, which is
//! WebKit's own nested loop and the only entry it offers. It is a dialog the reader
//! just asked for, which is the only kind axiomd has (`ux_decisions.md`), and it is
//! opened from an idle callback rather than from inside the action that asked for it
//! — so the action returns, and everything else in the window keeps running while the
//! dialog is up.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use axiomd_render::Picture;

/// The printer that is not a printer. GTK's file backend registers it under this
/// name, and pointing print settings at it with an output URI is what "print to a
/// file" is — the same path a reader who picks it in the print dialog takes.
const TO_A_FILE: &str = "Print to File";

/// The document as a delivery needs it: the page the reader is looking at, and the
/// buffer that page was made from (invariant 11 — never the file on disk).
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

/// Prints the document, through the reader's own print dialog.
///
/// Returns at once: the dialog opens on the next turn of the main loop and `then` is
/// called when the reader has answered it and the job — if they asked for one — has
/// been sent.
pub(crate) fn print(document: &Document, parent: &gtk::Window, then: impl Fn(Outcome) + 'static) {
    let operation = webkit6::PrintOperation::new(&document.view);
    let parent = parent.clone();
    let then = Rc::new(then);
    let answering = then.clone();
    let dialog = operation.clone();
    glib::idle_add_local_once(move || {
        // Blocks in WebKit's own nested loop until the reader answers. Everything
        // else in the application keeps running — that nested loop is the same one
        // the window's timers, its watch on the file and the test channel live on.
        match dialog.run_dialog(Some(&parent)) {
            webkit6::PrintOperationResponse::Print => operate(&dialog, answering),
            _ => answering(Outcome::Cancelled),
        }
    });
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

    let operation = webkit6::PrintOperation::new(&document.view);
    let settings = gtk::PrintSettings::new();
    settings.set_printer(TO_A_FILE);
    settings.set(
        gtk::PRINT_SETTINGS_OUTPUT_URI.as_str(),
        Some(&gio::File::for_path(file).uri()),
    );
    operation.set_print_settings(&settings);
    operate(&operation, Rc::new(then));
}

/// Whether the reader named a file they mean to open in a browser.
fn is_a_page(file: &Path) -> bool {
    file.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("html") || extension.eq_ignore_ascii_case("htm")
        })
}

/// Runs one print job and reports how it ended, exactly once.
///
/// The operation is held here for as long as the job lasts and let go of afterwards,
/// on a later turn of the loop: WebKit is still finishing the emission that said the
/// job was over, and dropping the object underneath it would be the last thing this
/// process did.
fn operate(operation: &webkit6::PrintOperation, then: Rc<impl Fn(Outcome) + 'static>) {
    let held: Rc<RefCell<Option<webkit6::PrintOperation>>> =
        Rc::new(RefCell::new(Some(operation.clone())));
    let complained = Rc::new(Cell::new(false));

    operation.connect_failed({
        let then = then.clone();
        let complained = complained.clone();
        move |_, error| {
            complained.set(true);
            then(Outcome::Failed(error.to_string()));
        }
    });
    operation.connect_finished(move |operation| {
        let destination = operation
            .print_settings()
            .and_then(|settings| settings.get(gtk::PRINT_SETTINGS_OUTPUT_URI.as_str()))
            .map(|uri| gio::File::for_uri(&uri))
            .and_then(|file| file.path());
        if !complained.get() {
            then(Outcome::Done(destination));
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
            Err(_) => then(Outcome::Failed(
                "the export stopped unexpectedly".to_owned(),
            )),
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
