//! The surface the reader types on, and the one rule it keeps: a keystroke costs a
//! keystroke.
//!
//! Nothing here parses, renders or writes anything. A key press reaches the buffer and
//! is echoed, and all the editor does about it is say *that* something changed
//! ([`Editor::connect_changed`]). Whoever listens decides when the text is worth
//! taking, and takes it with [`Editor::text`] — so the cost of a keystroke is the same
//! in a ten-megabyte document as in an empty one, however expensive rendering that
//! document happens to be (issue #18, "typing latency is independent of render cost").
//!
//! # Where the reader is
//!
//! [`Editor::caret_line`] and [`Editor::place_caret`] are the editing half of the
//! span map. Switching modes maps a source line to a rendered anchor and back
//! (`window.rs`), and these two are the end of that mapping that a caret has.
//!
//! # Searching the source
//!
//! The editor is the other surface the search bar drives ([`crate::find`]), and what it
//! searches is the source as the reader wrote it — markup included, because in edit
//! mode the markup is what they are looking at and what they came to change. Matches
//! are two text tags over the buffer, so nothing is re-parsed and the text itself is
//! never touched; the tags' colours follow the reader's colour scheme, which is the one
//! thing a text tag cannot get from a stylesheet.
//!
//! # What is not here yet
//!
//! Syntax highlighting and spell checking. Both need GtkSourceView 5 and libspelling,
//! whose development packages are not installable in this environment; the rest of the
//! editor — the buffer, undo, the caret, the modified state — is the same either way,
//! and this module is the only place that changes when they arrive. The search is
//! written over `GtkTextBuffer` for the same reason: a `GtkSourceSearchContext` is not
//! reachable from this build, and the rule it would apply is stated once in
//! [`crate::find::Query::matches`] and shared with the rendered page (issue #8, and
//! reported to the owner).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use crate::find::{Counted, Query, Searchable};

/// The two things a search does to the source: every match marked, and the one the
/// reader is on marked differently.
const MATCH: &str = "axiomd-find";
const CURRENT_MATCH: &str = "axiomd-find-current";

/// One window's editing surface.
pub(crate) struct Editor {
    scroller: gtk::ScrolledWindow,
    view: gtk::TextView,
    buffer: gtk::TextBuffer,
    changed: RefCell<Option<Rc<dyn Fn()>>>,
    moved: RefCell<Option<Moved>>,
    /// Set while the *application* is putting text in the buffer — opening a document,
    /// following the file, taking the version on disk. Those are not the reader typing
    /// and must not mark the document modified or start an autosave.
    filling: Rc<Cell<bool>>,
    /// The search the source is showing, or `None` when the reader is not searching.
    finding: RefCell<Option<Finding>>,
    /// Where its matches are, in characters, so that walking them costs no more than
    /// moving one tag. Emptied whenever the text changes, which is the only thing that
    /// can make them wrong.
    found: RefCell<Vec<(usize, usize)>>,
    /// Which scheduled re-search is the current one. Every keystroke supersedes the one
    /// before it, so a burst of typing is one search at the end rather than one per key.
    researching: Cell<u64>,
    /// This surface's subscription to the reader's colour scheme, which ends with the
    /// window rather than outliving it on a store that belongs to the application
    /// (invariant 7).
    recolouring: RefCell<Option<glib::SignalHandlerId>>,
}

/// A search the source is showing.
struct Finding {
    looking_for: Query,
    /// Which match is the current one, counting from zero in source order.
    nth: usize,
    counted: Counted,
}

/// What the window does when the caret lands on another line.
type Moved = Rc<dyn Fn(u32)>;

