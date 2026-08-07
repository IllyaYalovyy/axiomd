//! The preferences dialog, asserted against the running application.
//!
//! Every row is driven the way the reader drives it — the dialog they opened with
//! `Ctrl+comma`, the control they turned — and every assertion is what they would then
//! see: the document's own measure and colour, read out of the page in front of them,
//! and the setting still there when they come back tomorrow.
//!
//! The two things a preference must never cost are asserted beside every visible one:
//! the view's load count and the window's page count, which are what a reload and a
//! re-render would move. A preference restyles the document the reader is looking at
//! and nothing else (invariant 9).

use axiomd_e2e::{App, Fixture, Preferences};

/// Something long enough to have a measure worth holding.
const NOTES: &str = "\
# Reading

A paragraph that is comfortably longer than one line, so that the measure the reader
chose is the thing deciding where it wraps.

## Details

```rust
fn main() {
    println!(\"hello\");
}
```
";

/// The width of the column the document is laid out in, as CSS resolves it.
const MEASURE: &str = "getComputedStyle(document.querySelector('article.markdown')).maxWidth";

/// The colour the reader is reading on.
const PAGE_COLOUR: &str = "getComputedStyle(document.body).backgroundColor";

/// The colour of the first highlighted piece of the document's code block — the other
/// half of a theme change, and one that comes from a different stylesheet.
const CODE_COLOUR: &str = "(() => { const token = document.querySelector('pre code span[class^=sy-]'); \
     return token === null ? '' : getComputedStyle(token).color; })()";

/// 46rem, the measure a first run reads at, at the pinned 16px root font size.
const DEFAULT_MEASURE: &str = "736px";

