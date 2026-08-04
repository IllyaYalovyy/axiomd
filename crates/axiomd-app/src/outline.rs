//! The document's headings, beside the document (UT-005).
//!
//! One module owns the whole sidebar: the split view the document sits in, the panel of
//! headings, the empty state for a document that has none, the action `F9` and the
//! header-bar button share, and what a window too narrow to hold both means for it,
//! which is the whole of what stops such a window from being all sidebar. What the
//! window has to know is five calls — here is the document and its outline, the reader
//! is here, tell me when they pick a section, show or hide it.
//!
//! # Nothing here re-reads the document
//!
//! The entries are [`axiomd_render::Rendered::outline`], which is the part of the
//! anchor map that happens to be a heading. So an entry names a source line, the block
//! on screen carrying that `data-line` is the section, and the two cannot drift: the
//! same map already carries scroll sync, search and live-reload position preservation
//! (invariant 3). Nothing is parsed, scanned or measured to build this.
//!
//! # What the reader sees
//!
//! A panel rather than a list (issue #35): a title row saying which document these are
//! the sections of and how many there are, and under it the sections themselves — the
//! level a heading is written at said by its weight, its size and its indentation, with
//! no `#` marks and no markup. A section with sections under it carries a chevron and
//! folds away; a section with none carries the space where the chevron would be, so the
//! words all start on the same line. Everything about how that is drawn is in
//! `outline.css` and every colour in it is a libadwaita variable, which is what makes
//! light, dark and high contrast three designs rather than one.
//!
//! The tree is a [`gtk::TreeListModel`] over the flat outline, so what the list draws is
//! the rows the reader can see and nothing else: folding a section takes its rows out of
//! the model, and the [`gtk::ListView`] under it still builds widgets only for the rows
//! on screen. A thousand-section document is a thousand small objects and a screenful of
//! widgets.
//!
//! # Where the reader is
//!
//! [`Outline::follow`] is given a source line and highlights the section it falls in —
//! the last row at or before it, and none at all above the first one. Because the rows
//! are in document order, that is also the answer for a reader who has folded away the
//! section they are in: the last *visible* row at or before them is the section that
//! holds it, so their place is always drawn on a row they can see. It is told that line
//! by whichever surface the reader is on: the page, over the message bridge and only on
//! a frame in which the answer can have changed (`view.rs`, `track.js`), or the editor's
//! caret. Nothing here polls, measures a height, or reads the file.
//!
//! # What survives a rebuild
//!
//! Every render rebuilds the panel, including the renders nobody asked for — a file that
//! changed under the reader, a keystroke in the editor. Two things survive that: the
//! section they were reading, and the sections they had folded away. Both are found
//! again by what they are called rather than by where they were — a heading inserted
//! above them moves every source line in the document and neither of these.
//!
//! The folds are this module's own record rather than the list's, because the list
//! forgets: `GtkTreeListModel` throws away the rows under a section that is folded and
//! builds them folded again when it is opened. So [`Outline::folds_changed`] keeps what
//! the reader meant, and [`Outline::unfold`] puts the rows back in agreement with it —
//! which is what makes opening a section give back exactly the tree that was under it,
//! and what carries a fold through a live reload. The record belongs to the document it
//! was made in and is dropped when another one is opened here.
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
//! press, a move and a release emit — a chevron is turned by `listitem.toggle-expand`,
//! the action `GtkTreeExpander` itself puts on its gesture and on `Ctrl+Space`
//! (GTK 4.20, `Gtk-4.0.gir`), and picking a section emits the list's own `activate`.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use adw::prelude::*;
use adw::subclass::prelude::*;
use axiomd_render::Heading;
use gtk::gdk;
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

/// How the panel is drawn. Loaded once per display, over the theme.
const STYLE: &str = include_str!("outline.css");

/// Which of the sidebar's two faces is showing.
const HEADINGS: &str = "headings";
const NOTHING: &str = "nothing";

/// What the sidebar says instead of a list, for a document that has no sections.
const NO_HEADINGS: &str = "No headings";

