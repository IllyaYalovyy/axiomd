//! The reader's search, and the one bar that runs it on whichever surface they are on
//! (UT-006).
//!
//! `Ctrl+F` opens it, `Enter` and `Ctrl+G` walk the matches, `Escape` closes it and
//! takes every mark back off the document. What the window has to know is five calls —
//! here is the bar, the reader has changed surfaces, close it, tell me when it closed,
//! and what does it say.
//!
//! # One search, two surfaces
//!
//! Search always works, in both modes (owner ruling, 2026-08-02): reading, it searches
//! the words on the page; editing, it searches the source in front of the reader. Which
//! of the two it is talking to is the whole of the difference, and neither of them
//! knows there is another — both answer the same two questions ([`Searchable`]) and
//! this module asks them. So the bar, the counter, the case toggle, the cycling and the
//! wrap are written once and the reader gets the same search either way. The totals are
//! honestly different: `[needle](https://example.com/needle)` is one word to read and
//! two to edit, and the counter says so.
//!
//! # Where the counting lives
//!
//! Here, and nowhere else. A surface is asked to mark a query and answers how many
//! matches it made; wrapping, the position in the cycle and what the counter reads are
//! arithmetic over that answer and belong nowhere near a DOM or a text buffer. That is
//! also what makes a mode switch cost nothing but a re-count: the reader's query
//! survives it because the query was never the surface's to hold.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use axiomd_i18n::{gettext, gettext_noop, pgettext};
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use crate::window::Mode;

/// The window actions the bar, the keyboard and the menu share. Named twice because a
/// widget addresses an action by its full name and a window registers it by its bare
/// one.
pub(crate) const FIND: &str = "win.find";
pub(crate) const FIND_NEXT: &str = "win.find-next";
pub(crate) const FIND_PREVIOUS: &str = "win.find-previous";
pub(crate) const FIND_CLOSE: &str = "win.find-close";

/// What the counter says when the document has none of what the reader asked for.
const NOTHING_FOUND: &str = gettext_noop("No results");

/// What the bar says when walking the matches has just taken the reader past an end of
/// the document and back round.
const WRAPPED_TO_THE_TOP: &str = gettext_noop("Wrapped to the top");
const WRAPPED_TO_THE_BOTTOM: &str = gettext_noop("Wrapped to the bottom");

/// What the reader is looking for.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub(crate) struct Query {
    pub(crate) text: String,
    /// Off by default: a reader who types `needle` means `Needle` too until they say
    /// otherwise.
    pub(crate) cased: bool,
}

impl Query {
    /// Where this query occurs in `haystack`, as `(start, end)` in characters, in
    /// order.
    ///
    /// Occurrences do not overlap — `aa` is in `aaa` once — which is what every find
    /// bar the reader has used does. Case is compared character by character rather
    /// than by lowercasing both sides, because the lowercase of a string is not always
    /// the same length as the string and an offset that drifts puts the highlight on
    /// the wrong letters. `find.js` matches the rendered page by the same rule, so the
    /// two surfaces cannot disagree about what a match is.
    pub(crate) fn matches(&self, haystack: &[char]) -> Vec<(usize, usize)> {
        let needle: Vec<char> = self.text.chars().collect();
        if needle.is_empty() || needle.len() > haystack.len() {
            return Vec::new();
        }
        let cased = self.cased;
        let same = |here: char, wanted: char| {
            here == wanted || (!cased && here.to_lowercase().eq(wanted.to_lowercase()))
        };

        let mut found = Vec::new();
        let mut at = 0;
        while at + needle.len() <= haystack.len() {
            if (0..needle.len()).all(|step| same(haystack[at + step], needle[step])) {
                found.push((at, at + needle.len()));
                at += needle.len();
            } else {
                at += 1;
            }
        }
        found
    }
}

/// What a surface does with the count it made: the query it counted, and how many
/// occurrences of it there were.
pub(crate) type Counted = Rc<dyn Fn(&Query, usize)>;

/// A surface the reader's search runs on.
///
/// Both of a window's surfaces are one of these, and neither knows the other exists.
pub(crate) trait Searchable {
    /// Marks every occurrence of `looking_for`, makes the `nth` one current — counting
    /// from zero in document order, wrapping — and answers how many there are by
    /// calling `counted` with the query it counted.
    ///
    /// `bring` is whether the reader asked to be taken to that match. They did when
    /// they typed it or pressed Next; they did not when the surface is merely catching
    /// up with them — a document that changed under them, or the mode switch they just
    /// made, which owns where they land (invariant 5).
    ///
    /// The answer may come on a later turn of the main loop: the rendered page is asked
    /// through the web process. It carries its query so that an answer about a search
    /// the reader has already moved on from can be told apart from an answer about the
    /// one they are running.
    fn show_matches(&self, looking_for: &Query, nth: usize, bring: bool, counted: Counted);

