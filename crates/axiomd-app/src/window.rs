//! One document, one window, two ways of looking at it.
//!
//! The window owns everything that belongs to its document and nothing that belongs
//! to another: its buffer, its two surfaces, its renderer, its watch on the file, and
//! its own place on the `axiomd://` scheme. Closing it drops all of them, so nothing
//! survives a closed window — no shared state, no reachable page, no watch that can
//! wake it, no worker result with anywhere to go.
//!
//! # The buffer is the document
//!
//! While a window owns a file, [`axiomd_doc::Document`] is the source of truth and the
//! file is only where the text came from and where it goes back to (invariant 11).
//! Rendering reads the buffer, so an edit is on screen before — and whether or not —
//! it is ever saved.
//!
//! # Two modes, and the door left open for more
//!
//! Read and edit ([`Mode`]), on `Ctrl+E` and the header-bar button; no split view in
//! the MVP (`ux_decisions.md`). What keeps that door open is that neither surface
//! assumes it is the only one: both exist for the whole life of the window and both
//! are kept current — the renderer runs while the reader is typing, which is what
//! makes switching back instant and is exactly what a split view would need. The stack
//! below is only today's answer to "which of them is on screen".
//!
//! Switching preserves the reader's place in both directions, through the anchor map
//! and never through proportional scroll (invariant 5): read to edit puts the caret on
//! the source line of the topmost block still on screen, edit to read brings the block
//! the caret is in back to the top. The rule for "the block a line is in" is stated
//! once, in [`place.js`](../src/place.js), and it is the same rule live reload uses.
//!
//! # Typing costs a keystroke
//!
//! A key press reaches the buffer and stops there. The document is marked edited —
//! constant cost — and two timers are restarted: the render debounce and, when the
//! reader has asked for it, the autosave delay. Neither the parse nor the write is
//! ever on the path between the key and the character appearing, so how long this
//! document takes to render has nothing to do with how it feels to type in it.
//!
//! # Following the file
//!
//! While a window holds a document it watches the file behind it, and what a change
//! means is [`axiomd_doc::Document::reconcile`]'s to say. A clean buffer follows the
//! file silently, in the page the reader is already looking at and keeping their place
//! (UT-004). A modified one never loses a word in either direction: the reader is told
//! beside the document and chooses. The window's own saves — including automatic ones
//! — are recognised as its own and reach none of this.
//!
//! # Which engine this window reads with
//!
//! The reader's preference decides, unless this window has been switched to another
//! engine from its main menu (issue #17). The override belongs to the window and to
//! nothing else: it lasts as long as the window does, it follows the reader from one
//! document to the next inside it, another window is unaffected, and nothing about it
//! is written down (invariant 7). A window that has not been switched follows the
//! preference live — change it and every such window re-renders where it stands.
//!
//! Switching costs a render and never a reload: the page is patched, so the reader
//! keeps their place exactly as they do when a plugin is switched (invariants 5 and 9).
//!
//! # Following a link
//!
//! A link to another Markdown file in the reader's own folder is read in this same
//! window, and the window remembers where they have been: back and forward are the
//! header-bar buttons, `Alt+Left` and `Alt+Right`, and the in-memory stack below
//! (UT-007). Everything else a link can be leaves the app entirely — the browser gets
//! an external address, the desktop gets a file axiomd does not render — and only
//! ever because the reader clicked it.
//!
//! # What the user sees while something is wrong
//!
//! Never a dialog on the open or view path (`ux_decisions.md`). A file that cannot be
//! opened at all is a status page inside the window. A file that goes wrong *while it
//! is being read* — deleted, replaced with something unreadable, changed under unsaved
//! work — does not take the document off the screen: the reader keeps the version they
//! had and is told beside it, in an inline notice whose buttons are the whole of the
//! choice. The two dialogs this window does raise are both answers to something the
//! reader just asked for: Save As, and being asked about unsaved work on the way out.

use std::cell::{Cell, OnceCell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;
use axiomd_doc::{Document, External, Trouble};
use axiomd_render::{Rendered, Request};
use gtk::gio;
use gtk::glib;

use crate::document::{FileId, Renderer};
use crate::editor::Editor;
use crate::find::{Find, Searchable};
use crate::links::Follow;
use crate::outline::Outline;
use crate::remote;
use crate::scheme::{Publication, Scheme};
use crate::settings::{Settings, Watch};
use crate::view::DocumentView;
use crate::watch::{FileWatch, QUIET};
use crate::zoom::Zoom;

/// Which of its two surfaces a window is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// The rendered document. Where opening a file lands (`ux_decisions.md`).
    Read,
    /// The source. Where a bare launch and `Ctrl+N` land.
    Edit,
}

impl Mode {
    fn other(self) -> Mode {
        match self {
            Mode::Read => Mode::Edit,
            Mode::Edit => Mode::Read,
        }
    }
}

/// A window showing one document.
pub(crate) struct DocumentWindow {
    window: adw::ApplicationWindow,
    title: adw::WindowTitle,
    notice: Notice,
    view: Rc<DocumentView>,
    editor: Rc<Editor>,
    outline: Rc<Outline>,
    find: Rc<Find>,
    status: adw::StatusPage,
    surfaces: gtk::Stack,
    scheme: Rc<Scheme>,
    settings: Rc<Settings>,
    /// The text this window owns, and everything true about it that is not the text.
    document: RefCell<Document>,
    open: RefCell<Option<OpenDocument>>,
    mode: Cell<Mode>,
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
    /// Which debounced re-render is the current one. Every keystroke supersedes the
    /// one before it, so a burst of typing is one render at the end rather than one
    /// render per key.
    rerender: Cell<u64>,
    /// The same, for the automatic save.
    autosave: Cell<u64>,
    /// Set once the reader has answered the question about unsaved work, so that the
    /// close they asked for goes through the second time rather than asking again.
    leaving: Cell<bool>,
    /// Where the reader has been in this window, and where they are in it.
    history: RefCell<History>,
    /// The remote images this window has asked for and not yet heard back about, so
    /// that pressing "load all" twice is one fetch per image rather than two.
    fetching: RefCell<Vec<String>>,
    /// This window's subscription to the reader changing how they want to read. It
    /// ends with the window, so a closed one is neither restyled nor kept alive by
    /// being restyled (invariant 7).
    layout: OnceCell<Watch>,
    /// The same, for whether the outline sits beside documents.
    sidebar: OnceCell<Watch>,
    /// And for which optional rendering capabilities are switched on. This one costs a
    /// render rather than a restyle — a plugin changes what the document is — and the
    /// page on screen is patched, never reloaded.
    capabilities: OnceCell<Watch>,
    /// How big this window shows its documents. One window's own and nothing that is
    /// written down: it lasts as long as the window and no longer (UT-011).
    zoom: Rc<Zoom>,
    /// The engine this window has been switched to, or `None` while it follows the
    /// reader's preference. One window's own, and nothing that is written down either.
    engine: Cell<Option<axiomd_engine::EngineId>>,
    /// This window's subscription to the reader changing which engine documents are
    /// read with. Only heeded while this window has not been switched itself.
    parser: OnceCell<Watch>,
}

/// Where the reader has been in one window.
///
/// Only documents this window opened in place are here: following a link, and going
/// back and forward over it. It is per window, in memory, and goes away with the
/// window (invariant 7); nothing about it is persisted (issue #6, out of scope).
#[derive(Default)]
struct History {
    entries: Vec<Visit>,
    /// Which entry the window is showing. Meaningless when `entries` is empty.
    at: usize,
}

/// One place the reader has been: a document, and the section of it they arrived at.
#[derive(Clone)]
struct Visit {
    file: PathBuf,
    fragment: String,
}

impl History {
    /// Starts again at `visit` — the window has been given a document rather than
    /// having followed a link to one.
    fn restart(&mut self, visit: Visit) {
        self.entries = vec![visit];
        self.at = 0;
    }

    /// Records a link followed from where the reader is, which is what makes forward
    /// mean something and what discards a branch they have left.
    fn follow(&mut self, visit: Visit) {
        self.entries.truncate(self.at + 1);
        self.entries.push(visit);
        self.at = self.entries.len() - 1;
    }

    /// Moves back or forward, answering with where that is — or `None` when there is
    /// nowhere that way.
    fn step(&mut self, forward: bool) -> Option<Visit> {
        let next = if forward {
            self.at.checked_add(1).filter(|at| *at < self.entries.len())
        } else {
            self.at.checked_sub(1)
        }?;
        self.at = next;
        self.entries.get(next).cloned()
    }