/// One window's outline sidebar.
pub(crate) struct Outline {
    split: adw::OverlaySplitView,
    /// The top-level sections. Every other section hangs off one of these.
    roots: gio::ListStore,
    /// Those sections as the reader sees them: the rows on screen, in document order,
    /// with a folded section's own rows absent.
    tree: gtk::TreeListModel,
    selection: gtk::SingleSelection,
    list: gtk::ListView,
    faces: gtk::Stack,
    /// The sidebar the split view holds: the panel, and the grip on its edge.
    sidebar: gtk::Box,
    /// The title row: which document this is the outline of, and how many sections it
    /// has.
    name: gtk::Label,
    count: gtk::Label,
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
    /// Whether the panel is already putting itself in order. While it is, the rows
    /// coming and going are this module's own doing rather than the reader's, so they
    /// are not read as the reader having folded anything and the work is done once
    /// rather than once per row.
    settling: Cell<bool>,
    /// Whether the sidebar is out of the way for want of anything to put in it — a
    /// window opened with no document (issue #32). Set while that is true and cleared
    /// the moment it stops being: a heading arrives, or the reader asks for the sidebar
    /// themselves.
    held: Cell<bool>,
    /// The sections the reader has folded away, by what they are called — this
    /// document's, dropped when another is opened here. What the panel is put back into
    /// agreement with whenever its rows change, so a fold outlives both the reader
    /// opening the section above it and every render, including the ones a keystroke
    /// causes.
    folded: RefCell<HashSet<String>>,
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
        style_the_panel();

        let roots = gio::ListStore::new::<Entry>();
        // Not passthrough, because the rows the list draws have to be `GtkTreeListRow`s
        // for the expander to watch, and not autoexpand, because what is expanded is
        // this module's answer rather than GTK's: a section the reader folded away stays
        // folded when the document is rebuilt under it.
        let tree = gtk::TreeListModel::new(roots.clone(), false, false, |item| {
            item.downcast_ref::<Entry>()
                .and_then(Entry::offspring)
                .map(Cast::upcast)
        });
        // Nothing is selected until the reader is somewhere: a document open at its
        // first paragraph is in no section yet, and a highlight would be a lie.
        let selection = gtk::SingleSelection::builder()
            .model(&tree)
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
        list.add_css_class("axiomd-outline");

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
        faces.set_vexpand(true);