impl Editor {
    pub(crate) fn new() -> Rc<Self> {
        let buffer = gtk::TextBuffer::new(None);
        buffer.set_enable_undo(true);

        let view = gtk::TextView::builder()
            .buffer(&buffer)
            .monospace(true)
            .wrap_mode(gtk::WrapMode::WordChar)
            .left_margin(24)
            .right_margin(24)
            .top_margin(18)
            .bottom_margin(18)
            .build();

        let scroller = gtk::ScrolledWindow::builder()
            .child(&view)
            .hexpand(true)
            .vexpand(true)
            .build();

        // Created here and once, in this order, so that the current match's colours win
        // over the ones every match gets: a tag added later to the same table has the
        // higher priority.
        buffer.create_tag(Some(MATCH), &[]);
        buffer.create_tag(Some(CURRENT_MATCH), &[]);

        let editor = Rc::new(Self {
            scroller,
            view,
            buffer,
            changed: RefCell::new(None),
            moved: RefCell::new(None),
            filling: Rc::new(Cell::new(false)),
            finding: RefCell::new(None),
            found: RefCell::new(Vec::new()),
            researching: Cell::new(0),
            recolouring: RefCell::new(None),
        });
        editor.recolour_matches();

        // The highlight follows the reader's colour scheme, because a text tag carries
        // colours rather than a style class and nothing else would restyle it. Live,
        // and without the buffer being touched: the same rule the rendered document is
        // held to (invariant 9).
        let recolouring = Rc::downgrade(&editor);
        *editor.recolouring.borrow_mut() =
            Some(adw::StyleManager::default().connect_dark_notify(move |_| {
                if let Some(editor) = recolouring.upgrade() {
                    editor.recolour_matches();
                }
            }));

        // The editing end of "which section is the reader in": the caret moving is
        // the editor's answer to the page's scroll (issue #7). It costs an integer
        // comparison per keystroke and per click, and nothing else.
        let moved = Rc::downgrade(&editor);
        editor.buffer.connect_cursor_position_notify(move |_| {
            let Some(editor) = moved.upgrade() else {
                return;
            };
            let handler = editor.moved.borrow().clone();
            if let Some(handler) = handler {
                handler(editor.caret_line());
            }
        });

        let typed = Rc::downgrade(&editor);
        editor.buffer.connect_changed(move |_| {
            let Some(editor) = typed.upgrade() else {
                return;
            };
            // Whatever changed the text — the reader, or the application putting a
            // document in the buffer — the matches found in the old text are no longer
            // where they were, so the search is run again over what is there now. Once
            // they stop, not once per key: see `search_again`.
            editor.found.borrow_mut().clear();
            editor.search_again();
            if editor.filling.get() {
                return;
            }
            let handler = editor.changed.borrow().clone();
            if let Some(handler) = handler {
                handler();
            }
        });

        editor
    }

    pub(crate) fn widget(&self) -> &gtk::Widget {
        self.scroller.upcast_ref()
    }

    /// Calls `handler` whenever the reader changes the text — and never when the
    /// application does.
    pub(crate) fn connect_changed(&self, handler: impl Fn() + 'static) {
        *self.changed.borrow_mut() = Some(Rc::new(handler));
    }

    /// Calls `handler` with the line the caret has moved to — however it moved: a
    /// click, an arrow key, typing, or the application putting it somewhere.
    pub(crate) fn connect_moved(&self, handler: impl Fn(u32) + 'static) {
        *self.moved.borrow_mut() = Some(Rc::new(handler));
    }

    /// What the reader has written. Costs a copy of the document, so it is asked for
    /// when something needs it rather than when something changes.
    pub(crate) fn text(&self) -> String {
        self.buffer
            .text(&self.buffer.start_iter(), &self.buffer.end_iter(), true)
            .to_string()
    }

    /// Puts `text` in the buffer as the application's doing rather than the reader's:
    /// no change is reported, and the undo history starts again from here because
    /// there is nothing about the previous document left to undo.
    pub(crate) fn fill(&self, text: &str) {
        if self.text() == text {
            return;
        }
        self.filling.set(true);
        self.buffer.set_enable_undo(false);
        self.buffer.set_text(text);
        self.buffer.set_enable_undo(true);
        self.filling.set(false);
    }

    /// The line the caret is on, counting from one as the source does.
    pub(crate) fn caret_line(&self) -> u32 {
        self.buffer
            .iter_at_mark(&self.buffer.get_insert())
            .line()
            .saturating_add(1) as u32
    }

