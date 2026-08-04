//! The document's headings, beside the document (UT-005).
//!
//! One module owns the whole sidebar: the split view the document sits in, the list of
//! headings, the empty state for a document that has none, the action `F9` and the
//! header-bar button share, and what a window too narrow to hold both means for it,
//! which is the whole of what stops such a window from being all sidebar. What the
//! window has to know is four calls — here is the document's outline, the reader is
//! here, tell me when they pick a section, show or hide it.
//!
//! # Nothing here re-reads the document
//!
//! The entries are [`axiomd_render::Rendered::outline`], which is the part of the
//! anchor map that happens to be a heading. So an entry names a source line, the block
//! on screen carrying that `data-line` is the section, and the two cannot drift: the
//! same map already carries scroll sync, search and live-reload position preservation
//! (invariant 3). Nothing is parsed, scanned or measured to build this.
//!
//! # Where the reader is
//!
//! [`Outline::follow`] is given a source line and highlights the section it falls in —
//! the last heading at or before it, and none at all above the first one. It is told
//! that line by whichever surface the reader is on: the page, over the message bridge
//! and only on a frame in which the answer can have changed (`view.rs`, `track.js`), or
//! the editor's caret. Nothing here polls, measures a height, or reads the file.
//!
//! # How wide it is
//!
//! The reader says, by dragging the divider (issue #27), and it is remembered as
//! window state rather than as a preference — there is no row to hunt for, because the
//! thing itself is the control (`ux_decisions.md`). The width the divider is let go at
//! is written once, on release, and the next window built reads it.
//!
//! Neither pane can be crushed. The split view is told three numbers rather than one:
//! the chosen width as its maximum, the fraction that width is of a window just wide
//! enough to hold it *and* a document, and the floor below which the outline never
//! shrinks. Documented behaviour is that the sidebar is drawn at
//! `clamp(fraction × total, min, max)`, and that a collapsed one uses `max` when it can
//! (libadwaita 1.8.6, `/usr/share/doc/libadwaita-1/class.OverlaySplitView.html`). So a
//! window with room honours the chosen width exactly, and a narrower one shrinks the
//! two panes together instead of leaving the document a sliver.
//!
//! The one thing no test here drives is a real pointer: a headless compositor has none
//! and GTK 4 offers no way to inject one. So a drag arrives as the divider's own
//! gesture signals ([`Outline::drag`], [`Outline::restore`]) — the very signals a
//! press, a move and a release emit — exactly as picking a section emits the list's
//! own `activate`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use adw::subclass::prelude::*;
use axiomd_render::Heading;
use gtk::gio;
use gtk::glib;

use crate::settings::Settings;

/// The narrowest and widest the reader may drag the outline, in pixels: narrower and a
/// heading is unreadable, wider and it is a pane rather than an index.
///
/// The schema's own range for `sidebar-width` is these two numbers, and
/// `the_widths_the_divider_allows_are_the_ones_the_schema_does` holds the two together
/// — a bound the schema refuses is a drag that writes nothing.
pub(crate) const BOUNDS: (i32, i32) = (180, 480);

/// The least room the document keeps beside the outline, in pixels. The reader cannot
/// drag past it, and a window too narrow to give it shrinks both panes rather than
/// starving one.
const ROOM_FOR_THE_DOCUMENT: i32 = 480;

/// How wide the grip on the divider is — the part of the edge that answers a pointer,
/// as `GtkPaned`'s own handle does.
const GRIP: i32 = 5;

/// How far one heading level is indented under the one above it.
const NESTING: i32 = 12;

/// Which of the sidebar's two faces is showing.
const HEADINGS: &str = "headings";
const NOTHING: &str = "nothing";

/// What the sidebar says instead of a list, for a document that has no sections.
const NO_HEADINGS: &str = "No headings";

/// One window's outline sidebar.
pub(crate) struct Outline {
    split: adw::OverlaySplitView,
    entries: gio::ListStore,
    selection: gtk::SingleSelection,
    list: gtk::ListView,
    faces: gtk::Stack,
    /// The sidebar the split view holds: the headings, and the grip on their edge.
    sidebar: gtk::Box,
    /// The two gestures the grip answers — held so a test can emit what a pointer
    /// emits, there being no pointer to move.
    divider: gtk::GestureDrag,
    restorer: gtk::GestureClick,
    settings: Rc<Settings>,
    /// The width the reader has chosen, in pixels — what is drawn when the window has
    /// room for it, and what is written down when they let go.
    width: Cell<i32>,
    /// How wide the outline was drawn when the current drag took hold of it, so that a
    /// drag moves the divider from where the reader is looking at it rather than from
    /// a remembered width a narrow window is not honouring.
    grabbed: Cell<i32>,
    /// The source line the reader was last known to be on, so that a document which
    /// has just been re-rendered under them highlights the same section rather than
    /// none (live reload, and every keystroke in edit mode).
    at: Cell<u32>,
    /// Whether the sidebar is out of the way for want of anything to put in it — a
    /// window opened with no document (issue #32). Set while that is true and cleared
    /// the moment it stops being: a heading arrives, or the reader asks for the sidebar
    /// themselves.
    held: Cell<bool>,
    chosen: RefCell<Option<Chosen>>,
}

