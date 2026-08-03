//! One document, one window.
//!
//! The window owns everything that belongs to its document and nothing that belongs
//! to another: its own view, its own renderer, its own watch on the file, and its own
//! place on the `axiomd://` scheme. Closing it drops all of them, so nothing survives
//! a closed window — no shared state, no reachable page, no watch that can wake it,
//! no worker result with anywhere to go.
//!
//! # Following the file
//!
//! While a window holds a document it watches the file behind it. A save reaches the
//! reader within the debounce and lands in the page they are already looking at,
//! keeping their place (UT-004): the window re-reads and re-renders, and
//! [`DocumentView`] patches the result in rather than navigating.
//!
//! Editors save in ways that are not a write: most write a new file and rename it
//! over the old one, which deletes the file the window opened. The window follows the
//! path rather than the file, so the replacement is just the next version — and the
//! document's identity on disk is retaken with every render, because a window that
//! remembered the identity of a replaced file would no longer recognise its own
//! document.
//!
//! # What the user sees while something is wrong
//!
//! Never a dialog (`ux_decisions.md`). A file that cannot be opened at all is a
//! status page inside the window. A file that goes wrong *while it is being read* —
//! deleted, replaced with something unreadable — does not take the document off the
//! screen: the reader keeps the last version they had and is told beside it, in a
//! banner.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;
use gtk::gio;
use gtk::glib;

use crate::document::{FileId, Page, Renderer};
use crate::scheme::{Publication, Scheme};
use crate::view::DocumentView;
use crate::watch::FileWatch;

/// A window showing one document, or none yet.
pub(crate) struct DocumentWindow {
    window: adw::ApplicationWindow,
    title: adw::WindowTitle,
    banner: adw::Banner,
    view: Rc<DocumentView>,
    status: adw::StatusPage,
    pages: gtk::Stack,
    scheme: Rc<Scheme>,
    open: RefCell<Option<OpenDocument>>,
    /// Which document this window is on. A render that finishes after the window has
    /// been given another file belongs to a document nobody is looking at any more,
    /// and is dropped rather than shown.
    epoch: Cell<u64>,
    /// How many pages this window has finished — rendered documents and the status
    /// pages that explain why there is none alike.
    ///
    /// Rendering is asynchronous, so "the window has caught up with what it was
    /// asked to show" is otherwise unobservable, and a test would have to guess with
    /// a sleep.
    renders: Cell<u32>,
}

/// The document a window currently holds.
struct OpenDocument {
    /// The path the window is following. Not the file it was opened on: an editor
    /// that saves by renaming replaces that file, and the reader means the path.
    file: PathBuf,
    /// The identity on disk of whatever the path last resolved to. Windows are
    /// deduplicated on this, and it is retaken with every render because a save can
    /// change it.
    id: Cell<Option<FileId>>,
    /// This document's place on the `axiomd://` origin. Dropping it withdraws the
    /// document, so a closed window leaves nothing a webview could still ask for.
    publication: Publication,
    renderer: Renderer,
    /// Never read — held so that the file is watched exactly as long as the window
    /// shows it.
    #[expect(
        dead_code,
        reason = "ownership, not data: it ties the watch to the window"
    )]
    watch: FileWatch,
}

/// Names for the two things a window can be showing.
const DOCUMENT_PAGE: &str = "document";
const STATUS_PAGE: &str = "status";

impl DocumentWindow {
    /// Builds an empty window, ready for a document.
    pub(crate) fn new(
        app: &adw::Application,
        context: &webkit6::WebContext,
        scheme: &Rc<Scheme>,
    ) -> Rc<Self> {
        let view = DocumentView::new(context);
        let status = adw::StatusPage::builder()
            .icon_name("text-x-generic-symbolic")
            .title("No document open")
            .description("Open a Markdown file to start reading.")
            .child(&open_button())
            .build();

        let pages = gtk::Stack::new();
        pages.add_named(&status, Some(STATUS_PAGE));
        pages.add_named(view.widget(), Some(DOCUMENT_PAGE));
        pages.set_visible_child_name(STATUS_PAGE);

        let title = adw::WindowTitle::new("axiomd", "");
        let header = adw::HeaderBar::builder().title_widget(&title).build();
        header.pack_start(&open_button());
        header.pack_end(&primary_menu_button());

        // Beneath the header and above the document: what the app has to say about a
        // document the reader is still reading is said next to it, never over it.
        let banner = adw::Banner::builder().revealed(false).build();

        let layout = adw::ToolbarView::builder().content(&pages).build();
        layout.add_top_bar(&header);
        layout.add_top_bar(&banner);

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("axiomd")
            .default_width(900)
            .default_height(700)
            .content(&layout)
            .build();

        Rc::new(Self {
            window,
            title,
            banner,
            view,
            status,
            pages,
            scheme: scheme.clone(),
            open: RefCell::new(None),
            epoch: Cell::new(0),
            renders: Cell::new(0),
        })
    }

    pub(crate) fn window(&self) -> &adw::ApplicationWindow {
        &self.window
    }

    pub(crate) fn webview(&self) -> &webkit6::WebView {
        self.view.widget()
    }

    /// How many loads this window's view has committed since it was built.
    pub(crate) fn navigations(&self) -> u32 {
        self.view.navigations()
    }

    /// What the window is saying beside the document, or an empty string when it has
    /// nothing to say.
    pub(crate) fn banner(&self) -> String {
        if self.banner.is_revealed() {
            self.banner.title().to_string()
        } else {
            String::new()
        }
    }

    /// How many pages this window has finished showing since it was built.
    pub(crate) fn renders(&self) -> u32 {
        self.renders.get()
    }