    /// Puts the caret on `line` and brings it into view — where read mode hands the
    /// reader over to edit mode.
    ///
    /// The scroll is deferred to the next turn of the loop because a view that has not
    /// been given its size yet has nowhere to scroll to, and switching mode is exactly
    /// the moment the editor is first given one.
    pub(crate) fn place_caret(self: &Rc<Self>, line: u32) {
        let wanted = line.saturating_sub(1) as i32;
        let last = self.buffer.line_count().saturating_sub(1);
        let at = self
            .buffer
            .iter_at_line(wanted.min(last))
            .unwrap_or_else(|| self.buffer.start_iter());
        self.buffer.place_cursor(&at);

        // Through the caret's own mark rather than the iterator above: an iterator is
        // invalidated by any change to the buffer, and between this turn of the loop
        // and the next the reader may well have typed.
        let editor = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(editor) = editor.upgrade() {
                editor.view.scroll_to_mark(
                    &editor.buffer.get_insert(),
                    0.0,
                    true,
                    0.0,
                    EDITING_LINE_FROM_THE_TOP,
                );
            }
        });
    }

    /// Types `text` where the caret is, exactly as pressing the keys does — the
    /// insertion the buffer sees is the same one, so it is undoable, it marks the
    /// document modified, and it starts the same debounce.
    pub(crate) fn type_text(&self, text: &str) {
        self.buffer.insert_at_cursor(text);
    }

    /// Undo and redo, as `Ctrl+Z` and `Ctrl+Y` ask for them.
    pub(crate) fn undo(&self) {
        self.buffer.undo();
    }

    pub(crate) fn redo(&self) {
        self.buffer.redo();
    }

    /// Puts the keyboard in the editor, so that switching to edit mode is enough to
    /// start typing.
    pub(crate) fn take_the_keyboard(&self) {
        self.view.grab_focus();
    }

    /// What the reader can see highlighted in the source, in order, with the match they
    /// are on marked out from the rest.
    ///
    /// The editing half of reading `mark.axiomd-find` out of the rendered page: the
    /// only way to ask what a search actually did to the text in front of the reader.
    pub(crate) fn highlighted(&self) -> Vec<String> {
        let found = self.found.borrow().clone();
        if found.is_empty() {
            return Vec::new();
        }
        let current = self
            .finding
            .borrow()
            .as_ref()
            .map(|finding| finding.nth % found.len());
        found
            .iter()
            .enumerate()
            .map(|(index, (from, to))| {
                let text = self.slice(*from, *to);
                if Some(index) == current {
                    format!(">{text}")
                } else {
                    text
                }
            })
            .collect()
    }

    /// Runs the search again once the reader stops typing.
    ///
    /// Searching a document is work proportional to the document, and a keystroke must
    /// never pay for it (issue #18: typing costs a keystroke, whatever the document
    /// costs). So a burst of keys is one search at the end rather than one per key —
    /// the same quiet period and the same reason as the re-render the window schedules.
    /// In between, the marks the reader can see move with the text they are typing in,
    /// because that is what a text tag does.
    fn search_again(self: &Rc<Self>) {
        if self.finding.borrow().is_none() {
            return;
        }
        let mine = self.researching.get() + 1;
        self.researching.set(mine);
        let editor = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            glib::timeout_future(crate::watch::QUIET).await;
            if let Some(editor) = editor.upgrade()
                && editor.researching.get() == mine
            {
                editor.mark_matches(false);
            }
        });
    }

    /// Runs the search the bar has given this surface over the text as it stands.
    ///
    /// `bring` is whether the reader asked to be taken to the current match: they did
    /// when they typed or pressed Next, and they did not when the document changed
    /// under them — a live reload must not move a caret they left somewhere.
    fn mark_matches(&self, bring: bool) {
        let Some((looking_for, nth, counted)) = self.finding.borrow().as_ref().map(|finding| {
            (
                finding.looking_for.clone(),
                finding.nth,
                finding.counted.clone(),
            )
        }) else {
            return;
        };

        if self.found.borrow().is_empty() {
            let text: Vec<char> = self.text().chars().collect();
            *self.found.borrow_mut() = looking_for.matches(&text);
        }
        let found = self.found.borrow().clone();

        self.unmark();
        for (from, to) in &found {
            self.buffer
                .apply_tag_by_name(MATCH, &self.at(*from), &self.at(*to));
        }

        if !found.is_empty() {
            let (from, to) = found[nth % found.len()];
            self.buffer
                .apply_tag_by_name(CURRENT_MATCH, &self.at(from), &self.at(to));
            if bring {
                // The caret goes with the reader: pressing Escape leaves them at the
                // word they searched for rather than back where they started.
                self.buffer.place_cursor(&self.at(from));
                self.reveal_the_caret();
            }
        }
        counted(&looking_for, found.len());
    }

    /// Takes the marks off the source, leaving the text untouched.
    fn unmark(&self) {
        let (start, end) = (self.buffer.start_iter(), self.buffer.end_iter());
        self.buffer.remove_tag_by_name(MATCH, &start, &end);
        self.buffer.remove_tag_by_name(CURRENT_MATCH, &start, &end);
    }

    /// Brings the caret into the middle of the editor — where a reader who was taken
    /// somewhere by a search expects to find what they searched for.
    ///
    /// On the next turn of the loop, and through the caret's own mark: an iterator is
    /// invalidated by any change to the buffer, and a view that has only just been
    /// given its size has nowhere to scroll to yet.
    fn reveal_the_caret(&self) {
        let view = self.view.clone();
        let buffer = self.buffer.clone();
        glib::idle_add_local_once(move || {
            view.scroll_to_mark(&buffer.get_insert(), 0.0, true, 0.0, 0.5);
        });
    }

    fn at(&self, offset: usize) -> gtk::TextIter {
        self.buffer
            .iter_at_offset(i32::try_from(offset).unwrap_or(i32::MAX))
    }

    fn slice(&self, from: usize, to: usize) -> String {
        self.buffer
            .text(&self.at(from), &self.at(to), true)
            .to_string()
    }

    /// The colours a search marks the source with, under the scheme the reader is
    /// reading in.
    ///
    /// The same two colours the rendered page uses (`axiomd.css`), so a match looks
    /// like a match whichever surface the reader is on.
    fn recolour_matches(&self) {
        let dark = adw::StyleManager::default().is_dark();
        let table = self.buffer.tag_table();
        for (name, background, foreground) in [
            (MATCH, if dark { "#7a5c00" } else { "#f9f06b" }, ink(dark)),
            (
                CURRENT_MATCH,
                if dark { "#c64600" } else { "#ff7800" },
                ink(dark),
            ),
        ] {
            if let Some(tag) = table.lookup(name) {
                tag.set_background(Some(background));
                tag.set_foreground(Some(foreground));
            }
        }
    }
}