/// What the window does when the reader picks a section.
type Chosen = Rc<dyn Fn(u32)>;

impl Outline {
    /// Builds the outline beside `content`, and gives `window` the action `action` —
    /// which is the header-bar toggle, `F9`, and the sidebar's own visibility, all the
    /// same boolean.
    ///
    /// The returned widget is what the window shows: the sidebar and `content` in one.
    pub(crate) fn new(
        window: &adw::ApplicationWindow,
        content: &impl IsA<gtk::Widget>,
        action: &str,
        settings: &Rc<Settings>,
        narrow: &adw::Breakpoint,
    ) -> Rc<Self> {
        let entries = gio::ListStore::new::<Entry>();
        // Nothing is selected until the reader is somewhere: a document open at its
        // first paragraph is in no section yet, and a highlight would be a lie.
        let selection = gtk::SingleSelection::builder()
            .model(&entries)
            .autoselect(false)
            .can_unselect(true)
            .build();
        selection.set_selected(gtk::INVALID_LIST_POSITION);

        let list = gtk::ListView::builder()
            .model(&selection)
            // A heading is a place, not a file: one click goes there, as it does in
            // every outline the reader has used.
            .single_click_activate(true)
            .factory(&factory())
            .build();
        list.add_css_class("navigation-sidebar");

        let scroller = gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();

        // Never a dialog and never a blank panel: a document with no headings says so
        // where the headings would be (invariant 12).
        let nothing = adw::StatusPage::builder()
            .icon_name("view-list-symbolic")
            .title(NO_HEADINGS)
            .description("This document has no headings to navigate by.")
            .build();
        nothing.add_css_class("compact");

        let faces = gtk::Stack::new();
        faces.add_named(&scroller, Some(HEADINGS));
        faces.add_named(&nothing, Some(NOTHING));
        faces.set_visible_child_name(NOTHING);
        faces.set_hexpand(true);

        // The divider the reader drags, on the outline's inner edge, where the split
        // view already draws the line between the two panes. It is the pointer's
        // target and nothing else: no ink of its own, and the resize cursor that says
        // what it is — the affordance `GtkPaned` gives its own handle.
        let grip = gtk::Box::builder()
            .width_request(GRIP)
            .tooltip_text("Drag to resize the outline, double-click to reset it")
            .build();
        grip.set_cursor_from_name(Some("col-resize"));

        let sidebar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        sidebar.append(&faces);
        sidebar.append(&grip);

        let split = adw::OverlaySplitView::builder()
            .sidebar(&sidebar)
            .content(content)
            .show_sidebar(true)
            // Pixels, because a width the reader dragged to is a number of pixels.
            .sidebar_width_unit(adw::LengthUnit::Px)
            .min_sidebar_width(f64::from(BOUNDS.0))
            .build();

        // A divider over a document the outline is overlaying resizes nothing — the
        // panes are not sharing the width to begin with — so it is not there to drag.
        split
            .bind_property("collapsed", &grip, "visible")
            .invert_boolean()
            .sync_create()
            .build();

        // A window too narrow to hold both stops holding both: the outline overlays
        // the document rather than squeezing it, and starts out of the way. Both
        // settings are undone when the window is given room again. What counts as too
        // narrow is the window's to say (`window.rs`), because the header bar answers
        // the same condition.
        narrow.add_setter(&split, "collapsed", Some(&true.to_value()));
        narrow.add_setter(&split, "show-sidebar", Some(&false.to_value()));

        // The sidebar's own visibility *is* the action's state, in both directions, so
        // the button can never say "open" while the sidebar is shut — including when
        // the breakpoint is what shut it.
        window.add_action(&gio::PropertyAction::new(
            action.strip_prefix("win.").unwrap_or(action),
            &split,
            "show-sidebar",
        ));

        let outline = Rc::new(Self {
            split,
            entries,
            selection,
            list,
            faces,
            sidebar,
            divider: gtk::GestureDrag::new(),
            restorer: gtk::GestureClick::new(),
            settings: settings.clone(),
            width: Cell::new(BOUNDS.0),
            grabbed: Cell::new(BOUNDS.0),
            at: Cell::new(0),
            held: Cell::new(false),
            chosen: RefCell::new(None),
        });

        // A sidebar the reader has in front of them is no longer being held back for
        // want of anything to put in it, however it got there — `F9`, the header-bar
        // toggle, or their preference. So a heading typed afterwards into a sidebar
        // they have since shut does not push it open again over them.
        let asked_for = Rc::downgrade(&outline);
        outline.split.connect_show_sidebar_notify(move |split| {
            if split.shows_sidebar()
                && let Some(outline) = asked_for.upgrade()
            {
                outline.held.set(false);
            }
        });

        // The width the reader left the divider at, before the window is ever drawn:
        // they never see the default one first.
        outline.widen_to(settings.sidebar_width());

        // A drag moves the divider from where it is drawn, so where it is drawn is
        // taken at the press. The release is the only moment anything is written down:
        // one write per drag, not one per frame of it.
        let grabbing = Rc::downgrade(&outline);
        outline.divider.connect_drag_begin(move |_, _, _| {
            if let Some(outline) = grabbing.upgrade() {
                outline.grabbed.set(outline.sidebar.width());
            }
        });
        let dragging = Rc::downgrade(&outline);
        outline.divider.connect_drag_update(move |_, across, _| {
            if let Some(outline) = dragging.upgrade() {
                outline.dragged(across);
            }
        });
        let released = Rc::downgrade(&outline);
        outline.divider.connect_drag_end(move |_, across, _| {
            if let Some(outline) = released.upgrade() {
                outline.dragged(across);
                outline.settings.remember_sidebar_width(outline.width.get());
            }
        });
        grip.add_controller(outline.divider.clone());

        // Double-click, the divider's way back: the reader who has dragged themselves
        // into a corner is never stuck with it (issue #27).
        let restoring = Rc::downgrade(&outline);
        outline.restorer.connect_released(move |_, presses, _, _| {
            if presses != 2 {
                return;
            }
            if let Some(outline) = restoring.upgrade() {
                outline.widen_to(outline.settings.forget_sidebar_width());
            }
        });
        grip.add_controller(outline.restorer.clone());

        let picked = Rc::downgrade(&outline);
        outline.list.connect_activate(move |_, position| {
            let Some(outline) = picked.upgrade() else {
                return;
            };
            let Some(line) = outline.line_at(position) else {
                return;
            };
            let handler = outline.chosen.borrow().clone();
            if let Some(handler) = handler {
                handler(line);
            }
        });

        outline
    }