    fn can_step(&self, forward: bool) -> bool {
        if forward {
            self.at + 1 < self.entries.len()
        } else {
            !self.entries.is_empty() && self.at > 0
        }
    }
}

/// Everything a window sets up for the document it currently holds, and lets go of
/// when it is given another one.
struct OpenDocument {
    /// The path the window is following, or `None` for a document that has never been
    /// saved. Not the file it was opened on: an editor that saves by renaming replaces
    /// that file, and the reader means the path.
    file: Option<PathBuf>,
    /// The section of it the reader arrived at, if they followed a link to one.
    fragment: String,
    /// The identity on disk of whatever the path last resolved to. Windows are
    /// deduplicated on this, and it is retaken with every render because a save can
    /// change it.
    id: Cell<Option<FileId>>,
    /// Whether the window has the document's text yet. Until it does, a change to the
    /// file is a first reading rather than a change under one.
    loaded: Cell<bool>,
    /// This document's place on the `axiomd://` origin. Dropping it withdraws the
    /// document, so a closed window leaves nothing a webview could still ask for.
    publication: Publication,
    renderer: Renderer,
    /// Never read — held so that the file is watched exactly as long as the window
    /// shows it. `None` for a document with no file to watch.
    #[expect(
        dead_code,
        reason = "ownership, not data: it ties the watch to the window"
    )]
    watch: Option<FileWatch>,
}

/// Names for the three things a window can be showing.
const STATUS_PAGE: &str = "status";
const DOCUMENT_PAGE: &str = "document";
const EDITOR_PAGE: &str = "editor";

/// The window actions the header bar and the keyboard share. Named twice because a
/// widget addresses an action by its full name and a window registers it by its bare
/// one.
pub(crate) const BACK: &str = "win.back";
pub(crate) const FORWARD: &str = "win.forward";
pub(crate) const MODE: &str = "win.mode";
pub(crate) const OUTLINE: &str = "win.outline";
pub(crate) const SAVE: &str = "win.save";
pub(crate) const SAVE_AS: &str = "win.save-as";
pub(crate) const UNDO: &str = "win.undo";
pub(crate) const REDO: &str = "win.redo";
pub(crate) const PRINT: &str = "win.print";
pub(crate) const EXPORT: &str = "win.export";
/// Which engine this window reads its document with. Stateful and parameterised: the
/// menu shows it as a set of radio items, and the state is the engine in force —
/// whether that is the reader's preference or this window's own choice.
pub(crate) const ENGINE: &str = "win.engine";

/// The name the primary menu's model gives the slot the zoom row is put into.
const ZOOM_SLOT: &str = "zoom";

impl DocumentWindow {
    /// Builds a window holding a new untitled document, ready to be given a file.
    pub(crate) fn new(
        app: &adw::Application,
        context: &webkit6::WebContext,
        scheme: &Rc<Scheme>,
        settings: &Rc<Settings>,
        engine: Option<axiomd_engine::EngineId>,
    ) -> Rc<Self> {
        let view = DocumentView::new(context);
        let editor = Editor::new();
        let status = adw::StatusPage::builder()
            .icon_name("text-x-generic-symbolic")
            .title("No document open")
            .description("Open a Markdown file to start reading.")
            .child(&open_button())
            .build();

        // All three exist for the whole life of the window and all three stay current;
        // the stack only decides which one the reader is looking at (see the module
        // documentation on keeping the split-view door open).
        let surfaces = gtk::Stack::new();
        surfaces.add_named(&status, Some(STATUS_PAGE));
        surfaces.add_named(view.widget(), Some(DOCUMENT_PAGE));
        surfaces.add_named(editor.widget(), Some(EDITOR_PAGE));
        surfaces.set_visible_child_name(STATUS_PAGE);

        // Beneath the header and above the document: what the app has to say about a
        // document the reader is still reading is said next to it, never over it.
        let notice = Notice::new();

        let layout = adw::ToolbarView::new();

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("axiomd")
            .default_width(900)
            .default_height(700)
            .content(&layout)
            .build();

        // Before the header, because the header shows it: this window's zoom owns the
        // three actions the menu row is bound to, and it is the window's own — closing
        // it takes the zoom with it (invariant 7).
        let zoom = Zoom::attach(&window, view.widget());

        let title = adw::WindowTitle::new("axiomd", "");
        let header = adw::HeaderBar::builder().title_widget(&title).build();
        header.pack_start(&outline_button());
        header.pack_start(&step_button("go-previous-symbolic", "Back", BACK));
        header.pack_start(&step_button("go-next-symbolic", "Forward", FORWARD));
        header.pack_start(&open_button());
        header.pack_end(&primary_menu_button(&zoom));
        header.pack_end(&mode_button());

        // The search bar goes directly under the header and above everything the app
        // has to say, because it is the reader's own doing rather than the app's. It
        // holds both surfaces and asks whichever one the reader is looking at, so the
        // window only has to tell it when that changes.
        let find = Find::new(
            &window,
            view.clone() as Rc<dyn Searchable>,
            editor.clone() as Rc<dyn Searchable>,
        );

        layout.add_top_bar(&header);
        layout.add_top_bar(find.widget());
        layout.add_top_bar(notice.widget());

        // The outline goes between the header and the document: it owns the split the
        // two surfaces sit in, the `F9` action, and the breakpoint that gets it out of
        // a narrow window's way.
        let outline = Outline::new(&window, &surfaces, OUTLINE);
        layout.set_content(Some(outline.widget()));

        let document_window = Rc::new(Self {
            window,
            title,
            notice,
            view,
            editor,
            outline,
            find,
            status,
            surfaces,
            scheme: scheme.clone(),
            settings: settings.clone(),
            document: RefCell::new(Document::untitled()),
            open: RefCell::new(None),
            mode: Cell::new(Mode::Read),
            epoch: Cell::new(0),
            renders: Cell::new(0),
            rerender: Cell::new(0),
            autosave: Cell::new(0),
            leaving: Cell::new(false),
            history: RefCell::new(History::default()),
            fetching: RefCell::new(Vec::new()),
            layout: OnceCell::new(),
            sidebar: OnceCell::new(),
            capabilities: OnceCell::new(),
            zoom,
            engine: Cell::new(engine),
            parser: OnceCell::new(),
        });

        // From here on this window lays its documents out the reader's way — the one
        // on screen and every one after it. It starts at once, so a reader whose
        // measure is not the default never sees the default one first.
        let relaying_out = Rc::downgrade(&document_window);
        let _ = document_window
            .layout
            .set(settings.follow_reading_style(move |stylesheet| {
                if let Some(window) = relaying_out.upgrade() {
                    window.view.restyle(stylesheet);
                }
            }));

        // And the same for whether documents are read with their outline beside them.
        // It starts at once, so a reader who has switched the sidebar off never sees
        // it appear and go.
        let revealing = Rc::downgrade(&document_window);
        let _ = document_window
            .sidebar
            .set(settings.follow_outline(move |shown| {
                if let Some(window) = revealing.upgrade() {
                    window.outline.reveal(shown);
                }
            }));

        // And for the optional capabilities documents are rendered with. Switching one
        // renders the document the reader is looking at again, from the buffer they
        // have in front of them — the page is patched where it stands, so the block
        // that changes is the only thing that changes and the reader keeps their place
        // (invariants 5 and 14).
        let recomposing = Rc::downgrade(&document_window);
        let _ = document_window
            .capabilities
            .set(settings.follow_plugins(move || {
                if let Some(window) = recomposing.upgrade() {
                    window.rerender_now();
                }
            }));

        // And for which engine documents are read with. A window the reader has
        // switched keeps its own; every other one follows the preference the moment it
        // changes, re-rendering where it stands rather than reloading (invariant 14).
        let reparsing = Rc::downgrade(&document_window);
        let _ = document_window.parser.set(settings.follow_engine(move || {
            if let Some(window) = reparsing.upgrade()
                && window.engine.get().is_none()
            {
                window.show_engine();
                window.rerender_now();
            }
        }));

        // Picking a section takes the reader to it in whichever surface they are on:
        // the page glides to that block, or the caret lands on that line.
        let navigating = Rc::downgrade(&document_window);
        document_window.outline.connect_chosen(move |line| {
            if let Some(window) = navigating.upgrade() {
                window.go_to_line(line);
            }
        });

        // Which section the page says the reader is in (`track.js`).
        let reading = Rc::downgrade(&document_window);
        document_window.view.connect_sectioned(move |line| {
            if let Some(window) = reading.upgrade()
                && window.mode.get() == Mode::Read
            {
                window.outline.follow(line);
            }
        });

        // And where the caret says they are, for the surface the page is not.
        let editing = Rc::downgrade(&document_window);
        document_window.editor.connect_moved(move |line| {
            if let Some(window) = editing.upgrade()
                && window.mode.get() == Mode::Edit
            {
                window.outline.follow(line);
            }
        });

        // Everything the view refuses to do itself: another document, the browser,
        // the desktop, or the reader asking for a remote image.
        let followed = Rc::downgrade(&document_window);
        document_window.view.connect_follow(move |elsewhere| {
            if let Some(window) = followed.upgrade() {
                window.act_on(elsewhere);
            }
        });

        // The whole of what a keystroke costs.
        let typed = Rc::downgrade(&document_window);
        document_window.editor.connect_changed(move || {
            if let Some(window) = typed.upgrade() {
                window.document.borrow_mut().edited();
                window.retitle();
                window.schedule_render();
                window.schedule_autosave();
            }
        });

        // Closing the search gives the keyboard back to whatever the reader was doing,
        // so that Escape out of the bar in edit mode leaves them able to type.
        let searched = Rc::downgrade(&document_window);
        document_window.find.connect_closed(move || {
            if let Some(window) = searched.upgrade()
                && window.mode.get() == Mode::Edit
            {
                window.editor.take_the_keyboard();
            }
        });

        document_window.install_actions();

        // Autosave on leaving the window as well as on falling quiet: a reader who
        // switches to another application has stopped typing whatever the timer says.
        let unfocused = Rc::downgrade(&document_window);
        document_window
            .window
            .connect_is_active_notify(move |window| {
                if !window.is_active()
                    && let Some(document_window) = unfocused.upgrade()
                {
                    document_window.autosave_now();
                }
            });

        document_window
    }

