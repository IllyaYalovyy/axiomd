//! The preferences dialog: every knob axiomd has, in one place, applying as it is
//! turned.
//!
//! A dialog the reader asked for is the one kind axiomd has (`ux_decisions.md`):
//! nothing here ever appears on the open/view path. Every row is bound to its setting
//! in both directions, so a row is never a control that merely looks set — closing the
//! dialog is not a confirmation step and there is no Apply button, because there is
//! nothing left to apply.
//!
//! Every later feature with a knob adds its row here (invariant 14) — a plugin and an
//! engine both do so by existing, since those two groups are built from what this
//! build has registered. A row is only ever added for a capability this build actually
//! has: a switch that changes nothing is not a preference, it is a promise.
//!
//! # How the rows are written
//!
//! Titles in header capitals and subtitles as finished sentences, the way the HIG
//! writes a boxed list, and never a developer identifier — an engine and a plugin are
//! each offered by the name they give themselves, and what a choice stores is separate
//! from what it shows (issue #31). `preferences.rs` holds every row in this file to
//! that, including rows written after it.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;

use super::{Key, Settings, Watch};

/// What the dialog is called — the title `Ctrl+comma` puts on screen.
const TITLE: &str = "Preferences";

/// The bounds each numeric row offers, which are the bounds its key declares.
/// `the_numbers_the_dialog_offers_are_the_ones_the_schema_allows` holds the two
/// together, so a spin button can never write a value the schema refuses.
pub(super) const BOUNDS: [(Key, i32, i32); 2] =
    [(Key::ReadingWidth, 20, 200), (Key::AutosaveDelay, 1, 60)];

/// The three answers to "which colour scheme", as the reader reads them.
const THEMES: [(&str, &str); 3] = [("System", "system"), ("Light", "light"), ("Dark", "dark")];

/// Puts the preferences dialog on screen over `parent`.
pub(super) fn present(settings: &Rc<Settings>, parent: &impl IsA<gtk::Widget>) {
    let dialog = adw::PreferencesDialog::builder().title(TITLE).build();
    dialog.add(&appearance(settings));
    dialog.add(&editing(settings));

    // The plugin switches are the one kind of row that is not a two-way binding on a
    // key, so they keep their own subscriptions — and let go of them when the dialog
    // the reader opened is closed again (invariant 7).
    let watching = Rc::new(RefCell::new(Vec::new()));
    dialog.add(&rendering(settings, &watching));
    dialog.connect_closed(move |_| watching.borrow_mut().clear());

    dialog.present(Some(parent));
}

/// What a document looks like: the two things a reader changes while reading.
fn appearance(settings: &Rc<Settings>) -> adw::PreferencesPage {
    let group = adw::PreferencesGroup::builder().title("Appearance").build();
    group.add(&choice(
        settings,
        Key::Theme,
        "Theme",
        "Follow the desktop's colour scheme, or override it.",
        &THEMES,
    ));
    let limit = toggle(
        settings,
        Key::ReadingWidthLimited,
        "Limit Reading Width",
        "Hold text to a comfortable measure instead of filling the window.",
    );
    let width = number(
        settings,
        Key::ReadingWidth,
        "Reading Width",
        "The measure text is held to, in multiples of its own size.",
    );
    // A width that cannot apply is not offered: with the limit off, the number below
    // it would be a control the reader can turn that does nothing.
    limit
        .bind_property("active", &width, "sensitive")
        .sync_create()
        .build();
    group.add(&limit);
    group.add(&width);
    group.add(&toggle(
        settings,
        Key::Outline,
        "Show Outline",
        "List a document's headings beside it. F9 shows or hides it in one window.",
    ));

    page("Appearance", "applications-graphics-symbolic", group)
}

/// What editing does, for the editor #18 brings.
fn editing(settings: &Rc<Settings>) -> adw::PreferencesPage {
    let group = adw::PreferencesGroup::builder().title("Editing").build();
    let autosave = toggle(
        settings,
        Key::Autosave,
        "Autosave",
        "Write edits back to the file without being asked.",
    );
    let delay = number(
        settings,
        Key::AutosaveDelay,
        "Autosave Delay",
        "Seconds of quiet before an edit is written.",
    );
    autosave
        .bind_property("active", &delay, "sensitive")
        .sync_create()
        .build();
    group.add(&autosave);
    group.add(&delay);

    group.add(&toggle(
        settings,
        Key::Spellcheck,
        "Check Spelling",
        "Mark misspelled words while editing. Reading is never affected.",
    ));

    page("Editing", "document-edit-symbolic", group)
}

