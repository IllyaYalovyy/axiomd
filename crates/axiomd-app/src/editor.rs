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
//! `GtkSourceSearchContext` is deliberately not used, although the buffer under this
//! module is now a source buffer that has one. Its case-insensitive matching folds
//! case the way Unicode does, and axiomd's — stated once in
//! [`crate::find::Query::matches`] and shared with the rendered page by `find.js` —
//! compares character by character. Measured on GtkSourceView 5.18.0: searching
//! `strasse` over `Straße STRASSE strasse` finds three matches there and two here, so
//! adopting it would make the counter disagree with itself between the two ways of
//! looking at one document (issue #8's single shared rule, and issue #21's condition
//! for the migration).
//!
//! # How the source is drawn
//!
//! GtkSourceView's own Markdown definition, as it ships, under the Adwaita scheme that
//! matches the reader's colour scheme — a heading bold and coloured, an emphasis
//! slanted, a code span apart from the prose, and nothing else. Deliberately minimal:
//! live-preview styling was ruled out for #21, and nothing here hides, replaces or
//! re-renders any part of the markup the reader typed.
//!
//! Highlighting and spell checking are both *ways of drawing* the buffer. Neither
//! changes a byte of it, and switching palette re-draws where the reader stands rather
//! than reloading anything (invariant 9).
//!
//! # What is spell checked
//!
//! The reader's preference, while they are editing, in a document short enough for
//! checking to cost nothing they can feel ([`LARGEST_CHECKED_DOCUMENT`]) — unless this
//! window's reader has used the Check Spelling row of the editor's own context menu,
//! which libspelling ships in it. That row is this window's alone: it holds for the
//! life of the window, nothing is written down, and every other window goes on
//! following the preference (owner ruling, issue #59; the same shape as the engine a
//! window has been switched to in `window.rs`). The corrections beside it are
//! libspelling's as it ships them — see [`corrections_follow_the_popup`].

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use sourceview5::prelude::*;

use crate::find::{Counted, Query, Searchable};

/// The two things a search does to the source: every match marked, and the one the
/// reader is on marked differently.
const MATCH: &str = "axiomd-find";
const CURRENT_MATCH: &str = "axiomd-find-current";

/// The language the source is drawn in, and the two schemes it is drawn under — all
/// three GtkSourceView's own, as it ships them.
const MARKDOWN: &str = "markdown";
const LIGHT: &str = "Adwaita";
const DARK: &str = "Adwaita-dark";

/// The biggest document whose spelling is checked, in characters.
///
/// Spell checking is the one thing here whose cost grows with the whole document
/// rather than with what is on screen, and past a point it is a cost the reader pays
/// in a main loop that stops answering. Measured on this machine on 2026-08-03, in a
/// release build, as the longest the application took to answer anything while an edit
/// reached the screen — the number invariant 4 is stated in:
///
/// | document | not checked | checked |
/// |----------|-------------|---------|
/// | 64 KB    | 6.2 ms      | 5.7 ms  |
/// | 256 KB   | 28.2 ms     | 16.4 ms |
/// | 1 MB     | 69.7 ms     | 53.6 ms |
/// | 4 MB     | 578.8 ms    | 683.2 ms |
/// | 7 MB     | 1449.2 ms   | 1800.7 ms |
/// | 10 MB    | 2648.8 ms   | 3324.9 ms |
///
/// Up to a megabyte checking costs nothing measurable; above it the cost is the
/// document's, and on the perf harness's ten-megabyte budget it was the difference
/// between 2.8 s and 4.8 s — a stall budget met and a stall budget missed (issue #9,
/// invariant 8). So a document longer than this is highlighted, edited and searched as
/// any other, and simply not spell checked.
///
/// Nothing tells the reader, which is a decision worth revisiting: an inline note
/// beside a document too long to check would be the honest version of this.
const LARGEST_CHECKED_DOCUMENT: i32 = 1_000_000;