    fn install_actions(self: &Rc<Self>) {
        for (name, forward) in [(BACK, false), (FORWARD, true)] {
            let action = gio::SimpleAction::new(bare(name), None);
            action.set_enabled(false);
            let stepping = Rc::downgrade(self);
            action.connect_activate(move |_, _| {
                if let Some(window) = stepping.upgrade() {
                    window.step(forward);
                }
            });
            self.window.add_action(&action);
        }

        // Stateful, because the header-bar button is a toggle showing which mode the
        // window is in — and read-to-edit finishes a turn of the loop later, so the
        // button follows the window rather than the other way round.
        let mode = gio::SimpleAction::new_stateful(bare(MODE), None, &false.to_variant());
        let switching = Rc::downgrade(self);
        mode.connect_activate(move |_, _| {
            if let Some(window) = switching.upgrade() {
                window.set_mode(window.mode.get().other());
            }
        });
        self.window.add_action(&mode);

        // Parameterised and stateful, which is what makes the menu a set of radio
        // items: the target is the engine's name and the state is the one in force.
        let engine = gio::SimpleAction::new_stateful(
            bare(ENGINE),
            Some(&String::static_variant_type()),
            &self.engine().as_str().to_variant(),
        );
        let choosing = Rc::downgrade(self);
        engine.connect_activate(move |_, chosen| {
            let Some(window) = choosing.upgrade() else {
                return;
            };
            let Some(chosen) = chosen.and_then(|chosen| chosen.get::<String>()) else {
                return;
            };
            window.read_with(&chosen);
        });
        self.window.add_action(&engine);

        for (name, act) in [
            (SAVE, Deed::Save),
            (SAVE_AS, Deed::SaveAs),
            (UNDO, Deed::Undo),
            (REDO, Deed::Redo),
            (PRINT, Deed::Print),
            (EXPORT, Deed::Export),
        ] {
            let action = gio::SimpleAction::new(bare(name), None);
            let doing = Rc::downgrade(self);
            action.connect_activate(move |_, _| {
                if let Some(window) = doing.upgrade() {
                    match act {
                        Deed::Save => window.save(),
                        Deed::SaveAs => window.save_as(),
                        Deed::Undo => window.editor.undo(),
                        Deed::Redo => window.editor.redo(),
                        Deed::Print => window.print(),
                        Deed::Export => window.export(),
                    }
                }
            });
            self.window.add_action(&action);
        }

        // Unsaved work is the one thing that may stop a close, and only by asking the
        // reader — who asked to close — a question they answer once.
        let closing = Rc::downgrade(self);
        self.window.connect_close_request(move |_| {
            let Some(window) = closing.upgrade() else {
                return glib::Propagation::Proceed;
            };
            window.pull_text();
            if window.leaving.get() || !window.document.borrow().is_modified() {
                return glib::Propagation::Proceed;
            }
            window.ask_about_unsaved_work();
            glib::Propagation::Stop
        });
    }

    pub(crate) fn window(&self) -> &adw::ApplicationWindow {
        &self.window
    }

    /// The engine this window reads its document with: its own choice, or the
    /// reader's preference while it has made none.
    pub(crate) fn engine(&self) -> axiomd_engine::EngineId {
        self.engine.get().unwrap_or_else(|| self.settings.engine())
    }

    /// Reads this window's document with `chosen` from now on — what picking an engine
    /// in the main menu does.
    ///
    /// The document the reader is looking at is rendered again from the buffer they
    /// have in front of them and the page is patched where it stands, so the block that
    /// changes is the only thing that changes and they keep their place (invariant 5).
    /// A name this build does not have is ignored rather than acted on: a menu can only
    /// offer engines that exist, so this is only reachable from a stale action target.
    fn read_with(self: &Rc<Self>, chosen: &str) {
        let Some(engine) = axiomd_engine::engine(chosen) else {
            eprintln!("axiomd: no {chosen} engine in this build");
            return;
        };
        if self.engine() == engine.id() && self.engine.get().is_some() {
            return;
        }
        self.engine.set(Some(engine.id()));
        self.show_engine();
        self.rerender_now();
    }

    /// Keeps the menu showing which engine this window is reading with.
    fn show_engine(&self) {
        if let Some(action) = self.window.lookup_action(bare(ENGINE))
            && let Some(action) = action.downcast_ref::<gio::SimpleAction>()
        {
            action.set_state(&self.engine().as_str().to_variant());
        }
    }

    pub(crate) fn webview(&self) -> &webkit6::WebView {
        self.view.widget()
    }

    /// How many loads this window's view has committed since it was built.
    pub(crate) fn navigations(&self) -> u32 {
        self.view.navigations()
    }

    /// The dialog this window is showing, by its title, or an empty string when it is
    /// showing none.
    ///
    /// Opening and reading a document is never interrupted by a question
    /// (`ux_decisions.md`), so this is empty for the whole of that path; everything
    /// that does put something in it — preferences, unsaved work on the way out, the
    /// print dialog — is an answer to something the reader just asked for.
    ///
    /// Two kinds of dialog, because a window can be interrupted by two kinds: one
    /// libadwaita puts inside the window, and one that is a window of its own put in
    /// front of it. The reader cannot tell them apart and neither does this.
    pub(crate) fn visible_dialog(&self) -> String {
        if let Some(dialog) = self.window.visible_dialog() {
            return dialog.title().to_string();
        }
        self.dialog_window()
            .and_then(|dialog| dialog.title())
            .map(|title| title.to_string())
            .unwrap_or_default()
    }

    /// The dialog window standing in front of this one, if there is one.
    fn dialog_window(&self) -> Option<gtk::Window> {
        let toplevels = gtk::Window::toplevels();
        (0..toplevels.n_items())
            .filter_map(|at| toplevels.item(at))
            .filter_map(|object| object.downcast::<gtk::Window>().ok())
            .find(|top| {
                top.is_visible()
                    && top.transient_for().as_ref() == Some(self.window.upcast_ref::<gtk::Window>())
            })
    }

    /// Everything the reader could press in this window right now: the window itself,
    /// and any dialog standing in front of it.
    pub(crate) fn pressable(&self) -> Vec<gtk::Widget> {
        let mut surfaces = vec![self.window.clone().upcast::<gtk::Widget>()];
        surfaces.extend(self.dialog_window().map(|dialog| dialog.upcast()));
        surfaces
    }

    /// What the window is saying beside the document, or an empty string when it has
    /// nothing to say.
    pub(crate) fn banner(&self) -> String {
        self.notice.message()
    }