    /// Takes every trace of the search back off this surface.
    fn hide_matches(&self);
}

/// One window's search bar, and the one search it is running.
pub(crate) struct Find {
    bar: gtk::SearchBar,
    entry: gtk::SearchEntry,
    counter: gtk::Label,
    /// What the bar says when the reader has just been carried past an end of the
    /// document — the whole of the feedback a wrap gets, and it stays until they move
    /// again (Firefox's answer, and the only one that is there to be read rather than
    /// gone before it can be).
    wrap: gtk::Label,
    cased: gtk::ToggleButton,
    reading: Rc<dyn Searchable>,
    editing: Rc<dyn Searchable>,
    /// Which of the two the reader is on.
    mode: Cell<Mode>,
    /// The search the counter is counting, so that an answer about an older one is
    /// dropped rather than shown.
    running: RefCell<Query>,
    /// How many matches that search made, and which of them is current. The two halves
    /// of "n of N".
    total: Cell<usize>,
    at: Cell<usize>,
    closed: RefCell<Option<Rc<dyn Fn()>>>,
}

impl Find {
    /// Builds the bar and gives `window` the four actions it, the keyboard and the menu
    /// share.
    ///
    /// The returned widget goes at the top of the document pane; the bar shows and
    /// hides itself.
    pub(crate) fn new(
        window: &adw::ApplicationWindow,
        reading: Rc<dyn Searchable>,
        editing: Rc<dyn Searchable>,
    ) -> Rc<Self> {
        let entry = gtk::SearchEntry::builder()
            .placeholder_text(gettext("Search this document"))
            .hexpand(true)
            .build();

        let counter = gtk::Label::builder().xalign(1.0).build();
        counter.add_css_class("dim-label");
        counter.add_css_class("numeric");

        // Ellipsized, and it is the only thing in the bar that is: the bar now has a
        // document pane to fit inside rather than a whole window (issue #26), and this
        // sentence is the longest thing in it by far. A narrow window shortens the
        // hint about having just wrapped; it never takes the entry, the counter or the
        // buttons off the edge of the pane, which is what the reader would otherwise
        // be left with.
        let wrap = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        wrap.add_css_class("dim-label");
        wrap.add_css_class("caption");

        // Two letters rather than an icon: there is no symbolic in the Adwaita set for
        // "match case", and a button whose meaning the reader has to guess at is worse
        // than one that simply says what it does.
        // Under a context, because two letters on their own say nothing to a
        // translator: this is the pair that shows an alphabet's upper and lower case.
        let cased = gtk::ToggleButton::builder()
            .label(pgettext("the search bar's case switch", "Aa"))
            .build();
        cased.add_css_class("flat");
        // On no key of its own, so no key in brackets — the one rule everything in
        // this window is named by (`chrome.rs`).
        crate::chrome::name(&cased, &gettext("Match Case"));

        let previous = step_button("go-up-symbolic", &gettext("Find Previous"), FIND_PREVIOUS);
        let next = step_button("go-down-symbolic", &gettext("Find Next"), FIND_NEXT);
        // Our own rather than the one `GtkSearchBar` draws for itself: that one carries
        // neither a name nor a key, so it is the one control in the bar a screen reader
        // could only announce as a button (issue #32). This one closes the bar through
        // the same action `Escape` fires, and says so.
        let close = step_button(
            "window-close-symbolic",
            &gettext("Close Search"),
            FIND_CLOSE,
        );

        let row = gtk::Box::builder().spacing(6).build();
        row.append(&entry);
        row.append(&counter);
        row.append(&wrap);
        row.append(&cased);
        row.append(&previous);
        row.append(&next);
        row.append(&close);

        let bar = gtk::SearchBar::builder()
            .child(&row)
            .show_close_button(false)
            .build();
        bar.connect_entry(&entry);

        let find = Rc::new(Self {
            bar,
            entry,
            counter,
            wrap,
            cased,
            reading,
            editing,
            mode: Cell::new(Mode::Read),
            running: RefCell::new(Query::default()),
            total: Cell::new(0),
            at: Cell::new(0),
            closed: RefCell::new(None),
        });

        let typed = Rc::downgrade(&find);
        find.entry.connect_search_changed(move |_| {
            if let Some(find) = typed.upgrade() {
                find.run(true);
            }
        });

        // Enter walks the matches, as it does in every find bar. Shift+Enter walks them
        // the other way, which `GtkSearchEntry` has no signal for, so both are read off
        // the key rather than one from the signal and one from a controller.
        let keys = gtk::EventControllerKey::new();
        // Before the text inside the entry rather than after it: `GtkText` answers
        // Return itself, and a handler waiting behind it would never be reached.
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let stepping = Rc::downgrade(&find);
        keys.connect_key_pressed(move |_, key, _, state| {
            let Some(find) = stepping.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if !matches!(key, gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter) {
                return glib::Propagation::Proceed;
            }
            find.step(!state.contains(gtk::gdk::ModifierType::SHIFT_MASK));
            glib::Propagation::Stop
        });
        find.entry.add_controller(keys);

        let toggled = Rc::downgrade(&find);
        find.cased.connect_toggled(move |_| {
            if let Some(find) = toggled.upgrade() {
                find.run(true);
            }
        });

        // The bar's own close button, and everything else that puts it away: whichever
        // it was, the marks come off the document here.
        let closing = Rc::downgrade(&find);
        find.bar.connect_search_mode_enabled_notify(move |bar| {
            if let Some(find) = closing.upgrade()
                && !bar.is_search_mode()
            {
                find.put_away();
            }
        });

        find.install_actions(window);
        find
    }