    /// The sidebar and the document together — what the window puts on screen.
    pub(crate) fn widget(&self) -> &gtk::Widget {
        self.split.upcast_ref()
    }

    /// The sidebar on its own — the panel the reader sees the headings in, for the
    /// window to say where on screen it is.
    pub(crate) fn panel(&self) -> &gtk::Widget {
        self.sidebar.upcast_ref()
    }

    /// Drags the divider `across` pixels, exactly as the pointer does: the grip's own
    /// gesture, which is what a press, a move and a release emit.
    pub(crate) fn drag(&self, across: f64) {
        self.divider
            .emit_by_name::<()>("drag-begin", &[&0.0f64, &0.0f64]);
        self.divider
            .emit_by_name::<()>("drag-update", &[&across, &0.0f64]);
        self.divider
            .emit_by_name::<()>("drag-end", &[&across, &0.0f64]);
    }

    /// Double-clicks the divider, which is what puts the outline back to the width a
    /// reader who never touched it reads at.
    pub(crate) fn restore(&self) {
        self.restorer
            .emit_by_name::<()>("released", &[&2i32, &0.0f64, &0.0f64]);
    }

    /// Where a drag `across` pixels from where it took hold leaves the divider.
    ///
    /// Nothing while the outline is overlaying the document: the two panes are not
    /// sharing the window's width, so there is nothing between them to move.
    fn dragged(&self, across: f64) {
        if self.split.is_collapsed() {
            return;
        }
        let wanted = self.grabbed.get().saturating_add(across.round() as i32);
        self.widen_to(wanted.min(self.widest_here()));
    }

    /// Puts the outline at `width` pixels, within the bounds neither pane may cross.
    ///
    /// Three numbers rather than one, because one alone would crush a pane — see the
    /// module documentation for what libadwaita does with them.
    fn widen_to(&self, width: i32) {
        let width = width.clamp(BOUNDS.0, BOUNDS.1);
        self.width.set(width);
        self.split.set_max_sidebar_width(f64::from(width));
        self.split.set_sidebar_width_fraction(
            f64::from(width) / f64::from(width + ROOM_FOR_THE_DOCUMENT),
        );
    }