/// One window's editing surface.
pub(crate) struct Editor {
    scroller: gtk::ScrolledWindow,
    view: sourceview5::View,
    buffer: sourceview5::Buffer,
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
    /// What underlines the words this window's reader has misspelled. One per window
    /// and owned by it, so closing the window ends the checking with it (invariant 7).
    spelling: libspelling::TextBufferAdapter,
    /// Whether this surface has been shown to the reader, and so whether the palette
    /// it is drawn in has been read off disk yet. See [`Editor::dress`].
    dressed: Cell<bool>,
    /// Whether the reader has asked for their spelling to be checked at all. What is
    /// actually checked is that *and* this surface being on screen — see
    /// [`Editor::check_spelling`].
    checking: Cell<bool>,
    /// What this window's reader said with the Check Spelling row of the editor's own
    /// menu, or `None` while this window still follows the preference above. The
    /// window's own, for as long as the window lives — see [`Editor::new`].
    switched: Cell<Option<bool>>,
    /// What this editor last asked of the adapter, which is the only way a change it
    /// did not make — the reader pressing that row — can be told from its own.
    marking: Cell<bool>,
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
        let buffer = sourceview5::Buffer::new(None);
        buffer.set_enable_undo(true);
        buffer.set_highlight_syntax(true);
        // Off: the pairing rectangle GtkSourceView draws around brackets is a code
        // editor's habit, and in prose full of parentheses it is a flicker under the
        // caret with nothing to say (owner ruling on #21: minimal).
        buffer.set_highlight_matching_brackets(false);

        let view = sourceview5::View::builder()
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

        // Off until somebody says otherwise: the reader's preference is answered by
        // the window that owns this editor, and a document being read is not being
        // spell checked at all.
        //
        // A checker of this window's own rather than libspelling's process-wide
        // default, because the Languages submenu below sets the language on the
        // checker: with the shared one, choosing a language in one window would
        // re-check every other window in it (invariant 7).
        let spelling =
            libspelling::TextBufferAdapter::new(&buffer, &libspelling::Checker::new(None, None));
        spelling.set_enabled(false);