    fn install_actions(self: &Rc<Self>, window: &adw::ApplicationWindow) {
        let opening = Rc::downgrade(self);
        let open = gio::SimpleAction::new(bare(FIND), None);
        open.connect_activate(move |_, _| {
            if let Some(find) = opening.upgrade() {
                find.open();
            }
        });
        window.add_action(&open);

        for (name, forward) in [(FIND_NEXT, true), (FIND_PREVIOUS, false)] {
            let stepping = Rc::downgrade(self);
            let action = gio::SimpleAction::new(bare(name), None);
            action.connect_activate(move |_, _| {
                if let Some(find) = stepping.upgrade() {
                    // A reader who presses Ctrl+G before Ctrl+F means to search: the
                    // bar comes up rather than the key doing nothing.
                    if find.bar.is_search_mode() {
                        find.step(forward);
                    } else {
                        find.open();
                    }
                }
            });
            window.add_action(&action);
        }

        // Escape, and it is a real accelerator rather than a key handler so that it is
        // in the table every other shortcut is in and is held to it by the same test.
        // Disabled while the bar is shut, which is what keeps Escape meaning whatever
        // else it means in this window — probed on GTK 4.20.4, where
        // `gtk_shortcut_action_activate` on a `GtkNamedAction` naming a disabled action
        // answers FALSE and answers TRUE the moment it is enabled, and a shortcut that
        // did not activate does not consume the key.
        let closing = Rc::downgrade(self);
        let close = gio::SimpleAction::new(bare(FIND_CLOSE), None);
        close.set_enabled(false);
        close.connect_activate(move |_, _| {
            if let Some(find) = closing.upgrade() {
                find.close();
            }
        });
        window.add_action(&close);

        // Weak, because the bar is inside the window: a closure holding it would keep
        // the window alive for as long as the bar, and the bar for as long as the
        // window (invariant 7).
        let arming = window.downgrade();
        self.bar.connect_search_mode_enabled_notify(move |bar| {
            let Some(window) = arming.upgrade() else {
                return;
            };
            if let Some(action) = window.lookup_action(bare(FIND_CLOSE))
                && let Some(action) = action.downcast_ref::<gio::SimpleAction>()
            {
                action.set_enabled(bar.is_search_mode());
            }
        });
    }

    /// The bar, for the window to put at the top of the pane the document is in.
    pub(crate) fn widget(&self) -> &gtk::Widget {
        self.bar.upcast_ref()
    }

    /// Calls `handler` whenever the search closes, however it was closed — so that the
    /// window can give the keyboard back to whatever the reader was doing.
    pub(crate) fn connect_closed(&self, handler: impl Fn() + 'static) {
        *self.closed.borrow_mut() = Some(Rc::new(handler));
    }