/// The dialog is the reader's own doing. Opening a document is not, and never puts a
/// question in front of them (`ux_decisions.md`).
#[test]
fn preferences_open_when_they_are_asked_for_and_reading_asks_nothing() {
    let fixture = Fixture::new("preferences-open");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", NOTES));

    assert_eq!(
        app.visible_dialog(),
        "",
        "opening a document put a dialog in front of the reader",
    );

    // `Ctrl+comma`: the accelerator activates this action, and so does the menu item.
    app.activate("app.preferences");
    assert_eq!(app.visible_dialog(), "Preferences");

    // The plugin section lists what this build has, each one switched on until the
    // reader says otherwise (#16).
    assert_eq!(app.preference("Emoji Shortcodes"), "true");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The reading width, changed while the reader is reading: the document in front of
/// them relaid out where it stands, without being loaded or rendered again.
#[test]
fn the_reading_width_applies_to_the_document_the_reader_is_looking_at() {
    let fixture = Fixture::new("preferences-width");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", NOTES));
    assert_eq!(app.dom(MEASURE), DEFAULT_MEASURE);

    let loads = app.navigation_count();
    let pages = app.render_count();

    app.activate("app.preferences");
    app.set_preference("Reading Width", "80");
    // 80rem at the pinned root font size.
    app.wait_until(&format!("{MEASURE} === '1280px'"));

    // The reader asked for a wider column, not for their document back.
    assert_eq!(app.navigation_count(), loads, "the document was reloaded");
    assert_eq!(app.render_count(), pages, "the document was rendered again");

    // And the other half of the preference: a document that fills the window.
    app.set_preference("Limit Reading Width", "false");
    app.wait_until(&format!("{MEASURE} === 'none'"));
    assert_eq!(app.navigation_count(), loads, "the document was reloaded");
    assert_eq!(app.render_count(), pages, "the document was rendered again");

    app.set_preference("Limit Reading Width", "true");
    app.wait_until(&format!("{MEASURE} === '1280px'"));

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The theme override, which the rendered document follows: a restyle, never a
/// re-parse (invariant 9). This is the in-app half of UT-008, including the part of
/// it about code blocks changing palette with everything else.
#[test]
fn the_theme_override_recolours_the_document_without_rendering_it_again() {
    let fixture = Fixture::new("preferences-theme");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", NOTES));
    assert_eq!(
        app.dom(PAGE_COLOUR),
        "rgb(255, 255, 255)",
        "the pinned desktop is a light one, and the document was not on it",
    );

    let loads = app.navigation_count();
    let pages = app.render_count();
    let code_was = app.dom(CODE_COLOUR);
    assert_ne!(code_was, "", "the fixture's code block was not highlighted");

    app.activate("app.preferences");
    app.set_preference("Theme", "Dark");
    // The dark page colour from the bundled stylesheet.
    app.wait_until(&format!("{PAGE_COLOUR} === 'rgb(29, 29, 32)'"));

    assert_eq!(app.navigation_count(), loads, "the document was reloaded");
    assert_eq!(app.render_count(), pages, "the document was rendered again");
    assert_ne!(
        app.dom(CODE_COLOUR),
        code_was,
        "the document went dark and its code block kept the light palette",
    );

    // Back to the desktop's own, which the pinned one says is light.
    app.set_preference("Theme", "System");
    app.wait_until(&format!("{PAGE_COLOUR} === 'rgb(255, 255, 255)'"));

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The identifiers this build has that no reader should ever be shown: the engines',
/// the plugins', and the values the theme choice stores.
///
/// Spelled out here rather than read back from the tables that build the dialog, so
/// that a dialog naming everything after its own keys cannot agree with itself.
const IDENTIFIERS: &[&str] = &[
    "comrak",
    "pulldown-cmark",
    "emoji",
    "math",
    "mermaid",
    "system",
    "light",
    "dark",
];

/// The words header capitalisation leaves in lower case when they are not first.
const MINOR: &[&str] = &[
    "a", "an", "the", "and", "or", "nor", "but", "as", "at", "by", "for", "from", "in", "of", "on",
    "to", "with",
];

/// Every word the preferences dialog says, held to the way GNOME writes them
/// (issue #31): headings and row titles in header capitals, the line under each one a
/// sentence that ends like one, and nowhere an identifier that belongs to the code.
///
/// Read off the dialog rather than listed here, so a row added later — by a feature, a
/// plugin or an engine this test has never heard of — is held to the same rules the
/// moment it appears.
#[test]
fn every_word_the_dialog_says_is_the_readers_rather_than_the_codes() {
    let fixture = Fixture::new("preferences-words");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", NOTES));

    app.activate("app.preferences");
    let said = app.preferences();
    assert!(said.len() > 8, "the dialog said almost nothing: {said:?}",);

    for entry in &said {
        assert!(
            !entry.title.is_empty(),
            "something in the dialog is untitled"
        );
        for (position, word) in entry.title.split(' ').enumerate() {
            let minor = position > 0 && MINOR.contains(&word);
            assert!(
                minor
                    || word
                        .chars()
                        .next()
                        .is_some_and(|first| !first.is_lowercase()),
                "{:?} is not header capitalised: {word:?}",
                entry.title,
            );
        }

        if !entry.subtitle.is_empty() {
            assert!(
                entry.subtitle.ends_with('.'),
                "{:?} says {:?}, which is not a finished sentence",
                entry.title,
                entry.subtitle,
            );
            assert!(
                entry
                    .subtitle
                    .chars()
                    .next()
                    .is_some_and(|first| !first.is_lowercase()),
                "{:?} says {:?}, which does not start a sentence",
                entry.title,
                entry.subtitle,
            );
        }

        for shown in std::iter::once(&entry.title)
            .chain(std::iter::once(&entry.subtitle))
            .chain(entry.options.iter())
        {
            assert!(
                !IDENTIFIERS.contains(&shown.as_str()),
                "the dialog shows the reader {shown:?}, which is an identifier",
            );
        }
    }

    // And the row the identifiers were most visible on, by name: every engine this
    // build has, each one named for a reader.
    let engines = said
        .iter()
        .find(|entry| entry.title == "Markdown Engine")
        .unwrap_or_else(|| panic!("the dialog has no engine row: {said:?}"));
    assert_eq!(engines.options, ["Comrak", "Pulldown"]);

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Every row the dialog offers, turned and read back, and the setting behind it
/// written down where the next launch will find it.
///
/// Only what the dialog writes down is asserted here. What each row then *does* is
/// asserted where that behaviour lives — the plugin switch and the engine row in
/// `plugins.rs` and `engines.rs`, spelling in `editor.rs`'s own suite (#21).
#[test]
fn every_row_the_dialog_offers_turns_and_is_written_down() {
    let fixture = Fixture::new("preferences-rows");
    let preferences = Preferences::new("preferences-rows");
    let app = axiomd_e2e::launch_with(&fixture.write("notes.md", NOTES), &preferences);

    app.activate("app.preferences");
    assert_eq!(app.visible_dialog(), "Preferences");

    // The row, what it starts as, what the reader turns it to, and what that is in
    // the store they will come back to.
    let rows = [
        ("Theme", "System", "Dark", "theme", "'dark'"),
        (
            "Limit Reading Width",
            "true",
            "false",
            "reading-width-limited",
            "false",
        ),
        ("Reading Width", "46", "72", "reading-width", "72"),
        (
            "Remember Reading Position",
            "true",
            "false",
            "remember-position",
            "false",
        ),
        ("Autosave", "true", "false", "autosave", "false"),
        ("Autosave Delay", "2", "9", "autosave-delay", "9"),
        ("Check Spelling", "true", "false", "spellcheck", "false"),
        (
            "Emoji Shortcodes",
            "true",
            "false",
            "disabled-plugins",
            "['emoji']",
        ),
        // The reader picks an engine by its name and the store keeps its identifier:
        // the two sides of issue #31's rule that no chooser shows a developer id.
        (
            "Markdown Engine",
            "Comrak",
            "Pulldown",
            "engine",
            "'pulldown-cmark'",
        ),
    ];

    for (row, first_run, turned_to, key, stored) in rows {
        assert_eq!(
            app.preference(row),
            first_run,
            "{row} did not start out right"
        );
        app.set_preference(row, turned_to);
        assert_eq!(app.preference(row), turned_to, "{row} did not stay turned");
        preferences.wait_until(key, stored);
    }

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A preference is a preference: the reader sets it once and comes back to it, in a
/// document opened by an application that has been started again since.
#[test]
fn a_preference_is_still_there_when_the_reader_comes_back() {
    let fixture = Fixture::new("preferences-restart");
    let notes = fixture.write("notes.md", NOTES);
    let preferences = Preferences::new("preferences-restart");

    let app = axiomd_e2e::launch_with(&notes, &preferences);
    app.activate("app.preferences");
    app.set_preference("Reading Width", "72");
    app.set_preference("Theme", "Dark");
    app.wait_until(&format!("{MEASURE} === '1152px'"));
    preferences.wait_until("reading-width", "72");
    preferences.wait_until("theme", "'dark'");
    assert!(app.close().is_empty(), "the launch left processes behind");

    let returning: App = axiomd_e2e::launch_with(&notes, &preferences);

    // The document arrives the way they left it — no dialog, no reapplying.
    assert_eq!(returning.dom(MEASURE), "1152px");
    assert_eq!(returning.dom(PAGE_COLOUR), "rgb(29, 29, 32)");
    assert_eq!(returning.visible_dialog(), "");

    // And the dialog agrees with the document.
    returning.activate("app.preferences");
    assert_eq!(returning.preference("Reading Width"), "72");
    assert_eq!(returning.preference("Theme"), "Dark");

    assert!(
        returning.close().is_empty(),
        "the launch left processes behind",
    );
}

/// A preference belongs to the reader, not to one window: a second window is already
/// showing documents the way they asked, and a change reaches both.
#[test]
fn a_preference_reaches_every_window_that_is_open() {
    let fixture = Fixture::new("preferences-windows");
    let notes = fixture.write("notes.md", NOTES);
    let more = fixture.write("more.md", NOTES);

    let app = axiomd_e2e::launch(&notes);
    app.activate("app.preferences");
    app.set_preference("Reading Width", "80");
    app.wait_until(&format!("{MEASURE} === '1280px'"));

    app.open(&more);
    app.wait_until_windows(2);
    // The newest window, which never saw the preference being set.
    assert_eq!(app.dom(MEASURE), "1280px");

    app.select_window(0);
    assert_eq!(app.dom(MEASURE), "1280px", "the first window fell behind");

    assert!(app.close().is_empty(), "the launch left processes behind");
}