    /// The widest the divider goes in the window it is in now: never so wide that the
    /// document beside it is left less than a document's worth of room.
    fn widest_here(&self) -> i32 {
        (self.split.width() - ROOM_FOR_THE_DOCUMENT).clamp(BOUNDS.0, BOUNDS.1)
    }

    /// Calls `handler` with the source line of the heading the reader picked.
    pub(crate) fn connect_chosen(&self, handler: impl Fn(u32) + 'static) {
        *self.chosen.borrow_mut() = Some(Rc::new(handler));
    }

    /// Shows `headings` as the outline of the document now on screen.
    ///
    /// Called for every render, including the ones nobody asked for — a file that
    /// changed under the reader, a keystroke in the editor — so the highlight survives
    /// it: the section the reader was in stays the section they are in, found again by
    /// what it is called rather than by where it was. A heading inserted above them
    /// moves every source line in the document and none of this.
    ///
    /// The page says where they are afterwards in any case; this is what keeps the
    /// sidebar from flickering through a wrong answer while it does.
    pub(crate) fn show(&self, headings: &[Heading]) {
        let reading = self.reading();
        self.entries.remove_all();
        for heading in headings {
            self.entries.append(&Entry::of(heading));
        }
        self.faces.set_visible_child_name(if headings.is_empty() {
            NOTHING
        } else {
            HEADINGS
        });
        // The first heading in a window that opened with nothing in it: there is
        // something to navigate by now, so the sidebar is no longer held back.
        if !headings.is_empty() {
            self.let_go();
        }
        match reading.and_then(|reading| self.find_section(&reading)) {
            Some(position) => self.select(position),
            None => self.highlight(),
        }
    }

    /// Says where the reader is, as a source line, and highlights the section that
    /// line falls in.
    pub(crate) fn follow(&self, line: u32) {
        if self.at.replace(line) != line {
            self.highlight();
        }
    }

    /// Shows or hides the sidebar — the preference, and what `F9` toggles.
    pub(crate) fn reveal(&self, shown: bool) {
        self.split.set_show_sidebar(shown);
    }

    /// What the sidebar does when the window is given something to show — a file, or a
    /// new untitled document with nothing in it yet (issue #32).
    ///
    /// A window opened with no document has no headings to list and no prospect of any
    /// until the reader writes one, so a fifth of it standing empty under "No headings"
    /// is noise where the reader's first line should be. The sidebar stays out of the
    /// way, and comes back the moment there is a reason for it: the first heading they
    /// write, `F9`, or the file they open here instead.
    ///
    /// A document opened into a window that was never holding the sidebar back leaves
    /// it exactly as the reader left it — following a link with the sidebar shut does
    /// not reopen it — and a heading-less document that was *opened* still says "No
    /// headings" where its headings would be, which is the empty state that is right
    /// for it.
    pub(crate) fn opened(&self, a_document: bool) {
        match a_document {
            true => self.let_go(),
            false => {
                self.held.set(true);
                self.reveal(false);
            }
        }
    }

    /// Stops holding the sidebar back, and puts it where the reader's preference says
    /// — but only for a sidebar that *was* being held back, so this can never move one
    /// the reader has put where they want it.
    fn let_go(&self) {
        if self.held.replace(false) {
            self.reveal(self.settings.outline_shown());
        }
    }

    /// Whether the sidebar is beside the document.
    pub(crate) fn is_revealed(&self) -> bool {
        self.split.shows_sidebar()
    }

    /// The sections listed, in order, each as its level and its words — what a reader
    /// sees as an indented row.
    pub(crate) fn listed(&self) -> Vec<String> {
        (0..self.entries.n_items())
            .filter_map(|position| self.entry_at(position))
            .map(|entry| format!("h{} {}", entry.level(), entry.text()))
            .collect()
    }

    /// What the sidebar says instead of a list, or an empty string while it is showing
    /// one.
    pub(crate) fn notice(&self) -> String {
        match self.faces.visible_child_name().as_deref() {
            Some(NOTHING) => NO_HEADINGS.to_owned(),
            _ => String::new(),
        }
    }

    /// Picks the section called `section`, exactly as clicking its row does: the list's
    /// own activation, which is what a single click on a row emits.
    ///
    /// Answers whether there was such a section to pick.
    pub(crate) fn pick(&self, section: &str) -> bool {
        let Some(position) = (0..self.entries.n_items()).find(|position| {
            self.entry_at(*position)
                .is_some_and(|entry| entry.text() == section)
        }) else {
            return false;
        };
        self.list.emit_by_name::<()>("activate", &[&position]);
        true
    }