    /// `Ctrl+F`: the bar, with the keyboard in it and whatever was in it selected, so
    /// that typing replaces the last search and Enter repeats it.
    pub(crate) fn open(self: &Rc<Self>) {
        self.bar.set_search_mode(true);
        self.entry.grab_focus();
        self.entry.select_region(0, -1);
        self.run(true);
    }

    /// Puts the search away and takes every mark off the document.
    pub(crate) fn close(self: &Rc<Self>) {
        self.bar.set_search_mode(false);
    }

    /// The reader has changed surfaces: the one they left stops showing a search, and
    /// the one they are on starts showing one, from its first match.
    ///
    /// From the first, because the matches themselves are different — the source has
    /// occurrences the page does not — so "the third of five" is not the third of
    /// anything over there. And without taking the reader anywhere: switching modes
    /// already puts them where they were reading, mapped through the anchor map
    /// (invariant 5), and a search must not overrule that. The next press of Next or
    /// Enter is what moves them.
    pub(crate) fn look_in(self: &Rc<Self>, mode: Mode) {
        let left = self.mode.replace(mode);
        if left == mode {
            return;
        }
        self.surface(left).hide_matches();
        if self.bar.is_search_mode() {
            self.at.set(0);
            self.wrap.set_label("");
            self.run(false);
        }
    }

    /// Types `text` into the bar, exactly as pressing the keys does — the search is
    /// then the entry's own doing and nothing here.
    pub(crate) fn type_query(&self, text: &str) {
        self.entry.set_text(text);
    }

    /// One question about the bar as the reader sees it, or `None` when it is not a
    /// question about the bar at all.
    pub(crate) fn showing(&self, of: &str) -> Option<String> {
        match of {
            "find-shown" => Some(self.bar.is_search_mode().to_string()),
            "find-query" => Some(self.entry.text().to_string()),
            "find-counter" => Some(self.counter.label().to_string()),
            "find-wrap" => Some(self.wrap.label().to_string()),
            "find-cased" => Some(self.cased.is_active().to_string()),
            _ => None,
        }
    }

    /// Searches for what is in the entry now.
    ///
    /// `bring` is whether the reader is to be taken to the match this lands on: they
    /// are when they typed it, and they are not when the bar is only catching up with a
    /// surface they have just moved to.
    fn run(self: &Rc<Self>, bring: bool) {
        let wanted = Query {
            text: self.entry.text().to_string(),
            cased: self.cased.is_active(),
        };
        if *self.running.borrow() != wanted {
            self.at.set(0);
            self.wrap.set_label("");
        }
        *self.running.borrow_mut() = wanted.clone();

        if wanted.text.is_empty() {
            self.total.set(0);
            self.surface(self.mode.get()).hide_matches();
            self.retell();
            return;
        }
        self.surface(self.mode.get())
            .show_matches(&wanted, self.at.get(), bring, self.counted());
    }

    /// Walks to the next match, or the one before — and says so when that carried the
    /// reader past an end of the document and round to the other one.
    fn step(self: &Rc<Self>, forward: bool) {
        let total = self.total.get();
        if total == 0 {
            return;
        }
        let at = self.at.get();
        let (next, wrapped) = if forward {
            (if at + 1 >= total { 0 } else { at + 1 }, at + 1 >= total)
        } else {
            (if at == 0 { total - 1 } else { at - 1 }, at == 0)
        };
        self.at.set(next);
        self.wrap.set_label(&match (wrapped, forward) {
            (false, _) => String::new(),
            (true, true) => gettext(WRAPPED_TO_THE_TOP),
            (true, false) => gettext(WRAPPED_TO_THE_BOTTOM),
        });
        let running = self.running.borrow().clone();
        self.surface(self.mode.get())
            .show_matches(&running, next, true, self.counted());
        self.retell();
    }

    /// What a surface does with the count it made: it becomes the counter, unless the
    /// reader has moved on to another search since it was asked.
    fn counted(self: &Rc<Self>) -> Counted {
        let find = Rc::downgrade(self);
        Rc::new(move |counted: &Query, total: usize| {
            let Some(find) = find.upgrade() else {
                return;
            };
            if *find.running.borrow() != *counted {
                return;
            }
            find.total.set(total);
            if total > 0 {
                find.at.set(find.at.get() % total);
            }
            find.retell();
        })
    }