/// What is legible on both of those, which is not the same colour on both.
fn ink(dark: bool) -> &'static str {
    if dark { "#ffffff" } else { "#241f31" }
}

impl Searchable for Editor {
    /// The source, searched as the reader wrote it: the markup counts, because in edit
    /// mode the markup is what they are looking at and what they came to change.
    fn show_matches(&self, looking_for: &Query, nth: usize, bring: bool, counted: Counted) {
        let changed = self
            .finding
            .borrow()
            .as_ref()
            .is_none_or(|finding| finding.looking_for != *looking_for);
        if changed {
            self.found.borrow_mut().clear();
        }
        *self.finding.borrow_mut() = Some(Finding {
            looking_for: looking_for.clone(),
            nth,
            counted,
        });
        self.mark_matches(bring);
    }

    fn hide_matches(&self) {
        self.finding.borrow_mut().take();
        self.found.borrow_mut().clear();
        self.unmark();
    }
}

impl Drop for Editor {
    fn drop(&mut self) {
        if let Some(handler) = self.recolouring.borrow_mut().take() {
            adw::StyleManager::default().disconnect(handler);
        }
    }
}

/// Where the line the reader was reading lands in the editor: a little below the top,
/// so the lines above it are still there to be seen.
const EDITING_LINE_FROM_THE_TOP: f64 = 0.15;