    /// Which of the two things a window can show it is showing: the document, or the
    /// status page that says why there is none.
    pub(crate) fn showing(&self) -> &'static str {
        match self.pages.visible_child_name().as_deref() {
            Some(DOCUMENT_PAGE) => DOCUMENT_PAGE,
            _ => STATUS_PAGE,
        }
    }

    pub(crate) fn present(&self) {
        self.window.present();
    }

    /// Which file this window holds, if any. Windows are deduplicated on this.
    pub(crate) fn file_id(&self) -> Option<FileId> {
        self.open.borrow().as_ref().and_then(|open| open.id.get())
    }

    /// Shows `file` in this window, replacing whatever it held.
    ///
    /// Returns immediately: the document is read and rendered on a worker and
    /// appears when it is ready. A file that cannot be shown becomes a status page
    /// inside the window, never a dialog. From here on the window follows the file:
    /// a save elsewhere reaches the reader within the debounce, in the page they are
    /// already looking at.
    pub(crate) fn show(self: &Rc<Self>, file: &Path) {
        let file = file.to_path_buf();
        let Some(id) = FileId::of(&file) else {
            self.show_unavailable(
                &format!("Could not open {}", file_name(&file)),
                "There is no such file.",
            );
            self.retitle(&file);
            return;
        };

        let epoch = self.epoch.get() + 1;
        self.epoch.set(epoch);
        self.banner.set_revealed(false);

        let open = OpenDocument {
            id: Cell::new(Some(id)),
            publication: self.scheme.publish(&file),
            renderer: Renderer::new({
                let window = Rc::downgrade(self);
                move |page| {
                    if let Some(window) = window.upgrade() {
                        window.present_page(page, epoch);
                    }
                }
            }),
            watch: FileWatch::new(&file, {
                let window = Rc::downgrade(self);
                move || {
                    if let Some(window) = window.upgrade() {
                        window.reread();
                    }
                }
            }),
            file: file.clone(),
        };

        // Whatever this window held is let go of before the new document starts, so
        // its page, its watch and its place on the scheme are already gone.
        let replaced = self.open.replace(Some(open));
        drop(replaced);

        self.retitle(&file);
        self.reread();
    }

    /// Reads the open document again — because the file behind it changed, and the
    /// reader is to see it without asking for it.
    fn reread(&self) {
        if let Some(open) = self.open.borrow().as_ref() {
            open.renderer.render(open.file.clone());
        }
    }

    /// Puts a finished page in front of the reader.
    ///
    /// `epoch` is the document the page was rendered for: a render that outlived the
    /// document that asked for it is dropped rather than shown, so a window given a
    /// second file can never flash the first one back.
    fn present_page(&self, page: Page, epoch: u64) {
        if epoch != self.epoch.get() {
            return;
        }
        match page {
            Page::Rendered(document) => {
                if let Some(open) = self.open.borrow().as_ref() {
                    self.view.show(&open.publication, &document);
                    // A save may have been a replacement, which gives the path a new
                    // identity on disk. The window follows it, or it stops
                    // recognising its own document and opens a second window on it.
                    open.id.set(FileId::of(&open.file));
                }
                self.banner.set_revealed(false);
                self.pages.set_visible_child_name(DOCUMENT_PAGE);
            }
            Page::Unavailable { title, detail } => {
                if self.showing() == DOCUMENT_PAGE {
                    // The reader is reading, and it is the file underneath them that
                    // went wrong. They keep the version they have and hear about it
                    // beside the document; the view is never taken away from them,
                    // and never interrupted by a dialog (`ux_decisions.md`).
                    self.banner
                        .set_title(&format!("{title} — showing the last version read"));
                    self.banner.set_revealed(true);
                } else {
                    self.status.set_title(&title);
                    self.status.set_description(Some(&detail));
                    self.pages.set_visible_child_name(STATUS_PAGE);
                }
            }
        }
        self.renders.set(self.renders.get() + 1);
    }

    /// Says, inside the window, why there is nothing to read — never in a dialog.
    pub(crate) fn show_unavailable(&self, title: &str, detail: &str) {
        self.status.set_title(title);
        self.status.set_description(Some(detail));
        self.pages.set_visible_child_name(STATUS_PAGE);
        self.renders.set(self.renders.get() + 1);
    }

    fn retitle(&self, file: &Path) {
        let name = file_name(file);
        self.window.set_title(Some(&name));
        self.title.set_title(&name);
        self.title.set_subtitle(&folder_of(file));
    }
}

fn file_name(file: &Path) -> String {
    file.file_name()
        .unwrap_or(file.as_os_str())
        .to_string_lossy()
        .into_owned()
}

/// The document's folder as the user thinks of it, with their home shortened.
fn folder_of(file: &Path) -> String {
    let folder = file.parent().unwrap_or(Path::new("")).display().to_string();
    match glib::home_dir().to_str() {
        Some(home) if folder == home => "~".to_owned(),
        Some(home) => match folder.strip_prefix(&format!("{home}/")) {
            Some(rest) => format!("~/{rest}"),
            None => folder,
        },
        None => folder,
    }
}

fn open_button() -> gtk::Button {
    gtk::Button::builder()
        .icon_name("document-open-symbolic")
        .tooltip_text("Open a document")
        .action_name("app.open")
        .build()
}

fn primary_menu_button() -> gtk::MenuButton {
    let documents = gio::Menu::new();
    documents.append(Some("_New Window"), Some("app.new"));
    documents.append(Some("_Open…"), Some("app.open"));

    let application = gio::Menu::new();
    application.append(Some("_Close Window"), Some("app.close-window"));
    application.append(Some("_Quit"), Some("app.quit"));

    let menu = gio::Menu::new();
    menu.append_section(None, &documents);
    menu.append_section(None, &application);

    gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .tooltip_text("Main menu")
        .menu_model(&menu)
        .build()
}