    /// The counter, as the reader reads it.
    fn retell(&self) {
        let total = self.total.get();
        if self.running.borrow().text.is_empty() {
            self.counter.set_label("");
            self.entry.remove_css_class("error");
            return;
        }
        if total == 0 {
            self.counter.set_label(&gettext(NOTHING_FOUND));
            self.wrap.set_label("");
            self.entry.add_css_class("error");
            return;
        }
        // The whole phrase rather than a number glued to a word: which way round the
        // two numbers go is the translator's to decide.
        self.counter.set_label(
            &gettext("{position} of {total}")
                .replace("{position}", &(self.at.get() + 1).to_string())
                .replace("{total}", &total.to_string()),
        );
        self.entry.remove_css_class("error");
    }

    /// Everything closing the search undoes — reached however it was closed: Escape,
    /// the bar's own close button, the window being given another document.
    fn put_away(&self) {
        self.reading.hide_matches();
        self.editing.hide_matches();
        *self.running.borrow_mut() = Query::default();
        self.total.set(0);
        self.at.set(0);
        self.counter.set_label("");
        self.wrap.set_label("");
        self.entry.remove_css_class("error");
        let handler = self.closed.borrow().clone();
        if let Some(handler) = handler {
            handler();
        }
    }

    fn surface(&self, mode: Mode) -> &Rc<dyn Searchable> {
        match mode {
            Mode::Read => &self.reading,
            Mode::Edit => &self.editing,
        }
    }
}

/// One of the two buttons that walk the matches. Bound to a window action, so it does
/// exactly what the key beside it in the tooltip does.
fn step_button(icon: &str, saying: &str, action: &str) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name(icon)
        .action_name(action)
        .build();
    crate::chrome::name(&button, saying);
    button
}

/// An action's bare name, as a window registers it, from the full one a widget uses.
fn bare(action: &str) -> &str {
    action.strip_prefix("win.").unwrap_or(action)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(text: &str, cased: bool, haystack: &str) -> Vec<String> {
        let haystack: Vec<char> = haystack.chars().collect();
        Query {
            text: text.to_owned(),
            cased,
        }
        .matches(&haystack)
        .into_iter()
        .map(|(from, to)| haystack[from..to].iter().collect())
        .collect()
    }

    /// The rule both surfaces search by, and the reason the counter can be believed:
    /// matches are in order, they do not overlap, and case is the reader's to ask for.
    #[test]
    fn a_search_finds_every_occurrence_in_order_and_none_of_them_twice() {
        assert_eq!(
            found("needle", false, "The needle is here: needle."),
            ["needle", "needle"],
        );
        assert_eq!(
            found("aa", false, "aaa"),
            ["aa"],
            "overlapping occurrences were counted twice",
        );
        assert_eq!(found("needle", false, "no such word"), Vec::<String>::new());
        assert_eq!(
            found("", false, "anything at all"),
            Vec::<String>::new(),
            "an empty query matched something",
        );
        assert_eq!(
            found("longer than the haystack", false, "short"),
            Vec::<String>::new(),
        );
    }

    /// Case-insensitive by default, and exactly as typed when the reader asks. The
    /// matched text is what the document says, never what the reader typed.
    #[test]
    fn case_is_ignored_until_the_reader_asks_for_it() {
        assert_eq!(
            found("needle", false, "Needle needle NEEDLE"),
            ["Needle", "needle", "NEEDLE"],
        );
        assert_eq!(found("needle", true, "Needle needle NEEDLE"), ["needle"]);
        assert_eq!(found("straße", false, "STRASSE Straße"), ["Straße"]);
    }

    /// The reason case is compared character by character rather than by lowercasing
    /// both sides: `İ` lowercases to two characters, so a query folded whole would
    /// find its match at an offset the document does not have.
    #[test]
    fn a_letter_whose_lowercase_is_longer_than_it_is_still_found_where_it_stands() {
        assert_eq!(found("İ", false, "aİb"), ["İ"]);
        assert_eq!(found("i̇", false, "aİb"), Vec::<String>::new());
    }

    /// An action is named once, in full, and registered by its bare name. A window that
    /// registered the full name would have every accelerator silently do nothing.
    #[test]
    fn every_search_action_is_registered_under_the_name_its_shortcut_uses() {
        for action in [FIND, FIND_NEXT, FIND_PREVIOUS, FIND_CLOSE] {
            assert!(
                action.starts_with("win."),
                "{action} is not a window action"
            );
            assert_eq!(bare(action), &action["win.".len()..]);
        }
    }
}