    /// The outline sidebar as the reader sees it: whether it is there, what it lists,
    /// and which section is highlighted.
    pub(crate) fn outline(&self, of: &str) -> Option<String> {
        match of {
            "outline" => Some(self.outline.listed().join("\n")),
            "outline-notice" => Some(self.outline.notice()),
            "outline-section" => Some(self.outline.current()),
            "outline-shown" => Some(self.outline.is_revealed().to_string()),
            // How many times the page has said which section the reader is in. The
            // bridge promises at most one of these per frame however far the reader
            // scrolls, and this is where that promise is visible.
            "section-reports" => Some(self.view.section_reports().to_string()),
            _ => None,
        }
    }

    /// The search bar as the reader sees it: whether it is up, what is in it, what the
    /// counter says, whether walking the matches has just wrapped, and whether case is
    /// being matched.
    pub(crate) fn search(&self, of: &str) -> Option<String> {
        if of == "find-highlights" {
            // What the *source* is showing highlighted; the rendered page is read
            // straight out of its own DOM, where every other assertion about a
            // document lives.
            return Some(self.editor.highlighted().join("\n"));
        }
        self.find.showing(of)
    }

    /// Types `text` into the search bar, exactly as pressing the keys does.
    pub(crate) fn search_for(&self, text: &str) {
        self.find.type_query(text);
    }

    /// How big this window is showing its document, in the words the primary menu
    /// shows the reader.
    pub(crate) fn zoom(&self, of: &str) -> Option<String> {
        self.zoom.shown(of)
    }

    /// A turn of the scroll wheel over the document, with `control` saying whether the
    /// reader was holding Ctrl — landing where the view's own scroll controller lands.
    pub(crate) fn scroll_over_document(&self, delta: f64, control: bool) {
        self.zoom.scrolled(delta, control);
    }

    /// A pinch over the document, `scale` being how far it has spread since it began —
    /// landing where the view's own zoom gesture lands.
    pub(crate) fn pinch_over_document(&self, scale: f64) {
        self.zoom.pinched(scale);
    }

    /// How many pages this window has finished showing since it was built.
    pub(crate) fn renders(&self) -> u32 {
        self.renders.get()
    }

