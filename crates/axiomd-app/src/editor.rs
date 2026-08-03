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
//! # What is not here yet
//!
//! Syntax highlighting and spell checking. Both need GtkSourceView 5 and libspelling,
//! whose development packages are not installable in this environment; the rest of the
//! editor — the buffer, undo, the caret, the modified state — is the same either way,
//! and this module is the only place that changes when they arrive.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;

/// One window's editing surface.
pub(crate) struct Editor {
    scroller: gtk::ScrolledWindow,
    view: gtk::TextView,
    buffer: gtk::TextBuffer,
    changed: RefCell<Option<Rc<dyn Fn()>>>,
    /// Set while the *application* is putting text in the buffer — opening a document,
    /// following the file, taking the version on disk. Those are not the reader typing
    /// and must not mark the document modified or start an autosave.
    filling: Rc<Cell<bool>>,
}

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

        let editor = Rc::new(Self {
            scroller,
            view,
            buffer,
            changed: RefCell::new(None),
            filling: Rc::new(Cell::new(false)),
        });

        let typed = Rc::downgrade(&editor);
        editor.buffer.connect_changed(move |_| {
            let Some(editor) = typed.upgrade() else {
                return;
            };
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
}

/// Where the line the reader was reading lands in the editor: a little below the top,
/// so the lines above it are still there to be seen.
const EDITING_LINE_FROM_THE_TOP: f64 = 0.15;