/// How a document is turned into a page: the engine, and the optional capabilities on
/// top of it.
fn rendering(settings: &Rc<Settings>, watching: &Rc<RefCell<Vec<Watch>>>) -> adw::PreferencesPage {
    // Named by the engines themselves, so an engine that lands is offered here without
    // this file learning anything about it — the same shape the plugin group has. The
    // reader reads the engine's name and the setting keeps its identifier, which is
    // what a stored preference and a menu item's target both are (issue #31).
    let engines: Vec<(&'static str, &'static str)> = axiomd_engine::engines()
        .iter()
        .map(|engine| (engine.display_name(), engine.id().as_str()))
        .collect();

    let group = adw::PreferencesGroup::builder().title("Rendering").build();
    group.add(&choice(
        settings,
        Key::Engine,
        "Markdown Engine",
        "The parser documents are read with. One window can be switched to another \
         from its main menu.",
        &engines,
    ));

    let plugins = adw::PreferencesGroup::builder()
        .title("Plugins")
        .description("Rendering capabilities beyond the core, each one optional.")
        .build();
    // One switch per plugin this build has, named by the plugin itself: a capability
    // that lands is offered here without this file learning anything about it.
    for manifest in axiomd_render::Plugins::builtin(&[]).manifests() {
        let (row, watch) = plugin(settings, manifest);
        plugins.add(&row);
        watching.borrow_mut().push(watch);
    }

    let page = page("Rendering", "view-paged-symbolic", group);
    page.add(&plugins);
    page
}

/// One plugin, as a switch the reader turns.
///
/// Not a `bind` like the rows above it: the setting is the list of plugins that are
/// *off*, so what the switch shows and what turning it writes are both about this
/// plugin's place in that list — and an id belonging to a plugin this build does not
/// have is left in it untouched.
fn plugin(
    settings: &Rc<Settings>,
    manifest: &'static axiomd_render::Manifest,
) -> (adw::SwitchRow, Watch) {
    let row = adw::SwitchRow::builder()
        .title(manifest.name)
        .subtitle(manifest.description)
        .active(settings.plugin_enabled(manifest.id))
        .build();
    let writing = settings.clone();
    row.connect_active_notify(move |row| {
        writing.set_plugin_enabled(manifest.id, row.is_active());
    });
    // And the other direction, so a change made elsewhere while this dialog is open is
    // what the switch shows.
    let showing = row.downgrade();
    let reading = settings.clone();
    let watch = settings.follow_plugins(move || {
        if let Some(row) = showing.upgrade() {
            row.set_active(reading.plugin_enabled(manifest.id));
        }
    });
    (row, watch)
}

fn page(title: &str, icon: &str, group: adw::PreferencesGroup) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title(title)
        .icon_name(icon)
        .build();
    page.add(&group);
    page
}

/// A setting that is on or off.
fn toggle(settings: &Rc<Settings>, key: Key, title: &str, subtitle: &str) -> adw::SwitchRow {
    let row = adw::SwitchRow::builder()
        .title(title)
        .subtitle(subtitle)
        .build();
    settings.store.bind(key.name(), &row, "active").build();
    row
}

/// A setting that is a number, offered between the bounds its key declares.
fn number(settings: &Rc<Settings>, key: Key, title: &str, subtitle: &str) -> adw::SpinRow {
    let (low, high) = BOUNDS
        .iter()
        .find(|(bounded, _, _)| *bounded == key)
        .map(|(_, low, high)| (f64::from(*low), f64::from(*high)))
        .expect("a numeric setting declares its bounds");

    let row = adw::SpinRow::builder()
        .title(title)
        .subtitle(subtitle)
        .adjustment(&gtk::Adjustment::new(low, low, high, 1.0, 5.0, 0.0))
        .build();
    // A whole number in the schema, a double on the widget: the two are mapped here
    // rather than the schema being loosened to fit the widget.
    settings
        .store
        .bind(key.name(), &row, "value")
        .mapping(|stored, _| stored.get::<i32>().map(|value| f64::from(value).to_value()))
        .set_mapping(|shown, _| {
            shown
                .get::<f64>()
                .ok()
                .map(|value| (value.round() as i32).to_variant())
        })
        .build();
    row
}

/// A setting that is one of a short list, shown as the reader reads them.
fn choice(
    settings: &Rc<Settings>,
    key: Key,
    title: &str,
    subtitle: &str,
    options: &[(&'static str, &'static str)],
) -> adw::ComboRow {
    let labels: Vec<&str> = options.iter().map(|(label, _)| *label).collect();
    let row = adw::ComboRow::builder()
        .title(title)
        .subtitle(subtitle)
        .model(&gtk::StringList::new(&labels))
        .build();

    let stored_values: Vec<&'static str> = options.iter().map(|(_, value)| *value).collect();
    let shown_values = stored_values.clone();
    settings
        .store
        .bind(key.name(), &row, "selected")
        .mapping(move |stored, _| {
            let stored = stored.get::<String>()?;
            let chosen = stored_values
                .iter()
                .position(|value| *value == stored)
                // A store holding something this build has never heard of — an engine
                // that is no longer built in — falls back to the first entry rather
                // than to an empty combo row.
                .unwrap_or(0);
            Some((chosen as u32).to_value())
        })
        .set_mapping(move |shown, _| {
            let chosen = shown.get::<u32>().ok()? as usize;
            shown_values.get(chosen).map(|value| (*value).to_variant())
        })
        .build();
    row
}