    /// Which of the three things a window can show it is showing.
    pub(crate) fn showing(&self) -> &'static str {
        match self.surfaces.visible_child_name().as_deref() {
            Some(DOCUMENT_PAGE) => DOCUMENT_PAGE,
            Some(EDITOR_PAGE) => EDITOR_PAGE,
            _ => STATUS_PAGE,
        }
    }

    /// Whether the buffer holds work that is not on disk.
    pub(crate) fn is_modified(&self) -> bool {
        self.pull_text();
        self.document.borrow().is_modified()
    }

    /// The source line the caret is on.
    pub(crate) fn caret_line(&self) -> u32 {
        self.editor.caret_line()
    }

    /// Picks the section called `section` in the outline, as clicking its row does.
    /// Answers whether the sidebar is listing one.
    pub(crate) fn pick_section(&self, section: &str) -> bool {
        self.outline.pick(section)
    }

    /// What the reader has in front of them in edit mode.
    pub(crate) fn editor_text(&self) -> String {
        self.editor.text()
    }

    /// Types `text` where the caret is, exactly as pressing the keys does.
    pub(crate) fn type_text(&self, text: &str) {
        self.editor.type_text(text);
    }

    /// Puts the caret on `line`, as clicking into that line does.
    pub(crate) fn place_caret(self: &Rc<Self>, line: u32) {
        self.editor.place_caret(line);
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
    /// This is the window being *given* a document — opened from the desktop, chosen
    /// in the file chooser — so the reader's way back is to wherever they came from
    /// outside axiomd, and the window's own history starts here.
    ///
    /// Returns immediately: the document is read and rendered on a worker and appears
    /// when it is ready, in read mode (`ux_decisions.md`). A file that cannot be shown
    /// becomes a status page inside the window, never a dialog.
    pub(crate) fn show(self: &Rc<Self>, file: &Path) {
        self.history.borrow_mut().restart(Visit {
            file: file.to_path_buf(),
            fragment: String::new(),
        });
        self.display(file, "");
    }

    /// Puts a new untitled document in this window, in edit mode — a bare launch, or
    /// `Ctrl+N` (`ux_decisions.md`). The reader's first `Ctrl+S` will ask where it goes.
    pub(crate) fn show_untitled(self: &Rc<Self>) {
        *self.history.borrow_mut() = History::default();
        self.retrace();
        *self.document.borrow_mut() = Document::untitled();
        self.editor.fill("");
        self.take_over(None, "");
        self.enter(Mode::Edit);
        self.retitle();
        self.rerender_now();
    }

    /// Follows a link to another document, in this window, remembering where the
    /// reader was so that back means something.
    fn visit(self: &Rc<Self>, file: &Path, fragment: &str) {
        self.history.borrow_mut().follow(Visit {
            file: file.to_path_buf(),
            fragment: fragment.to_owned(),
        });
        self.display(file, fragment);
    }

    /// Back or forward, as the header-bar buttons and `Alt+Left`/`Alt+Right` do.
    fn step(self: &Rc<Self>, forward: bool) {
        let stepped = self.history.borrow_mut().step(forward);
        if let Some(visit) = stepped {
            self.display(&visit.file, &visit.fragment);
        }
    }

    /// Puts a document on screen, wherever in the window's history it came from.
    fn display(self: &Rc<Self>, file: &Path, fragment: &str) {
        self.retrace();
        let file = file.to_path_buf();
        if FileId::of(&file).is_none() {
            self.show_unavailable(
                &format!("Could not open {}", file_name(&file)),
                "There is no such file.",
            );
            self.retitle_as(&file_name(&file), &folder_of(&file));
            return;
        }

        self.take_over(Some(&file), fragment);
        self.enter(Mode::Read);
        self.retitle_as(&file_name(&file), &folder_of(&file));
        self.read_the_file();
    }

    /// Gives this window a place on the scheme, a renderer and a watch for `file`, and
    /// lets go of whatever it had for the last one.
    fn take_over(self: &Rc<Self>, file: Option<&Path>, fragment: &str) {
        let epoch = self.epoch.get() + 1;
        self.epoch.set(epoch);
        self.notice.hide();
        self.fetching.borrow_mut().clear();
        // The search was of the document being left. A count of a document nobody is
        // looking at any more is worse than no count at all.
        self.find.close();

        let open = OpenDocument {
            id: Cell::new(file.and_then(FileId::of)),
            loaded: Cell::new(file.is_none()),
            publication: self.scheme.publish(file),
            renderer: Renderer::new({
                let window = Rc::downgrade(self);
                move |page| {
                    if let Some(window) = window.upgrade() {
                        window.present_page(page, epoch);
                    }
                }
            }),
            watch: file.map(|file| {
                FileWatch::new(file, {
                    let window = Rc::downgrade(self);
                    move || {
                        if let Some(window) = window.upgrade() {
                            window.read_the_file();
                        }
                    }
                })
            }),
            file: file.map(Path::to_path_buf),
            fragment: fragment.to_owned(),
        };

        // Whatever this window held is let go of before the new document starts, so
        // its page, its watch and its place on the scheme are already gone.
        let replaced = self.open.replace(Some(open));
        drop(replaced);
    }

    /// Reads the file behind this window on a worker, and hands the result to
    /// [`DocumentWindow::arrived`] on the main loop.
    ///
    /// The read is where a huge document costs something, so it never happens on the
    /// main thread — which is also what keeps the file monitor's callback from doing
    /// synchronous I/O (invariant 4).
    fn read_the_file(self: &Rc<Self>) {
        let Some(file) = self
            .open
            .borrow()
            .as_ref()
            .and_then(|open| open.file.clone())
        else {
            return;
        };
        let epoch = self.epoch.get();
        let window = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let read = gio::spawn_blocking(move || Document::read(&file)).await;
            let Some(window) = window.upgrade() else {
                return;
            };
            // The reader has moved on to another document; this one is nobody's.
            if window.epoch.get() != epoch {
                return;
            }
            if let Ok(read) = read {
                window.arrived(read);
            }
        });
    }

    /// What the file turned out to say, and what that means for the reader.
    fn arrived(self: &Rc<Self>, file: Result<Document, Trouble>) {
        let first = !self
            .open
            .borrow()
            .as_ref()
            .is_some_and(|open| open.loaded.get());
        if first {
            match file {
                Ok(document) => {
                    self.editor.fill(document.text());
                    *self.document.borrow_mut() = document;
                    if let Some(open) = self.open.borrow().as_ref() {
                        open.loaded.set(true);
                    }
                    self.notice.hide();
                    self.retitle();
                    self.rerender_now();
                }
                Err(trouble) => self.show_unavailable(trouble.title(), trouble.detail()),
            }
            return;
        }

        // The buffer is the truth, so the model is given what the reader has before it
        // is asked to compare the two.
        self.pull_text();
        let outcome = self.document.borrow_mut().reconcile(file);
        match outcome {
            External::Nothing => {}
            External::Followed => {
                let text = self.document.borrow().text().to_owned();
                self.refill(&text);
                self.notice.hide();
                self.rerender_now();
            }
            External::Gone => {
                let name = self.document.borrow().name();
                self.notice.say(
                    &format!("Could not open {name} — showing the last version read"),
                    Vec::new(),
                );
            }
            External::Conflict => self.offer_the_choice(),
        }
        self.retitle();
    }

    /// The one place in the app where the reader is asked something about a document
    /// they did not ask about — and it is beside the document, with the whole choice
    /// on screen, never a dialog over it (`ux_decisions.md`).
    fn offer_the_choice(self: &Rc<Self>) {
        let name = self.document.borrow().name();
        let keeping = Rc::downgrade(self);
        let reloading = Rc::downgrade(self);
        self.notice.say(
            &format!("{name} changed on disk, and you have unsaved changes"),
            vec![
                (
                    "Keep Mine".to_owned(),
                    Rc::new(move || {
                        if let Some(window) = keeping.upgrade() {
                            window.document.borrow_mut().keep_mine();
                            window.notice.hide();
                            window.retitle();
                        }
                    }) as Rc<dyn Fn()>,
                ),
                (
                    "Reload".to_owned(),
                    Rc::new(move || {
                        if let Some(window) = reloading.upgrade() {
                            window.document.borrow_mut().take_theirs();
                            let text = window.document.borrow().text().to_owned();
                            window.refill(&text);
                            window.notice.hide();
                            window.retitle();
                            window.rerender_now();
                        }
                    }) as Rc<dyn Fn()>,
                ),
            ],
        );
    }

    /// Puts `text` in the editor without disturbing the reader more than the change
    /// itself does: they keep the line they were on.
    fn refill(&self, text: &str) {
        let line = self.editor.caret_line();
        self.editor.fill(text);
        if self.mode.get() == Mode::Edit {
            self.editor.place_caret(line);
        }
    }

    /// Keeps the two history buttons saying what they can do. A button that looks
    /// available and does nothing is worse than one that is plainly not.
    fn retrace(&self) {
        for (name, forward) in [(BACK, false), (FORWARD, true)] {
            if let Some(action) = self.window.lookup_action(bare(name))
                && let Some(action) = action.downcast_ref::<gio::SimpleAction>()
            {
                action.set_enabled(self.history.borrow().can_step(forward));
            }
        }
    }

    /// Switches the window to `wanted`, keeping the reader's place across the switch.
    ///
    /// Both directions map through the anchor map and never through proportional
    /// scroll (invariant 5). Read to edit has to ask the page where the reader is, so
    /// it finishes a turn of the loop later; edit to read already knows, and hands the
    /// line to the view to apply once the page has caught up with the buffer.
    fn set_mode(self: &Rc<Self>, wanted: Mode) {
        if self.mode.get() == wanted {
            return;
        }
        match wanted {
            Mode::Edit => {
                let window = Rc::downgrade(self);
                self.view.topmost_source_line(move |line| {
                    if let Some(window) = window.upgrade() {
                        window.enter(Mode::Edit);
                        window.editor.place_caret(line);
                        window.editor.take_the_keyboard();
                    }
                });
            }
            Mode::Read => {
                self.view.place_after_next_render(self.editor.caret_line());
                self.enter(Mode::Read);
                // Always, even when nothing was typed: the place is applied when the
                // page next arrives, and this is what makes it arrive.
                self.rerender_now();
            }
        }
    }

    /// Puts `mode`'s surface on screen and tells the header-bar button about it.
    fn enter(&self, mode: Mode) {
        self.mode.set(mode);
        // A search the reader has open follows them across: the same bar, the same
        // words, counted over what is now in front of them (issue #8).
        self.find.look_in(mode);
        if self.showing() != STATUS_PAGE || mode == Mode::Edit {
            self.surfaces.set_visible_child_name(match mode {
                Mode::Read => DOCUMENT_PAGE,
                Mode::Edit => EDITOR_PAGE,
            });
        }
        if let Some(action) = self.window.lookup_action(bare(MODE))
            && let Some(action) = action.downcast_ref::<gio::SimpleAction>()
        {
            action.set_state(&(mode == Mode::Edit).to_variant());
        }
    }

    /// Takes the reader to source `line`, in whichever surface they are looking at —
    /// what picking a section in the outline does (UT-005).
    ///
    /// One line of source, two places it can be, and the same anchor map either way
    /// (invariant 3): in read mode the page glides to the block that line belongs to,
    /// in edit mode the caret goes to the line itself.
    fn go_to_line(self: &Rc<Self>, line: u32) {
        match self.mode.get() {
            Mode::Read => self.view.scroll_to_line(line),
            Mode::Edit => {
                self.editor.place_caret(line);
                self.editor.take_the_keyboard();
            }
        }
        // At once, rather than waiting for the page to report back: the reader pressed
        // the row and the row is where they are.
        self.outline.follow(line);
    }

    /// Does whatever the reader's click turned out to mean.
    fn act_on(self: &Rc<Self>, elsewhere: Follow) {
        match elsewhere {
            Follow::Stay | Follow::Refuse => {}
            Follow::Document { file, fragment } => {
                self.visit(&file, fragment.as_deref().unwrap_or_default());
            }
            Follow::Attachment { file } => {
                let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(&file)));
                launcher.launch(Some(&self.window), gio::Cancellable::NONE, move |opened| {
                    if let Err(error) = opened {
                        eprintln!("axiomd: {} could not be opened: {error}", file.display());
                    }
                });
            }
            Follow::External { uri } => {
                let launcher = gtk::UriLauncher::new(&uri);
                launcher.launch(Some(&self.window), gio::Cancellable::NONE, move |opened| {
                    if let Err(error) = opened {
                        eprintln!("axiomd: {uri} could not be opened: {error}");
                    }
                });
            }
            Follow::Ask(Request::LoadImage(source)) => self.fetch_image(source),
            Follow::Ask(Request::LoadAllImages) => {
                for source in self.view.unloaded_images() {
                    self.fetch_image(source);
                }
            }
            Follow::Ask(Request::ToggleTask(at)) => self.toggle_task(at),
        }
    }

    /// Ticks a task off, or un-ticks it: the reader pressed its box in the rendered
    /// document (issue #12).
    ///
    /// The edit goes into the buffer, which is the document (invariant 11) — so it is
    /// one step of the editor's own undo history, it marks the document unsaved,
    /// autosave picks it up like any other change, and the page is patched rather than
    /// reloaded, leaving the reader exactly where they were pressing.
    ///
    /// `at` is where the parser said the marker was in the source that page was
    /// rendered from. The buffer may have moved on since — the reader was typing in
    /// the other surface, the file changed underneath — so the byte is checked to be a
    /// checkbox before anything is written. A stale press does nothing rather than
    /// rewriting a character of somebody's prose.
    fn toggle_task(self: &Rc<Self>, at: usize) {
        self.pull_text();
        let text = self.document.borrow().text().to_owned();
        let Some(state) = ticked(&text, at) else {
            return;
        };
        self.editor.replace_marker(at, state);
        // Right away rather than at the end of the typing debounce: the reader pressed
        // a box and the box is what they are looking at.
        self.rerender_now();
    }

    /// The one thing axiomd fetches, and only because the reader pressed the card
    /// that says it will (D4).
    fn fetch_image(self: &Rc<Self>, source: String) {
        if self.view.has_image(&source) || self.fetching.borrow().contains(&source) {
            return;
        }
        let Some(session) = self.view.network_session() else {
            self.view
                .image_failed(&source, "This image cannot be requested.".to_owned());
            return;
        };
        self.fetching.borrow_mut().push(source.clone());

        let epoch = self.epoch.get();
        let window = Rc::downgrade(self);
        remote::load(&session, &remote::requestable(&source), move |arrived| {
            let Some(window) = window.upgrade() else {
                return;
            };
            window.fetching.borrow_mut().retain(|url| *url != source);
            // The reader has moved on to another document; this one is nobody's.
            if window.epoch.get() != epoch {
                return;
            }
            match arrived {
                Ok(image) => {
                    let uri = window.open.borrow().as_ref().map(|open| {
                        open.publication
                            .attach_image(image.body, image.content_type)
                    });
                    if let Some(uri) = uri {
                        window.view.image_arrived(&source, uri);
                    }
                }
                Err(complaint) => window.view.image_failed(&source, complaint),
            }
        });
    }

    /// Takes the buffer's text into the model. Costs a copy of the document, so it is
    /// done when something needs the text rather than when the text changes.
    fn pull_text(&self) {
        let text = self.editor.text();
        self.document.borrow_mut().holds(text);
    }

    /// Renders what the reader has in front of them, right now.
    fn rerender_now(&self) {
        self.pull_text();
        let (source, name, root) = {
            let document = self.document.borrow();
            (
                document.text().to_owned(),
                document.name(),
                document
                    .file()
                    .and_then(Path::parent)
                    .map(Path::to_path_buf),
            )
        };
        if let Some(open) = self.open.borrow().as_ref() {
            open.renderer
                .render(source, name, self.engine(), self.settings.plugins(), root);
        }
    }

    /// Renders once the reader stops typing.
    ///
    /// The same quiet period a save on disk gets, and for the same reason: a burst of
    /// keystrokes is one document, not one document per key.
    fn schedule_render(self: &Rc<Self>) {
        let mine = self.rerender.get() + 1;
        self.rerender.set(mine);
        let window = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            glib::timeout_future(QUIET).await;
            if let Some(window) = window.upgrade()
                && window.rerender.get() == mine
            {
                window.rerender_now();
            }
        });
    }

    /// Writes the document out once the reader stops typing, if they have asked for
    /// that (issue #20's `autosave` and `autosave-delay`).
    ///
    /// The delay is read when the timer is set rather than remembered, so changing it
    /// in preferences applies to the very next keystroke — no restart, no reopen
    /// (invariant 14).
    fn schedule_autosave(self: &Rc<Self>) {
        let Some(delay) = self.settings.autosave() else {
            return;
        };
        let mine = self.autosave.get() + 1;
        self.autosave.set(mine);
        let window = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            glib::timeout_future(delay).await;
            if let Some(window) = window.upgrade()
                && window.autosave.get() == mine
            {
                window.autosave_now();
            }
        });
    }

    /// Saves without being asked, when there is something to save and the reader has
    /// said they want this.
    ///
    /// Never for a document that has no file: asking where it goes is a question, and
    /// a question is exactly what an automatic save must not be. Never over a conflict
    /// either — the reader is being asked which version they want, and answering it
    /// for them by writing is the one thing that would lose their work.
    fn autosave_now(self: &Rc<Self>) {
        if self.settings.autosave().is_none() {
            return;
        }
        self.pull_text();
        let worth_writing = {
            let document = self.document.borrow();
            document.is_modified() && !document.needs_a_name() && !document.is_conflicted()
        };
        if worth_writing {
            self.write_out();
        }
    }

    /// `Ctrl+S`: the file, or the question of which file this is going to be.
    fn save(self: &Rc<Self>) {
        if self.document.borrow().needs_a_name() {
            self.save_as();
            return;
        }
        self.write_out();
    }

    fn write_out(self: &Rc<Self>) {
        self.pull_text();
        let written = self.document.borrow_mut().save();
        match written {
            Ok(()) => {
                self.notice.hide();
                self.retitle();
            }
            // A save the reader asked for and did not get is said where they are
            // looking, and stays there until something else happens.
            Err(trouble) => self.notice.say(
                &format!("{} — {}", trouble.title(), trouble.detail()),
                Vec::new(),
            ),
        }
    }

    /// `Ctrl+Shift+S`, and what the first `Ctrl+S` on an untitled document runs.
    ///
    /// A dialog, and a sanctioned one: the reader asked for something that cannot be
    /// done without knowing where (`ux_decisions.md`).
    fn save_as(self: &Rc<Self>) {
        let dialog = gtk::FileDialog::builder()
            .title("Save Document")
            .modal(true)
            .initial_name(with_a_markdown_name(&self.document.borrow().name()))
            .build();

        let window = Rc::downgrade(self);
        dialog.save(Some(&self.window), gio::Cancellable::NONE, move |chosen| {
            // A cancelled chooser is not an error, and never becomes a message.
            let Some(window) = window.upgrade() else {
                return;
            };
            if let Ok(file) = chosen
                && let Some(path) = file.path()
            {
                window.save_to(&path);
            }
        });
    }

    /// Writes the document to a file the reader has just chosen, and follows it from
    /// then on: it is this window's document now, watched and rendered from there.
    pub(crate) fn save_to(self: &Rc<Self>, file: &Path) {
        self.pull_text();
        let written = self.document.borrow_mut().save_as(file);
        if let Err(trouble) = written {
            self.notice.say(
                &format!("{} — {}", trouble.title(), trouble.detail()),
                Vec::new(),
            );
            return;
        }
        self.history.borrow_mut().restart(Visit {
            file: file.to_path_buf(),
            fragment: String::new(),
        });
        self.retrace();
        self.take_over(Some(file), "");
        if let Some(open) = self.open.borrow().as_ref() {
            open.loaded.set(true);
        }
        self.retitle();
        self.rerender_now();
    }

    /// `Ctrl+P`: the reader's own print dialog, over the page they are looking at.
    ///
    /// A dialog, and a sanctioned one: printing is something the reader just asked
    /// for, and what to print it on is a question only they can answer
    /// (`ux_decisions.md`). What comes back is said beside the document, never over
    /// it — and a reader who changes their mind is told nothing at all.
    fn print(self: &Rc<Self>) {
        let saying = Rc::downgrade(self);
        crate::export::print(
            &self.deliverable(),
            self.window.upcast_ref::<gtk::Window>(),
            move |outcome| {
                if let Some(window) = saying.upgrade() {
                    window.report(outcome, "Printed");
                }
            },
        );
    }

    /// `Ctrl+Shift+E`: the document as a file somebody else can open.
    ///
    /// Which format is the name the reader gives the file, chosen in the chooser they
    /// are already in — a PDF, or a page that carries everything it needs. There is no
    /// question afterwards and nothing to configure.
    fn export(self: &Rc<Self>) {
        let pdf = gtk::FileFilter::new();
        pdf.set_name(Some("PDF Document"));
        pdf.add_mime_type("application/pdf");
        pdf.add_pattern("*.pdf");
        let page = gtk::FileFilter::new();
        page.set_name(Some("Web Page"));
        page.add_mime_type("text/html");
        page.add_pattern("*.html");

        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&pdf);
        filters.append(&page);

        let stem = self.document.borrow().name();
        let stem = Path::new(&stem)
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or(stem.clone());
        let dialog = gtk::FileDialog::builder()
            .title("Export Document")
            .modal(true)
            .filters(&filters)
            .default_filter(&pdf)
            .initial_name(format!("{stem}.pdf"))
            .build();

        let window = Rc::downgrade(self);
        dialog.save(Some(&self.window), gio::Cancellable::NONE, move |chosen| {
            // A cancelled chooser is not an error, and never becomes a message.
            let Some(window) = window.upgrade() else {
                return;
            };
            if let Ok(file) = chosen
                && let Some(path) = file.path()
            {
                window.export_to(&path);
            }
        });
    }

    /// Writes this document to `file` — the far side of the export chooser.
    ///
    /// Returns at once, and says so beside the document while it happens: composing a
    /// page or paginating a PDF is work, and the window stays usable throughout
    /// (invariant 4).
    pub(crate) fn export_to(self: &Rc<Self>, file: &Path) {
        self.pull_text();
        self.notice
            .say(&format!("Exporting {}…", file_name(file)), Vec::new());

        let saying = Rc::downgrade(self);
        crate::export::write(&self.deliverable(), file, move |outcome| {
            if let Some(window) = saying.upgrade() {
                window.report(outcome, "Exported");
            }
        });
    }

    /// The document as the exporter needs it: the page on screen, and the buffer it
    /// was made from (invariant 11).
    fn deliverable(&self) -> crate::export::Document {
        let document = self.document.borrow();
        crate::export::Document {
            view: self.view.widget().clone(),
            source: document.text().to_owned(),
            name: document.name(),
            plugins: self.settings.plugins(),
            engine: self.engine(),
            root: document
                .file()
                .and_then(Path::parent)
                .map(Path::to_path_buf),
        }
    }

    /// Says how a print or an export ended, beside the document — the only place the
    /// app ever says anything about a document the reader is reading (invariant 12).
    fn report(&self, outcome: crate::export::Outcome, done: &str) {
        match outcome {
            crate::export::Outcome::Done(file) => {
                let what = file
                    .map(|file| file_name(&file))
                    .unwrap_or_else(|| self.document.borrow().name());
                self.notice.say(&format!("{done} {what}"), Vec::new());
            }
            // Nothing happened, so nothing is said — and whatever was being said
            // while it was happening stops.
            crate::export::Outcome::Cancelled => self.notice.hide(),
            crate::export::Outcome::Failed(trouble) => self
                .notice
                .say(&format!("{done} nothing — {trouble}"), Vec::new()),
        }
    }

    /// The question on the way out — sanctioned, because closing is something the
    /// reader just asked for (`ux_decisions.md`). With autosave on it is rare by
    /// construction: there is nothing unsaved to ask about.
    fn ask_about_unsaved_work(self: &Rc<Self>) {
        // No separate title: `AdwAlertDialog` makes the heading the dialog's title, so
        // what a test reads back is the sentence the reader is looking at.
        let dialog = adw::AlertDialog::builder()
            .heading(format!(
                "Save changes to {} before closing?",
                self.document.borrow().name()
            ))
            .body("If you don't save, your changes will be lost.")
            .close_response("cancel")
            .default_response("save")
            .build();
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("discard", "Discard");
        dialog.add_response("save", "Save");
        dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
        dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);

        let window = Rc::downgrade(self);
        dialog.connect_response(None, move |_, answer| {
            let Some(window) = window.upgrade() else {
                return;
            };
            match answer {
                "discard" => window.leave(),
                "save" => {
                    // Saving may itself be a question — an untitled document has to be
                    // given a name — so the window closes when there is nothing left
                    // unsaved rather than the moment the reader answers.
                    window.save();
                    if !window.document.borrow().is_modified() {
                        window.leave();
                    }
                }
                _ => {}
            }
        });
        dialog.present(Some(&self.window));
    }

    /// Closes the window the reader has finished answering about.
    ///
    /// On the next turn of the loop rather than here: this is called from inside the
    /// dialog's own response handler, and libadwaita is still taking that dialog off
    /// the window. A close asked for underneath it is answered by dismissing the
    /// dialog instead, and the window stays — which is the whole failure the reader
    /// would see as "I pressed Discard and nothing happened".
    fn leave(self: &Rc<Self>) {
        self.leaving.set(true);
        let window = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(window) = window.upgrade() {
                window.window.close();
            }
        });
    }

    /// Puts a finished page in front of the reader.
    ///
    /// `epoch` is the document the page was rendered for: a render that outlived the
    /// document that asked for it is dropped rather than shown, so a window given a
    /// second file can never flash the first one back.
    fn present_page(&self, page: Rendered, epoch: u64) {
        if epoch != self.epoch.get() {
            return;
        }
        // Beside the document before it is on screen: the outline is the page's own
        // heading map (invariant 3), never a second reading of the file.
        self.outline.show(page.outline());
        if let Some(open) = self.open.borrow().as_ref() {
            self.view.show(&open.publication, &page, &open.fragment);
            // A save may have been a replacement, which gives the path a new identity
            // on disk. The window follows it, or it stops recognising its own document
            // and opens a second window on it.
            if let Some(file) = &open.file {
                open.id.set(FileId::of(file));
            }
        }
        // The reader is typing; the page is kept current behind them and put on screen
        // when they ask for it, not instead of the editor they are in.
        if self.mode.get() == Mode::Read {
            self.surfaces.set_visible_child_name(DOCUMENT_PAGE);
        }
        self.renders.set(self.renders.get() + 1);
    }

    /// Says, inside the window, why there is nothing to read — never in a dialog.
    ///
    /// A reader who already has a document keeps it and hears about the trouble beside
    /// it; only a window with nothing on screen is given the status page.
    pub(crate) fn show_unavailable(&self, title: &str, detail: &str) {
        if self.showing() == DOCUMENT_PAGE {
            self.notice.say(
                &format!("{title} — showing the last version read"),
                Vec::new(),
            );
        } else {
            self.status.set_title(title);
            self.status.set_description(Some(detail));
            self.surfaces.set_visible_child_name(STATUS_PAGE);
            // There is no document, so there are no sections in it.
            self.outline.show(&[]);
        }
        self.renders.set(self.renders.get() + 1);
    }

    /// The window's name for its document: what it is called, where it lives, and
    /// whether there is work in it that is not on disk.
    fn retitle(&self) {
        let document = self.document.borrow();
        let where_it_lives = match document.file() {
            Some(file) => folder_of(file),
            None => "Not saved yet".to_owned(),
        };
        let name = document.name();
        let shown = if document.is_modified() {
            format!("• {name}")
        } else {
            name
        };
        drop(document);
        self.retitle_as(&shown, &where_it_lives);
    }

    fn retitle_as(&self, name: &str, where_it_lives: &str) {
        self.window.set_title(Some(name));
        self.title.set_title(name);
        self.title.set_subtitle(where_it_lives);
    }
}

