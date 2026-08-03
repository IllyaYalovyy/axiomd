//! The preferences dialog: every knob axiomd has, in one place, applying as it is
//! turned.
//!
//! A dialog the reader asked for is the one kind axiomd has (`ux_decisions.md`):
//! nothing here ever appears on the open/view path. Every row is bound to its setting
//! in both directions, so a row is never a control that merely looks set — closing the
//! dialog is not a confirmation step and there is no Apply button, because there is
//! nothing left to apply.
//!
//! Rows for a capability that has not landed yet (autosave, spellcheck, the engine
//! list, plugins) still write their setting, which is what #16, #17 and #18 read when
//! they arrive. Every later feature with a knob adds its row here (invariant 14).

use std::rc::Rc;

use adw::prelude::*;

use super::{Key, Settings};

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
    dialog.add(&rendering(settings));
    dialog.present(Some(parent));
}

/// What a document looks like: the two things a reader changes while reading.
fn appearance(settings: &Rc<Settings>) -> adw::PreferencesPage {
    let group = adw::PreferencesGroup::builder().title("Appearance").build();
    group.add(&choice(
        settings,
        Key::Theme,
        "Theme",
        "Follow the desktop's colour scheme, or override it",
        &THEMES,
    ));
    let limit = toggle(
        settings,
        Key::ReadingWidthLimited,
        "Limit the reading width",
        "Hold text to a comfortable measure instead of filling the window",
    );
    let width = number(
        settings,
        Key::ReadingWidth,
        "Reading width",
        "The measure text is held to, in multiples of its own size",
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
        "Show the outline",
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
        "Write edits back to the file without being asked",
    );
    let delay = number(
        settings,
        Key::AutosaveDelay,
        "Autosave delay",
        "Seconds of quiet before an edit is written",
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
        "Check spelling",
        "Mark misspelled words while editing. Reading is never affected.",
    ));

    page("Editing", "document-edit-symbolic", group)
}

/// How a document is turned into a page: the engine, and the optional capabilities on
/// top of it.
fn rendering(settings: &Rc<Settings>) -> adw::PreferencesPage {
    let engines: Vec<(&'static str, &'static str)> = crate::document::engines()
        .into_iter()
        .map(|engine| (engine.as_str(), engine.as_str()))
        .collect();

    let group = adw::PreferencesGroup::builder().title("Rendering").build();
    group.add(&choice(
        settings,
        Key::Engine,
        "Markdown engine",
        "The parser documents are read with",
        &engines,
    ));

    let plugins = adw::PreferencesGroup::builder()
        .title("Plugins")
        .description("Rendering capabilities beyond the core, each one optional")
        .build();
    // Nothing registers a plugin yet (#16). The group says so rather than standing
    // empty, and every plugin that lands adds its own switch here.
    plugins.add(
        &adw::ActionRow::builder()
            .title("No plugins yet")
            .subtitle("Diagrams, mathematics and the rest arrive here as they are added.")
            .sensitive(false)
            .build(),
    );

    let page = page("Rendering", "view-paged-symbolic", group);
    page.add(&plugins);
    page
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
