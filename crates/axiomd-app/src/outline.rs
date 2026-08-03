//! The document's headings, beside the document (UT-005).
//!
//! One module owns the whole sidebar: the split view the document sits in, the list of
//! headings, the empty state for a document that has none, the action `F9` and the
//! header-bar button share, and the breakpoint that stops a narrow window from being
//! all sidebar. What the window has to know is four calls — here is the document's
//! outline, the reader is here, tell me when they pick a section, show or hide it.
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

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use adw::subclass::prelude::*;
use axiomd_render::Heading;
use gtk::gio;
use gtk::glib;

/// How narrow a window has to get before the outline stops sitting beside the document
/// and starts overlaying it — the point at which a 900px window's document would be
/// squeezed into less than a comfortable measure.
const TOO_NARROW: f64 = 600.0;

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
    /// The source line the reader was last known to be on, so that a document which
    /// has just been re-rendered under them highlights the same section rather than
    /// none (live reload, and every keystroke in edit mode).
    at: Cell<u32>,
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

        let split = adw::OverlaySplitView::builder()
            .sidebar(&faces)
            .content(content)
            .show_sidebar(true)
            .min_sidebar_width(180.0)
            .max_sidebar_width(320.0)
            .sidebar_width_fraction(0.22)
            .build();

        // A window too narrow to hold both stops holding both: the outline overlays
        // the document rather than squeezing it, and starts out of the way. Both
        // settings are undone when the window is given room again.
        let breakpoint = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
            adw::BreakpointConditionLengthType::MaxWidth,
            TOO_NARROW,
            adw::LengthUnit::Px,
        ));
        breakpoint.add_setter(&split, "collapsed", Some(&true.to_value()));
        breakpoint.add_setter(&split, "show-sidebar", Some(&false.to_value()));
        window.add_breakpoint(breakpoint);

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
            at: Cell::new(0),
            chosen: RefCell::new(None),
        });

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