    /// The section the reader is in, as the sidebar shows it highlighted, or an empty
    /// string when no section is.
    pub(crate) fn current(&self) -> String {
        self.entry_at(self.selection.selected())
            .map(|entry| entry.text())
            .unwrap_or_default()
    }

    /// Puts the highlight on the section the reader's line falls in: the last heading
    /// at or before it, and none at all while they are still above the first one —
    /// which is where a document that opens with a paragraph starts.
    fn highlight(&self) {
        let line = self.at.get();
        let wanted = (0..self.entries.n_items())
            .take_while(|position| self.line_at(*position).is_some_and(|start| start <= line))
            .last()
            .unwrap_or(gtk::INVALID_LIST_POSITION);
        self.select(wanted);
    }

    fn select(&self, position: u32) {
        if self.selection.selected() == position {
            return;
        }
        self.selection.set_selected(position);
        // A highlight the reader cannot see is not a highlight: a long outline
        // scrolls to keep the current section in the sidebar.
        if position != gtk::INVALID_LIST_POSITION {
            self.list
                .scroll_to(position, gtk::ListScrollFlags::NONE, None);
        }
    }

    /// The section being read, as what it is rather than where it is: its level, its
    /// words, and — because two sections may be called the same thing — which of the
    /// sections called that it is.
    fn reading(&self) -> Option<(u8, String, usize)> {
        let position = self.selection.selected();
        let entry = self.entry_at(position)?;
        let earlier = (0..position)
            .filter_map(|before| self.entry_at(before))
            .filter(|other| other.level() == entry.level() && other.text() == entry.text())
            .count();
        Some((entry.level(), entry.text(), earlier))
    }

    /// Where that section is in the document now, if the document still has it.
    fn find_section(&self, (level, text, ordinal): &(u8, String, usize)) -> Option<u32> {
        (0..self.entries.n_items())
            .filter(|position| {
                self.entry_at(*position)
                    .is_some_and(|entry| entry.level() == *level && entry.text() == *text)
            })
            .nth(*ordinal)
    }

    fn entry_at(&self, position: u32) -> Option<Entry> {
        self.entries.item(position).and_downcast::<Entry>()
    }

    fn line_at(&self, position: u32) -> Option<u32> {
        self.entry_at(position).map(|entry| entry.line())
    }
}

/// How one heading is drawn: its words, indented under the level above it.
fn factory() -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let label = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        item.set_child(Some(&label));
    });
    factory.connect_bind(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let (Some(label), Some(entry)) = (
            item.child().and_downcast::<gtk::Label>(),
            item.item().and_downcast::<Entry>(),
        ) else {
            return;
        };
        label.set_label(&entry.text());
        // The whole heading, for one too long to fit the sidebar.
        label.set_tooltip_text(Some(&entry.text()));
        label.set_margin_start(NESTING * i32::from(entry.level().saturating_sub(1)));
    });
    factory
}

glib::wrapper! {
    /// One heading in the list model. `pub` because a `GObject` subclass names its
    /// own public type; the module it lives in is private, so nothing outside this
    /// file can reach it.
    pub struct Entry(ObjectSubclass<imp::Entry>);
}

impl Entry {
    fn of(heading: &Heading) -> Entry {
        let entry: Entry = glib::Object::new();
        entry.imp().level.set(heading.level);
        entry.imp().line.set(heading.line);
        *entry.imp().text.borrow_mut() = heading.text.clone();
        entry
    }

    fn text(&self) -> String {
        self.imp().text.borrow().clone()
    }

    fn level(&self) -> u8 {
        self.imp().level.get()
    }

    fn line(&self) -> u32 {
        self.imp().line.get()
    }
}

mod imp {
    use std::cell::{Cell, RefCell};

    use gtk::glib;
    use gtk::subclass::prelude::*;

    /// A heading as a `GObject`, because a `GtkListView` takes a `GListModel` and
    /// nothing else. It carries no properties: nothing binds to it, the list is
    /// rebuilt whole on every render, and a property is a promise of change
    /// notification this has no use for.
    #[derive(Default)]
    pub struct Entry {
        pub level: Cell<u8>,
        pub line: Cell<u32>,
        pub text: RefCell<String>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Entry {
        const NAME: &'static str = "AxiomdOutlineEntry";
        type Type = super::Entry;
    }

    impl ObjectImpl for Entry {}
}