        // The other half of libspelling's contract, as it ships (issue #59): the
        // corrections for the word under the caret, Add to Dictionary, Ignore and the
        // Languages submenu, in the source view's own context menu, pressing the
        // adapter's own actions. What the word is, what it could be and what happens
        // when one is chosen are all the adapter's; nothing here knows any of it.
        view.set_extra_menu(Some(&spelling.menu_model()));
        view.insert_action_group("spelling", Some(&spelling));
        corrections_follow_the_popup(&view, &spelling);

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
            spelling,
            checking: Cell::new(false),
            switched: Cell::new(None),
            marking: Cell::new(false),
            dressed: Cell::new(false),
            recolouring: RefCell::new(None),
        });
        editor.repaint();

        // "While they are editing" said in the only terms this module has for it: the
        // editor is on screen. The stack maps the surface the reader is looking at and
        // unmaps the other, so a document being read is never spell checked and a
        // reader who switches to editing is, without the editor having to be told what
        // a mode is.
        let shown = Rc::downgrade(&editor);
        editor.view.connect_map(move |_| {
            if let Some(editor) = shown.upgrade() {
                editor.dress();
                editor.mark_misspellings();
            }
        });
        let hidden = Rc::downgrade(&editor);
        editor.view.connect_unmap(move |_| {
            if let Some(editor) = hidden.upgrade() {
                editor.mark_misspellings();
            }
        });

        // The Check Spelling row of libspelling's menu, which is part of the section as
        // it ships. Owner ruling (2026-10-16, issue #59): that row belongs to this
        // window and to nothing else. Pressing it stops — or starts — the marking here
        // for as long as the window lives; the reader's preference is neither written
        // to nor read back over it, and every other window goes on following the
        // preference the moment it changes. The same shape as the engine a window has
        // been switched to (`window.rs`).
        //
        // Noticed here rather than acted on where the row is, because the row is
        // libspelling's own: its action toggles the adapter directly
        // (`spelling_enabled_action`, 0.4.9, which flips `get_enabled` rather than the
        // action's state, so it always flips the value in force), and nothing of
        // axiomd's sits between the two. What reaches this handler is therefore every
        // change of the adapter's `enabled` — including the ones this editor makes
        // itself, which are the ones it has just recorded in `marking`.
        let switched = Rc::downgrade(&editor);
        editor.spelling.connect_enabled_notify(move |spelling| {
            let Some(editor) = switched.upgrade() else {
                return;
            };
            if spelling.is_enabled() == editor.marking.get() {
                return;
            }
            editor.switched.set(Some(spelling.is_enabled()));
            // Said again through the one place that decides it, so that what the row
            // asks for still meets the document's length and the surface being on
            // screen rather than overriding them.
            editor.mark_misspellings();
        });

        // The highlight follows the reader's colour scheme, because a text tag carries
        // colours rather than a style class and nothing else would restyle it. Live,
        // and without the buffer being touched: the same rule the rendered document is
        // held to (invariant 9).
        let recolouring = Rc::downgrade(&editor);
        *editor.recolouring.borrow_mut() =
            Some(adw::StyleManager::default().connect_dark_notify(move |_| {
                if let Some(editor) = recolouring.upgrade() {
                    editor.repaint();
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
            // And whether this document is one whose spelling is worth checking is a
            // question about the document, so a new one — or one that has just grown
            // past the mark — is asked again. Two integers and a comparison, which is
            // what it may cost on the keystroke path.
            editor.mark_misspellings();
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

    /// Replaces the one character at byte offset `at` with `marker` — what pressing a
    /// task list item's box in the rendered document does to the source (issue #12).
    ///
    /// Through the buffer, and as one user action, so that it is one step of undo
    /// rather than two and reaches everything a keystroke reaches: the document is
    /// marked unsaved, autosave is started, and the page is re-rendered from the buffer
    /// that changed. Nothing here searches the text — the offset is the parser's, and
    /// it is the caller's job to have checked what is there (`window.rs`).
    ///
    /// The buffer counts in characters and the source in bytes, so the offset is
    /// converted rather than assumed: a document with one accented letter above the
    /// checkbox would otherwise have its boxes rewritten one place to the left.
    pub(crate) fn replace_marker(&self, at: usize, marker: char) {
        let text = self.text();
        let Some(before) = text.get(..at) else {
            return;
        };
        let character = before.chars().count() as i32;
        if character + 1 > self.buffer.char_count() {
            return;
        }
        let mut from = self.buffer.iter_at_offset(character);
        let mut to = self.buffer.iter_at_offset(character + 1);
        self.buffer.begin_user_action();
        self.buffer.delete(&mut from, &mut to);
        self.buffer.insert(&mut from, &marker.to_string());
        self.buffer.end_user_action();
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

    /// How the source draws the first occurrence of `of`, in the words a reader would
    /// use for it: the colour of the letters, whether they are bold, whether they are
    /// slanted, and what is behind them.
    ///
    /// The editing half of reading a rendered block's computed style out of the page.
    /// An empty answer means the reader sees that text in the ordinary ink of the
    /// editor, which is what unhighlighted source looks like.
    pub(crate) fn styling(&self, of: &str) -> String {
        let Some(at) = self.first(of) else {
            return String::new();
        };
        let at = self.at(at);
        // The same call the view makes for the lines it is about to draw: highlighting
        // is worked out where it is needed and nowhere else, so asking about a place
        // has to ask for its line too — an empty range asks for nothing.
        let (from, to) = line_around(&at);
        self.buffer.ensure_highlight(&from, &to);
        drawing(&at)
    }

    /// Where `of` first occurs in the source, in characters, or `None` when it does not.
    fn first(&self, of: &str) -> Option<usize> {
        let text: Vec<char> = self.text().chars().collect();
        let needle: Vec<char> = of.chars().collect();
        if needle.is_empty() || needle.len() > text.len() {
            return None;
        }
        (0..=text.len() - needle.len())
            .find(|start| text[*start..*start + needle.len()] == needle[..])
    }

    /// Puts the caret in the middle of the first occurrence of `word`, as clicking on
    /// that word does, and answers whether the source has one.
    ///
    /// Only the caret moves: what the spelling adapter makes of the move is its own
    /// queued business, exactly as after a click.
    pub(crate) fn place_caret_on(&self, word: &str) -> bool {
        let Some(at) = self.first(word) else {
            return false;
        };
        self.buffer
            .place_cursor(&self.at(at + word.chars().count() / 2));
        true
    }

    /// The source view's context menu as the reader meets it: whether it is open, and
    /// every row they can press in it, as `item<TAB>label<TAB>detailed-action`.
    ///
    /// Read off the rows the popover draws rather than off its model, because the model
    /// says what *could* be there: libspelling's Add to Dictionary and Ignore stay in it
    /// with nothing to add, and GTK hides them (`hidden-when` `action-disabled`); a
    /// Paste with nothing to paste is in it too, drawn greyed. A row that is hidden,
    /// greyed, or on a submenu page the reader has not opened is not listed. The
    /// popover is built the first time the menu opens, so before that the answer is a
    /// closed menu with nothing in it.
    pub(crate) fn context_menu(&self) -> String {
        let popover = self.popover();
        let mut said = vec![format!(
            "open {}",
            popover.as_ref().is_some_and(|popover| popover.is_visible())
        )];
        if let Some(popover) = &popover {
            said.extend(pressable_rows(popover).iter().map(|(_, label, action)| {
                format!("item\t{}\t{action}", crate::window::as_it_is_drawn(label))
            }));
        }
        said.join("\n")
    }

    /// Presses the row labelled `label` in the open context menu, as clicking it does,
    /// and answers whether the menu had one the reader could press.
    pub(crate) fn press_in_context_menu(&self, label: &str) -> bool {
        let Some(popover) = self.popover().filter(|popover| popover.is_visible()) else {
            return false;
        };
        let Some((row, _, _)) = pressable_rows(&popover)
            .into_iter()
            .find(|(_, drawn, _)| crate::window::as_it_is_drawn(drawn) == label)
        else {
            return false;
        };
        // The row's own signal, which is what a pointer press emits: it closes the
        // menu and activates the row's action (`gtk_model_button_clicked`, GTK 4.20.4).
        row.emit_by_name::<()>("clicked", &[]);
        true
    }

    /// The popover GtkTextView parents to itself the first time its menu opens
    /// (`gtk_text_view_do_popup`, GTK 4.20.4), if it has opened.
    fn popover(&self) -> Option<gtk::PopoverMenu> {
        let mut child = self.view.first_child();
        while let Some(candidate) = child {
            if let Some(popover) = candidate.downcast_ref::<gtk::PopoverMenu>() {
                return Some(popover.clone());
            }
            child = candidate.next_sibling();
        }
        None
    }

    /// Loads what the source is drawn with, the first time there is anybody to draw it
    /// for.
    ///
    /// GtkSourceView reads its language definitions and its style schemes off disk the
    /// first time either is asked for, and doing that while a window is opening costs
    /// the reader 52 ms of a cold start that VISION states in milliseconds — measured
    /// on 2026-08-03: 492 ms with this in [`Editor::new`] against 440 ms without it.
    /// A reader opening a document to read it never pays for the editor's dressing;
    /// one who presses Ctrl+E pays for it once, in a window that is already up.
    ///
    /// It is a trade and not a free win, so both halves are written down. A buffer
    /// given its language when it already holds a document is analysed all at once, and
    /// on the perf harness's ten megabytes that shows up in the very thing this module
    /// exists to protect: a key press costs 22 ms in the seconds after the switch,
    /// against 7 ms when the language was set before the text arrived (medians, same
    /// day, 50 ms budget). The measurement went the other way on the two budgets that
    /// are about the reader rather than the machine — cold start 446 ms against 495 ms,
    /// on a 560 ms ceiling — and every launch pays that one while only an enormous
    /// document being edited pays the other.
    fn dress(&self) {
        if self.dressed.replace(true) {
            return;
        }
        // The stock definition, unmodified. A missing one means a GtkSourceView
        // installed without its language files, which is a source the reader can still
        // read and edit — plainly — rather than a reason not to open their document.
        self.buffer.set_language(
            sourceview5::LanguageManager::default()
                .language(MARKDOWN)
                .as_ref(),
        );
        self.repaint();
    }

    /// Whether the reader has asked for their spelling to be checked.
    ///
    /// What they asked for is honoured while they are editing and never while they are
    /// reading: a document on the page is not spell checked, and the words behind it
    /// are not checked either — checking a ten-megabyte buffer nobody is typing in
    /// would be work spent on marks nobody can see.
    ///
    /// A window whose reader has used the Check Spelling row of the editor's own menu
    /// has said what it does for itself, and the preference no longer reaches it (see
    /// [`Editor::new`]).
    pub(crate) fn check_spelling(&self, wanted: bool) {
        self.checking.set(wanted);
        self.mark_misspellings();
    }

    /// Applies that, for whichever of the four things that decide it has just changed:
    /// this window's own choice, the preference behind it, the surface being on screen,
    /// or the document.
    fn mark_misspellings(&self) {
        let checkable =
            self.view.is_mapped() && self.buffer.char_count() <= LARGEST_CHECKED_DOCUMENT;
        let marking = self.switched.get().unwrap_or(self.checking.get()) && checkable;
        // Recorded before it is asked for, because asking emits the adapter's own
        // notification before this call returns, and the handler reading it must see
        // what this editor is in the middle of doing rather than what it did before.
        self.marking.set(marking);
        self.spelling.set_enabled(marking);
    }

    /// The words the reader can see underlined as misspelled, in the order they are
    /// written.
    ///
    /// Empty whenever nothing is being checked — which is every moment the reader has
    /// spell checking switched off, every moment they are reading rather than editing,
    /// and every document too big to check (see [`LARGEST_CHECKED_DOCUMENT`]).
    pub(crate) fn misspelled(&self) -> Vec<String> {
        let Some(marked) = self.spelling.tag() else {
            return Vec::new();
        };
        let mut words = Vec::new();
        let mut at = self.buffer.start_iter();
        if !at.starts_tag(Some(&marked)) && !at.forward_to_tag_toggle(Some(&marked)) {
            return words;
        }
        loop {
            let from = at;
            if !at.forward_to_tag_toggle(Some(&marked)) {
                break;
            }
            words.push(self.buffer.text(&from, &at, true).to_string());
            if !at.forward_to_tag_toggle(Some(&marked)) {
                break;
            }
        }
        words
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

    /// Draws the source in the reader's colour scheme: the palette the markup is
    /// highlighted in, and the two colours a search marks it with.
    ///
    /// A repaint and nothing else — no byte of the buffer is read or written, so
    /// changing scheme costs neither a parse nor a reload (invariant 9). The match
    /// colours are the two the rendered page uses (`axiomd.css`), so a match looks like
    /// a match whichever surface the reader is on.
    ///
    /// The scheme is only swapped on an editor that has been dressed: reading the
    /// schemes off disk is what [`Editor::dress`] defers, and a reader who changes
    /// palette without ever having edited anything must not pay for it either.
    fn repaint(&self) {
        let dark = adw::StyleManager::default().is_dark();
        let scheme = if dark { DARK } else { LIGHT };
        if self.dressed.get() {
            self.buffer.set_style_scheme(
                sourceview5::StyleSchemeManager::default()
                    .scheme(scheme)
                    .as_ref(),
            );
        }
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

/// Makes the corrections in the context menu the ones for the word the menu opens on.
///
/// libspelling learns the word under the caret on a low-priority timeout a tenth of a
/// second after the caret moves (`spelling_text_buffer_adapter_cursor_moved`, 0.4.9),
/// so a menu opened in the same moment as the move would offer the previous word's
/// corrections. `update_corrections` is libspelling's own answer — "force an update
/// immediately rather than after the automatic timeout" — and it is called on both
/// ways the menu opens, before GtkTextView builds it:
///
/// * a right click, from a capture-phase gesture that runs before the view's own. It
///   first puts the caret where the click is, unless the click is inside a selection,
///   because GtkTextView opens its menu without moving the caret
///   (`gtk_text_view_click_gesture_pressed`, GTK 4.20.4). This is what libspelling's
///   test program (`test/test-spelling.c`) and GNOME Text Editor 49
///   (`on_click_pressed_cb`, `editor-source-view.c`) do;
/// * Shift+F10 or the Menu key, from a capture-phase shortcut on those keys that does
///   not consume them, so the view's own binding to `menu.popup` still opens the menu.
///   Text Editor does the same from a key controller, because GtkTextView gives no
///   callback before `menu.popup` builds its menu (its issue #693).
///
/// Neither moves anything the reader can see except the caret a right click lands.
/// While checking is off `update_corrections` does nothing, and the adapter has
/// already emptied the corrections and disabled Add and Ignore.
fn corrections_follow_the_popup(
    view: &sourceview5::View,
    spelling: &libspelling::TextBufferAdapter,
) {
    let clicked = gtk::GestureClick::builder()
        .button(gtk::gdk::BUTTON_SECONDARY)
        .propagation_phase(gtk::PropagationPhase::Capture)
        .build();
    let adapter = spelling.downgrade();
    clicked.connect_pressed(move |gesture, presses, x, y| {
        let Some(adapter) = adapter.upgrade() else {
            return;
        };
        if presses != 1 {
            return;
        }
        let Some(view) = gesture.widget().and_downcast::<gtk::TextView>() else {
            return;
        };
        let buffer = view.buffer();
        let (at_x, at_y) =
            view.window_to_buffer_coords(gtk::TextWindowType::Widget, x as i32, y as i32);
        if let Some(clicked) = view.iter_at_location(at_x, at_y) {
            let inside_the_selection = buffer
                .selection_bounds()
                .is_some_and(|(from, to)| clicked.in_range(&from, &to) || clicked == to);
            if !inside_the_selection {
                buffer.place_cursor(&clicked);
            }
        }
        adapter.update_corrections();
    });
    view.add_controller(clicked);

    let keyed = gtk::ShortcutController::new();
    keyed.set_propagation_phase(gtk::PropagationPhase::Capture);
    let adapter = spelling.downgrade();
    keyed.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string(CONTEXT_MENU_KEYS),
        Some(gtk::CallbackAction::new(move |_, _| {
            if let Some(adapter) = adapter.upgrade() {
                adapter.update_corrections();
            }
            // Not consumed: the key goes on to GtkTextView's own `menu.popup`.
            glib::Propagation::Proceed
        })),
    ));
    view.add_controller(keyed);
}

/// The keys GtkTextView opens its context menu on (`gtk_text_view_class_init`, GTK
/// 4.20.4), spelled the way GTK spells a trigger.
const CONTEXT_MENU_KEYS: &str = "<Shift>F10|Menu";

/// Every row in `popover` the reader can press — drawn, not greyed, and on the page of
/// the menu that is showing — with its label as the model spells it and the detailed
/// action pressing it fires (`spelling.correct::sentence`; empty for a submenu).
///
/// The words come from the rows and the action from the popover's model, matched in
/// order by label: a row is GTK's private `GtkModelButton`, which says what it is
/// called in `text` but is activated through GTK's menu tracker rather than an action
/// of its own, so it cannot say what it fires (probed on GTK 4.20.4: its
/// `GtkActionable` name is unset).
fn pressable_rows(popover: &gtk::PopoverMenu) -> Vec<(gtk::Widget, String, String)> {
    fn walk(widget: &gtk::Widget, rows: &mut Vec<gtk::Widget>) {
        if !widget.is_visible() || !widget.is_child_visible() {
            return;
        }
        if widget.is::<gtk::Actionable>() && widget.find_property("text").is_some() {
            if widget.is_sensitive() {
                rows.push(widget.clone());
            }
            return;
        }
        let mut child = widget.first_child();
        while let Some(candidate) = child {
            walk(&candidate, rows);
            child = candidate.next_sibling();
        }
    }
    let mut drawn = Vec::new();
    walk(popover.upcast_ref(), &mut drawn);
    let mut items = popover
        .menu_model()
        .map(|model| crate::window::menu_items(&model))
        .unwrap_or_default();
    drawn
        .into_iter()
        .filter_map(|row| {
            // Compared as they are drawn: a row's `text` has lost the mnemonic marks its
            // model item's label still carries — `Select All` against `Select _All`.
            let label = crate::window::as_it_is_drawn(&row.property::<Option<String>>("text")?);
            let at = items
                .iter()
                .position(|(named, _)| crate::window::as_it_is_drawn(named) == label)?;
            let (label, action) = items.remove(at);
            Some((row, label, action))
        })
        .collect()
}

/// What is legible on both of those, which is not the same colour on both.
fn ink(dark: bool) -> &'static str {
    if dark { "#ffffff" } else { "#241f31" }
}

/// From where a reader would call the letters bold, in the numbers Pango weighs them
/// in (`PANGO_WEIGHT_BOLD`).
const BOLD: i32 = 700;

/// The whole line `at` is on — the unit highlighting is worked out in.
fn line_around(at: &gtk::TextIter) -> (gtk::TextIter, gtk::TextIter) {
    let mut from = *at;
    from.set_line_offset(0);
    let mut to = *at;
    if !to.ends_line() {
        to.forward_to_line_end();
    }
    (from, to)
}

/// How the letters at `at` are drawn, as everything that has been said about them —
/// the tags over that place folded in the order they are painted, so what comes out is
/// what the reader is looking at rather than what some layer underneath asked for.
fn drawing(at: &gtk::TextIter) -> String {
    let mut drawn: Vec<(&str, String)> = Vec::new();
    let mut set = |what: &'static str, how: String| {
        drawn.retain(|(named, _)| *named != what);
        drawn.push((what, how));
    };
    // In ascending priority, which is the order they are applied in: the last thing
    // said about a colour is the colour the reader sees.
    for tag in at.tags() {
        if tag.is_foreground_set() {
            set("colour", colour(tag.foreground_rgba()));
        }
        if tag.is_background_set() {
            set("behind", colour(tag.background_rgba()));
        }
        if tag.is_weight_set() && tag.weight() >= BOLD {
            set("weight", "bold".to_owned());
        }
        if tag.is_style_set() && tag.style() != gtk::pango::Style::Normal {
            set("slant", "italic".to_owned());
        }
        if tag.is_underline_set() && tag.underline() != gtk::pango::Underline::None {
            set("underline", "yes".to_owned());
        }
    }
    drawn
        .iter()
        .map(|(what, how)| format!("{what}={how}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A colour as it would be written down, so two runs of the same test compare strings
/// rather than floating-point channels.
fn colour(rgba: Option<gtk::gdk::RGBA>) -> String {
    let Some(rgba) = rgba else {
        return "none".to_owned();
    };
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        channel(rgba.red()),
        channel(rgba.green()),
        channel(rgba.blue())
    )
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