        // The title row, so the sidebar reads as a panel about a document rather than as
        // a list of words: what it is called, and how much of it there is.
        let name = gtk::Label::builder()
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["axiomd-outline-name"])
            .build();
        let count = gtk::Label::builder()
            .css_classes(["axiomd-outline-count"])
            .build();
        let header = gtk::Box::builder()
            .spacing(6)
            .css_classes(["axiomd-outline-header"])
            .build();
        header.append(&name);
        header.append(&count);

        let panel = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .hexpand(true)
            .css_classes(["axiomd-outline-panel"])
            .build();
        panel.append(&header);
        panel.append(&faces);

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
        sidebar.append(&panel);
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
            roots,
            tree,
            selection,
            list,
            faces,
            sidebar,
            name,
            count,
            divider: gtk::GestureDrag::new(),
            restorer: gtk::GestureClick::new(),
            settings: settings.clone(),
            width: Cell::new(BOUNDS.0),
            grabbed: Cell::new(BOUNDS.0),
            at: Cell::new(0),
            held: Cell::new(false),
            settling: Cell::new(false),
            folded: RefCell::new(HashSet::new()),
            chosen: RefCell::new(None),
        });

        // The reader folding a section away, or opening one: what they did arrives as
        // rows coming and going, whichever way they did it.
        let folding = Rc::downgrade(&outline);
        outline
            .tree
            .connect_items_changed(move |_, from, _, added| {
                if let Some(outline) = folding.upgrade() {
                    outline.folds_changed(from, added);
                }
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
    /// window to say where on screen it is and to capture what it looks like.
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

    /// Shows `headings` as the outline of the document called `name`.
    ///
    /// Called for every render, including the ones nobody asked for — a file that
    /// changed under the reader, a keystroke in the editor — so what the reader has
    /// done to the panel survives it: the section they were in stays the section they
    /// are in, and the sections they folded away stay folded, both found again by what
    /// they are called rather than by where they were.
    ///
    /// The page says where they are afterwards in any case; this is what keeps the
    /// sidebar from flickering through a wrong answer while it does.
    pub(crate) fn show(&self, name: &str, headings: &[Heading]) {
        let reading = self.reading();
        self.settling.set(true);
        self.rebuild(headings);
        self.unfold();
        self.settling.set(false);

        self.name.set_label(name);
        self.count.set_label(&counted(headings.len()));
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

    /// Builds the tree the panel draws: every heading under the nearest heading above it
    /// written at a shallower level, and the rest at the top.
    ///
    /// A document that skips a level — `#` then `###` — nests the way it reads: the
    /// deeper heading goes under the shallower one, whatever the distance between them.
    ///
    /// The whole document is built before a single row of it reaches the model, and that
    /// is not a nicety. `gtk_tree_list_row_is_expandable` asks this module for a
    /// section's children once and remembers the answer for the life of the row (probed
    /// on GTK 4.20.4: a row asked before its children exist is a leaf for ever after),
    /// and the list on screen asks the moment an item appears. A section appended before
    /// its own sections were hung off it would therefore lose its chevron — every
    /// document flat, and only in the running application, never in a model built beside
    /// a test. One splice at the end also means one change signal for a document rather
    /// than one per heading.
    fn rebuild(&self, headings: &[Heading]) {
        let mut roots: Vec<Entry> = Vec::new();
        let mut holders: Vec<(u8, Entry)> = Vec::new();
        let mut seen: HashMap<(u8, &str), usize> = HashMap::new();
        for heading in headings {
            // Two sections of a document may be called the same thing at the same level,
            // so which of them this is is part of what it is called.
            let ordinal = seen
                .entry((heading.level, heading.text.as_str()))
                .and_modify(|counted| *counted += 1)
                .or_insert(0);
            let entry = Entry::of(heading, *ordinal);
            while holders
                .last()
                .is_some_and(|(level, _)| *level >= heading.level)
            {
                holders.pop();
            }
            match holders.last() {
                Some((_, holder)) => holder.adopt(&entry),
                None => roots.push(entry.clone()),
            }
            holders.push((heading.level, entry));
        }
        self.roots.splice(0, self.roots.n_items(), &roots);
    }

    /// The rows have changed: `added` of them appeared at `from`. Works out what the
    /// reader meant by it and puts the panel back in agreement with them.
    ///
    /// This is the only place a fold is recorded, and it is read from the rows rather
    /// than from the chevron, because a chevron can be turned by the pointer, by
    /// `Ctrl+Space` and by a drag hovering over it — a change to the rows is the one
    /// thing all three produce.
    ///
    /// The rows that just appeared are not asked what the reader wants, because they
    /// were not there to be asked: `GtkTreeListModel` throws away a folded section's
    /// rows and builds them folded again when it is opened (probed on GTK 4.20.4), so
    /// reading them would take the reader's own tree apart every time they opened a
    /// section above it. They are told instead, by [`Outline::unfold`], which is what
    /// makes opening a section give back exactly what was under it.
    fn folds_changed(&self, from: u32, added: u32) {
        if self.settling.replace(true) {
            return;
        }
        {
            let mut folded = self.folded.borrow_mut();
            for position in (0..self.tree.n_items())
                .filter(|position| !(from..from.saturating_add(added)).contains(position))
            {
                let (Some(row), Some(key)) = (self.row_at(position), self.key_at(position)) else {
                    continue;
                };
                if !row.is_expandable() {
                    continue;
                }
                match row.is_expanded() {
                    true => folded.remove(&key),
                    false => folded.insert(key),
                };
            }
        }
        self.unfold();
        self.settling.set(false);
        // Which rows there are decides which one the reader's place is drawn on: a
        // section folded away hands their place to the section that holds it, and
        // opening it hands it back.
        self.highlight();
    }

    /// Opens every section the reader has not folded away.
    ///
    /// Walking forwards is what makes this one pass: opening a section puts its own rows
    /// in immediately after it, so the loop meets them next and asks the same question
    /// of them. A folded section's rows never appear at all, so a document folded down
    /// to its top level costs its top level and nothing more.
    fn unfold(&self) {
        let mut position = 0;
        while position < self.tree.n_items() {
            if let Some(row) = self.row_at(position)
                && row.is_expandable()
            {
                let wanted = self
                    .key_at(position)
                    .is_none_or(|key| !self.folded.borrow().contains(&key));
                row.set_expanded(wanted);
            }
            position += 1;
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
    ///
    /// The folds go with the document that was folded: a second file opened in this
    /// window is read whole, not with a section of it missing because the last document
    /// happened to have a section called the same thing.
    pub(crate) fn opened(&self, a_document: bool) {
        self.folded.borrow_mut().clear();
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

    /// The whole panel as the reader reads it, top to bottom: the title row, then one
    /// line per row on screen — its level, its chevron, whether it is drawn as the
    /// reader's place, and its words.
    ///
    /// One answer rather than four, because the four are one moment: a chevron turned
    /// down is a claim about the rows under it, and the reader's place is a claim about
    /// which of the rows on screen it is.
    pub(crate) fn shown(&self) -> String {
        let mut said = vec![format!(
            "title\t{}\t{}",
            self.name.label(),
            self.count.label(),
        )];
        let here = self.selection.selected();
        for position in 0..self.tree.n_items() {
            let (Some(row), Some(entry)) = (self.row_at(position), self.entry_at(position)) else {
                continue;
            };
            let chevron = match (row.is_expandable(), row.is_expanded()) {
                (false, _) => "leaf",
                (true, true) => "expanded",
                (true, false) => "collapsed",
            };
            let drawn_as_here = if position == here { "here" } else { "-" };
            said.push(format!(
                "row\t{}\t{chevron}\t{drawn_as_here}\t{}",
                entry.level(),
                entry.text(),
            ));
        }
        said.join("\n")
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
    /// Answers whether there was such a section on screen to pick — a section folded
    /// away is not one, for the same reason it is not one the pointer could reach.
    pub(crate) fn pick(&self, section: &str) -> bool {
        let Some(position) = self.position_of(section) else {
            return false;
        };
        self.list.emit_by_name::<()>("activate", &[&position]);
        true
    }

    /// Turns the chevron of the section called `section`, exactly as clicking it does.
    ///
    /// `listitem.toggle-expand` is `GtkTreeExpander`'s own action — what its gesture
    /// activates and what `Ctrl+Space` is bound to (GTK 4.20, documented on the class) —
    /// so this is the reader's path and not a way round it. Answers whether the sidebar
    /// is showing a section by that name with anything under it to fold.
    pub(crate) fn fold(&self, section: &str) -> bool {
        let Some(expander) = self.expander_of(section) else {
            return false;
        };
        WidgetExt::activate_action(&expander, "listitem.toggle-expand", None).is_ok()
    }

    /// The section the reader is in, as the sidebar shows it highlighted, or an empty
    /// string when no section is.
    pub(crate) fn current(&self) -> String {
        self.entry_at(self.selection.selected())
            .map(|entry| entry.text())
            .unwrap_or_default()
    }

    /// Puts the highlight on the section the reader's line falls in: the last row at or
    /// before it, and none at all while they are still above the first one — which is
    /// where a document that opens with a paragraph starts.
    ///
    /// The rows are the ones on screen, in document order, so a reader inside a section
    /// they have folded away is shown the section that holds it: their place is always
    /// on a row they can see.
    fn highlight(&self) {
        let line = self.at.get();
        let wanted = (0..self.tree.n_items())
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

    /// The section being read, as what it is called rather than where it is.
    fn reading(&self) -> Option<String> {
        self.key_at(self.selection.selected())
    }

    /// Where that section is on screen now, if the document still has it and nothing is
    /// hiding it.
    fn find_section(&self, key: &str) -> Option<u32> {
        (0..self.tree.n_items()).find(|position| self.key_at(*position).as_deref() == Some(key))
    }

    /// The first row on screen reading `section`.
    fn position_of(&self, section: &str) -> Option<u32> {
        (0..self.tree.n_items()).find(|position| {
            self.entry_at(*position)
                .is_some_and(|entry| entry.text() == section)
        })
    }

    /// The expander of the row reading `section`, from the widgets the list has built —
    /// which are the rows on screen, because that is where a chevron the reader could
    /// turn is.
    fn expander_of(&self, section: &str) -> Option<gtk::Widget> {
        fn search(widget: &gtk::Widget, section: &str) -> Option<gtk::Widget> {
            if let Some(expander) = widget.downcast_ref::<gtk::TreeExpander>()
                && expander
                    .child()
                    .and_downcast::<gtk::Label>()
                    .is_some_and(|label| label.label() == section)
            {
                return Some(expander.clone().upcast());
            }
            let mut child = widget.first_child();
            while let Some(candidate) = child {
                if let Some(found) = search(&candidate, section) {
                    return Some(found);
                }
                child = candidate.next_sibling();
            }
            None
        }
        search(self.list.upcast_ref(), section)
    }

    fn row_at(&self, position: u32) -> Option<gtk::TreeListRow> {
        self.tree.item(position).and_downcast::<gtk::TreeListRow>()
    }

    fn entry_at(&self, position: u32) -> Option<Entry> {
        self.row_at(position)?.item().and_downcast::<Entry>()
    }

    fn key_at(&self, position: u32) -> Option<String> {
        self.entry_at(position).map(|entry| entry.key())
    }

    fn line_at(&self, position: u32) -> Option<u32> {
        self.entry_at(position).map(|entry| entry.line())
    }
}

/// What the title row says beside the document's name, or nothing at all for a document
/// with no sections — the empty state under it already says so, and saying it twice is
/// noise.
fn counted(headings: usize) -> String {
    match headings {
        0 => String::new(),
        1 => "1 heading".to_owned(),
        many => format!("{many} headings"),
    }
}

/// Loads `outline.css` over the theme, once for the display every window is on.
///
/// A stylesheet is a specification rather than state, so one provider serves every
/// window and nothing is shared between them (invariant 7). A stylesheet GTK cannot read
/// is said out loud rather than silently ignored: it would otherwise be a panel that
/// looks almost right.
fn style_the_panel() {
    thread_local! {
        static LOADED: Cell<bool> = const { Cell::new(false) };
    }
    LOADED.with(|loaded| {
        if loaded.replace(true) {
            return;
        }
        let Some(display) = gdk::Display::default() else {
            return;
        };
        let provider = gtk::CssProvider::new();
        provider.connect_parsing_error(|_, section, error| {
            eprintln!("axiomd: outline.css {section}: {error}");
        });
        provider.load_from_string(STYLE);
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    });
}

/// How one heading is drawn: a chevron for a section with sections under it, the space
/// where one would be for a section without, and then its words at the weight and size
/// its level is written at.
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
        let expander = gtk::TreeExpander::builder()
            .child(&label)
            // The words of every row start on the same line, whether or not the row has
            // a chevron — an index whose entries do not line up is not an index.
            .indent_for_icon(true)
            .indent_for_depth(true)
            .build();
        item.set_child(Some(&expander));
        // GTK's own instruction for a list of expanders: the keyboard belongs to the
        // expander, or its shortcuts never reach it (GTK 4.20, `GtkTreeExpander`).
        item.set_focusable(false);
    });
    factory.connect_bind(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let (Some(expander), Some(row)) = (
            item.child().and_downcast::<gtk::TreeExpander>(),
            item.item().and_downcast::<gtk::TreeListRow>(),
        ) else {
            return;
        };
        let Some(entry) = row.item().and_downcast::<Entry>() else {
            return;
        };
        // A section with nothing under it keeps the chevron's room and loses the
        // chevron: there is nothing to fold, so there is nothing to press.
        expander.set_hide_expander(!row.is_expandable());
        expander.set_list_row(Some(&row));
        if let Some(label) = expander.child().and_downcast::<gtk::Label>() {
            label.set_label(&entry.text());
            // The whole heading, for one too long to fit the sidebar.
            label.set_tooltip_text(Some(&entry.text()));
            label.set_css_classes(&[&format!(
                "axiomd-outline-level-{}",
                entry.level().clamp(1, 6)
            )]);
        }
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
    /// One heading, and which of the headings called that it is.
    fn of(heading: &Heading, ordinal: usize) -> Entry {
        let entry: Entry = glib::Object::new();
        entry.imp().level.set(heading.level);
        entry.imp().line.set(heading.line);
        *entry.imp().text.borrow_mut() = heading.text.clone();
        // Unit separator: a character no heading holds, so two headings cannot spell
        // one another's name.
        *entry.imp().key.borrow_mut() =
            format!("{}\u{1f}{}\u{1f}{ordinal}", heading.level, heading.text);
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

    /// What this section is called, in the sense that survives the document being
    /// rebuilt under it: its level, its words, and which of the sections called that it
    /// is.
    fn key(&self) -> String {
        self.imp().key.borrow().clone()
    }

    /// Puts `child` under this section.
    fn adopt(&self, child: &Entry) {
        self.imp()
            .children
            .borrow_mut()
            .get_or_insert_with(gio::ListStore::new::<Entry>)
            .append(child);
    }

    /// The sections under this one, or `None` for a section with none — which is what
    /// makes its row a leaf and takes its chevron away.
    fn offspring(&self) -> Option<gio::ListStore> {
        self.imp().children.borrow().clone()
    }
}

mod imp {
    use std::cell::{Cell, RefCell};

    use gtk::gio;
    use gtk::glib;
    use gtk::subclass::prelude::*;

    /// A heading as a `GObject`, because a `GtkListView` takes a `GListModel` and
    /// nothing else. It carries no properties: nothing binds to it, the panel is
    /// rebuilt whole on every render, and a property is a promise of change
    /// notification this has no use for.
    ///
    /// The store of children is made only for a heading that has any, so a document of
    /// leaves costs no stores at all.
    #[derive(Default)]
    pub struct Entry {
        pub level: Cell<u8>,
        pub line: Cell<u32>,
        pub text: RefCell<String>,
        pub key: RefCell<String>,
        pub children: RefCell<Option<gio::ListStore>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Entry {
        const NAME: &'static str = "AxiomdOutlineEntry";
        type Type = super::Entry;
    }

    impl ObjectImpl for Entry {}
}