/// The things a window action can be, so that one closure serves all of them.
#[derive(Clone, Copy)]
enum Deed {
    Save,
    SaveAs,
    Undo,
    Redo,
    Print,
    Export,
}

/// What a task list marker at `at` should become when the reader presses it, or `None`
/// when `at` is not a marker at all any more.
///
/// The three bytes have to spell a checkbox — `[`, its state, `]` — because the only
/// thing that says the source has not moved under the page is what the source says.
fn ticked(text: &str, at: usize) -> Option<char> {
    let bytes = text.as_bytes();
    if at == 0 || bytes.get(at - 1) != Some(&b'[') || bytes.get(at + 1) != Some(&b']') {
        return None;
    }
    match bytes.get(at) {
        Some(b' ') => Some('x'),
        Some(b'x') | Some(b'X') => Some(' '),
        _ => None,
    }
}

/// An action's bare name, as a window registers it, from the full one a widget uses.
fn bare(action: &str) -> &str {
    action.strip_prefix("win.").unwrap_or(action)
}

/// What the Save As dialog offers as a name. An untitled document is offered a
/// Markdown one, because that is the only kind axiomd writes.
fn with_a_markdown_name(name: &str) -> String {
    if Path::new(name).extension().is_some() {
        name.to_owned()
    } else {
        format!("{name}.md")
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

/// What the app has to say about a document the reader is still reading, said beside
/// it: a sentence, and the whole of the choice as buttons next to it.
///
/// An `AdwBanner` would do for the sentence, but it carries exactly one button, and
/// the case that matters most — unsaved work meeting a changed file — is a choice
/// between two things. A choice with one of its halves missing is not a choice.
struct Notice {
    revealer: gtk::Revealer,
    label: gtk::Label,
    choices: gtk::Box,
}

impl Notice {
    fn new() -> Notice {
        let label = gtk::Label::builder()
            .hexpand(true)
            .xalign(0.0)
            .wrap(true)
            .build();
        let choices = gtk::Box::builder().spacing(6).build();

        let bar = gtk::Box::builder()
            .spacing(12)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(12)
            .margin_end(12)
            .build();
        bar.append(&label);
        bar.append(&choices);
        bar.add_css_class("toolbar");

        let revealer = gtk::Revealer::builder()
            .child(&bar)
            .reveal_child(false)
            .build();
        Notice {
            revealer,
            label,
            choices,
        }
    }

    fn widget(&self) -> &gtk::Widget {
        self.revealer.upcast_ref()
    }

    /// Says `message` beside the document, offering `choices` — each of which is a
    /// button that does the thing it is labelled with.
    fn say(&self, message: &str, choices: Vec<(String, Rc<dyn Fn()>)>) {
        self.clear_choices();
        self.label.set_label(message);
        for (label, act) in choices {
            let button = gtk::Button::with_label(&label);
            button.connect_clicked(move |_| act());
            self.choices.append(&button);
        }
        self.revealer.set_reveal_child(true);
    }

    fn hide(&self) {
        self.revealer.set_reveal_child(false);
        self.clear_choices();
        self.label.set_label("");
    }

    fn clear_choices(&self) {
        while let Some(child) = self.choices.first_child() {
            self.choices.remove(&child);
        }
    }

    /// What the window is saying, or an empty string when it is saying nothing.
    fn message(&self) -> String {
        if self.revealer.reveals_child() {
            self.label.label().to_string()
        } else {
            String::new()
        }
    }
}

/// One of the two history buttons. It is bound to a window action, so GTK makes it
/// insensitive exactly when there is nowhere that way to go.
fn step_button(icon: &str, tooltip: &str, action: &str) -> gtk::Button {
    gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .action_name(action)
        .build()
}

fn open_button() -> gtk::Button {
    gtk::Button::builder()
        .icon_name("document-open-symbolic")
        .tooltip_text("Open a document")
        .action_name("app.open")
        .build()
}

/// The switch for the outline sidebar. A toggle, because it says whether the sidebar
/// is there as well as putting it there — including when a window too narrow to hold
/// it is what took it away.
fn outline_button() -> gtk::ToggleButton {
    gtk::ToggleButton::builder()
        .icon_name("view-list-symbolic")
        .tooltip_text("Show the outline (F9)")
        .action_name(OUTLINE)
        .build()
}

/// The switch between reading and editing. A toggle rather than a button, because it
/// says which of the two the window is in as well as changing it.
fn mode_button() -> gtk::ToggleButton {
    gtk::ToggleButton::builder()
        .icon_name("document-edit-symbolic")
        .tooltip_text("Edit the source (Ctrl+E)")
        .action_name(MODE)
        .build()
}

fn primary_menu_button(zoom: &Rc<Zoom>) -> gtk::MenuButton {
    let documents = gio::Menu::new();
    documents.append(Some("_New Window"), Some("app.new"));
    documents.append(Some("_Open…"), Some("app.open"));

    // How big the document is, shown the way every GNOME application shows it: a row
    // of its own rather than menu items, because the reader steps it several times in
    // a row and a menu that closed after each step would be unusable. A menu model
    // carries it as a named slot the popover fills with a real widget.
    let scale = gio::Menu::new();
    let slot = gio::MenuItem::new(None, None);
    slot.set_attribute_value("custom", Some(&ZOOM_SLOT.to_variant()));
    scale.append_item(&slot);

    let reading = gio::Menu::new();
    reading.append(Some("_Find…"), Some(crate::find::FIND));
    // Which engine this window reads with (issue #17). A submenu of the main menu
    // rather than something buried: it is two presses from any document, and every
    // engine this build has is in it, named by the engine itself.
    let parsers = gio::Menu::new();
    for engine in axiomd_engine::engines() {
        let item = gio::MenuItem::new(Some(engine.id().as_str()), None);
        item.set_action_and_target_value(Some(ENGINE), Some(&engine.id().as_str().to_variant()));
        parsers.append_item(&item);
    }
    reading.append_submenu(Some("Markdown _Engine"), &parsers);

    let editing = gio::Menu::new();
    editing.append(Some("_Edit Source"), Some(MODE));
    editing.append(Some("_Save"), Some(SAVE));
    editing.append(Some("Save _As…"), Some(SAVE_AS));

    let leaving = gio::Menu::new();
    leaving.append(Some("_Print…"), Some(PRINT));
    leaving.append(Some("E_xport…"), Some(EXPORT));

    let application = gio::Menu::new();
    application.append(Some("_Preferences"), Some("app.preferences"));
    application.append(Some("_Close Window"), Some("app.close-window"));
    application.append(Some("_Quit"), Some("app.quit"));

    let menu = gio::Menu::new();
    menu.append_section(None, &documents);
    menu.append_section(None, &scale);
    menu.append_section(None, &reading);
    menu.append_section(None, &editing);
    menu.append_section(None, &leaving);
    menu.append_section(None, &application);

    let button = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .tooltip_text("Main menu")
        .menu_model(&menu)
        .build();
    // Setting the model builds the popover, so the slot can be filled straight away
    // (probed on GTK 4.20.4: the popover is a `GtkPopoverMenu` and `add_child` answers
    // true). Without this the row would be a gap in the menu.
    if let Some(popover) = button.popover().and_downcast::<gtk::PopoverMenu>() {
        popover.add_child(zoom.indicator(), ZOOM_SLOT);
    }
    button
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visit(name: &str) -> Visit {
        Visit {
            file: PathBuf::from(name),
            fragment: String::new(),
        }
    }

    fn names(history: &History) -> Vec<String> {
        history
            .entries
            .iter()
            .map(|visit| visit.file.display().to_string())
            .collect()
    }

    /// What the two header-bar buttons can do is exactly this, so a button that
    /// looks available when there is nowhere to go starts here.
    #[test]
    fn where_the_reader_can_go_is_where_they_have_been() {
        let mut history = History::default();
        assert!(!history.can_step(false), "back from nowhere");
        assert!(!history.can_step(true), "forward from nowhere");

        history.restart(visit("guide.md"));
        assert!(!history.can_step(false), "back from the first document");
        assert!(!history.can_step(true));

        history.follow(visit("notes.md"));
        assert!(history.can_step(false));
        assert!(!history.can_step(true), "forward from the newest document");

        assert_eq!(
            history.step(false).map(|visit| visit.file),
            Some("guide.md".into())
        );
        assert!(!history.can_step(false));
        assert!(history.can_step(true));

        assert_eq!(
            history.step(true).map(|visit| visit.file),
            Some("notes.md".into())
        );
        assert!(
            history.step(true).is_none(),
            "there was a way forward from the newest document",
        );
    }

    /// Following a link from where the reader went back to discards the way forward,
    /// as every back-and-forward has worked since Mosaic.
    #[test]
    fn following_a_link_from_the_middle_leaves_the_branch_behind() {
        let mut history = History::default();
        history.restart(visit("a.md"));
        history.follow(visit("b.md"));
        history.follow(visit("c.md"));
        history.step(false);
        history.step(false);

        history.follow(visit("d.md"));

        assert_eq!(names(&history), ["a.md", "d.md"]);
        assert!(history.can_step(false));
        assert!(
            !history.can_step(true),
            "a branch the reader left came back"
        );
    }

    /// Being given a document is not following a link to one: the window is showing
    /// something new, and there is nowhere behind it inside axiomd.
    #[test]
    fn opening_a_document_starts_the_window_over() {
        let mut history = History::default();
        history.restart(visit("a.md"));
        history.follow(visit("b.md"));

        history.restart(visit("c.md"));

        assert_eq!(names(&history), ["c.md"]);
        assert!(!history.can_step(false));
        assert!(!history.can_step(true));
    }

    /// Save As offers a name axiomd can actually write. An untitled document has no
    /// extension to keep, and one that has been saved before keeps its own.
    #[test]
    fn save_as_offers_a_markdown_name_for_a_document_that_has_none() {
        assert_eq!(with_a_markdown_name("Untitled"), "Untitled.md");
        assert_eq!(with_a_markdown_name("notes.md"), "notes.md");
        assert_eq!(with_a_markdown_name("README.markdown"), "README.markdown");
    }

    /// Pressing a box rewrites one character, and only when that character is still
    /// the box the page was rendered from. The reader may have typed in the other
    /// surface, or the file may have changed underneath — a press that arrives against
    /// a source that has moved must do nothing rather than damage a word of prose.
    #[test]
    fn a_press_only_ever_turns_a_checkbox_the_other_way_round() {
        let source = "- [ ] not done\n- [x] done\n- [X] shouted\n";

        assert_eq!(ticked(source, 3), Some('x'));
        assert_eq!(ticked(source, 18), Some(' '));
        assert_eq!(ticked(source, 29), Some(' '));

        // Everywhere else in the same document.
        for at in 0..source.len() {
            if [3, 18, 29].contains(&at) {
                continue;
            }
            assert_eq!(
                ticked(source, at),
                None,
                "offset {at} ({:?}) was taken for a checkbox",
                &source[at..(at + 1).min(source.len())],
            );
        }

        // And against a document that has moved on since the page was rendered.
        assert_eq!(ticked("nothing here at all\n", 3), None);
        assert_eq!(ticked("", 3), None);
        assert_eq!(ticked(source, source.len() + 100), None);
        // A bracketed thing that is not a checkbox is not one.
        assert_eq!(ticked("- [-] partly\n", 3), None);
    }

    /// An action is named once, in full, and registered by its bare name. A window
    /// that registered the full name would have every accelerator silently do nothing.
    #[test]
    fn every_window_action_is_registered_under_the_name_its_shortcut_uses() {
        for action in [
            BACK, FORWARD, MODE, OUTLINE, SAVE, SAVE_AS, UNDO, REDO, ENGINE,
        ] {
            assert!(
                action.starts_with("win."),
                "{action} is not a window action",
            );
            assert_eq!(bare(action), &action["win.".len()..]);
        }
    }
}
