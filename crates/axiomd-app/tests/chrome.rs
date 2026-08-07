//! Issue #29: the chrome a GNOME reader expects to find, asserted through the running
//! application.
//!
//! What the application is (the About dialog), what its keys do (the keyboard-shortcuts
//! dialog on `Ctrl+?`), the menu that leads to both and opens on `F10`, and the two
//! ways this desktop redoes an edit.
//!
//! The shortcuts dialog is read row by row out of the dialog the reader is looking at
//! and held to a list spelled out here rather than read back from the table that
//! builds it: a table that fills a dialog with nothing would otherwise agree with
//! itself. Together with `shell.rs`'s completeness test — every key the application
//! installs is one of these — a shortcut the reader can press and cannot find is
//! caught from both sides.

use axiomd_e2e::{Fixture, Preferences};

const NOTES: &str = "# Release Notes\n\nThe first paragraph.\n";

/// Every row of the shortcuts dialog, in the order the reader reads them: what it is
/// called, and the keys it is on as GTK spells them.
const LISTED: &[(&str, &str)] = &[
    ("New Document", "<Control>n"),
    ("Open Document", "<Control>o"),
    ("Preferences", "<Control>comma"),
    ("Keyboard Shortcuts", "<Control>question"),
    ("Close Window", "<Control>w"),
    ("Quit", "<Control>q"),
    ("Back", "<Alt>Left"),
    ("Forward", "<Alt>Right"),
    ("Outline", "F9"),
    ("Zoom In", "<Control>plus <Control>equal <Control>KP_Add"),
    ("Zoom Out", "<Control>minus <Control>KP_Subtract"),
    ("Reset Zoom", "<Control>0 <Control>KP_0"),
    ("Find", "<Control>f"),
    ("Find Next", "<Control>g"),
    ("Find Previous", "<Shift><Control>g"),
    ("Close Search", "Escape"),
    ("Switch Between Reading and Editing", "<Control>e"),
    ("Save", "<Control>s"),
    ("Save As", "<Shift><Control>s"),
    ("Undo", "<Control>z"),
    ("Redo", "<Shift><Control>z <Control>y"),
    ("Print", "<Control>p"),
    ("Export", "<Shift><Control>e"),
];

/// The primary menu ends the way the HIG asks every GNOME primary menu to end, and it
/// offers no way out of the application: closing a window and quitting are keys and the
/// window's own controls, and the shortcuts dialog above About is where they are found.
#[test]
fn the_primary_menu_ends_with_the_shortcuts_and_about_and_offers_no_way_out() {
    let fixture = Fixture::new("chrome-menu");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", NOTES));

    let menu = app.menu();
    assert_eq!(
        menu.offers(),
        [
            ("New Document", "app.new"),
            ("Open…", "app.open"),
            ("Find…", "win.find"),
            // The submenu of engines, and what a reader who points at it reads: every
            // engine this build has, each named for them, each carrying the identifier
            // the window is switched by (issues #17 and #31).
            ("Markdown Engine", ""),
            ("Comrak", "win.engine::comrak"),
            ("Pulldown", "win.engine::pulldown-cmark"),
            ("Edit Source", "win.mode"),
            ("Save", "win.save"),
            ("Save As…", "win.save-as"),
            ("Print…", "win.print"),
            ("Export…", "win.export"),
            ("Preferences", "app.preferences"),
            ("Keyboard Shortcuts", "app.shortcuts"),
            ("About axiomd", "app.about"),
        ],
    );
    assert!(!menu.open, "the menu opened itself");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// `F10` opens the main menu, which is the only way to it without a pointer.
#[test]
fn f10_opens_the_main_menu() {
    let fixture = Fixture::new("chrome-f10");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", NOTES));

    assert!(!app.menu().open, "the menu was open before F10");
    assert!(app.press_key("F10"), "F10 did nothing");
    assert!(app.menu().open, "F10 did not open the main menu");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// About says what this build is, in the metainfo's own words — the same file the
/// software centre reads, so the two cannot tell the reader different things.
#[test]
fn about_says_what_this_build_is() {
    let fixture = Fixture::new("chrome-about");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", NOTES));

    // Reading a document asks nothing (`ux_decisions.md`); About is the reader's own
    // doing, from the menu item or this action behind it.
    assert_eq!(app.visible_dialog(), "");
    app.activate("app.about");
    assert_eq!(app.visible_dialog(), "About");

    let about = app.about();
    assert_eq!(
        (
            about.name.as_str(),
            about.developer.as_str(),
            about.version.as_str(),
            about.license.as_str(),
            about.website.as_str(),
            about.issues.as_str(),
        ),
        (
            // Lowercase on purpose (`ux_decisions.md`).
            "axiomd",
            "Illya Yalovyy",
            env!("CARGO_PKG_VERSION"),
            // The GNU GPL version 3 or later, which is what the metainfo declares.
            "Gpl30",
            "https://github.com/IllyaYalovyy/axiomd",
            "https://github.com/IllyaYalovyy/axiomd/issues",
        ),
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// `Ctrl+?` lists every key the reader can press, with the keys each one is on.
///
/// The dialog is read as the reader reads it — the rows on the page in front of them —
/// so a section that was never added, or a row whose action carries no accelerator,
/// is a row that goes missing here.
#[test]
fn the_shortcuts_dialog_lists_every_key_the_reader_can_press() {
    let fixture = Fixture::new("chrome-shortcuts");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", NOTES));

    assert_eq!(app.visible_dialog(), "");
    // Ctrl+?, and the "Keyboard Shortcuts" menu item, are this action.
    app.activate("app.shortcuts");
    assert_eq!(app.visible_dialog(), "Keyboard Shortcuts");

    let expected: Vec<(String, String)> = LISTED
        .iter()
        .map(|(title, keys)| ((*title).to_owned(), (*keys).to_owned()))
        .collect();
    assert_eq!(app.shortcuts(), expected);

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Redo is on both keys this desktop redoes with, and both are pressed here rather
/// than described: `Shift+Ctrl+Z`, which is what GNOME's own editors use, and `Ctrl+Y`
/// beside it for the readers who arrive from elsewhere.
#[test]
fn redo_is_on_both_keys_a_reader_might_press_for_it() {
    let fixture = Fixture::new("chrome-redo");
    // Without autosave, so that what the source says is only ever what the keys did.
    let app = axiomd_e2e::launch_with(
        &fixture.write("notes.md", "original\n"),
        &Preferences::with("chrome-redo", "autosave", "false"),
    );

    app.activate("win.mode");
    app.wait_until_mode("edit");
    app.type_text("typed ");
    assert_eq!(app.source(), "typed original\n");

    assert!(app.press_key("<Control>z"), "Ctrl+Z did nothing");
    assert_eq!(app.source(), "original\n");
    assert!(
        app.press_key("<Shift><Control>z"),
        "Shift+Ctrl+Z did nothing",
    );
    assert_eq!(
        app.source(),
        "typed original\n",
        "Shift+Ctrl+Z did not redo the edit",
    );

    // And the other one, from the same place.
    assert!(app.press_key("<Control>z"), "Ctrl+Z did nothing");
    assert_eq!(app.source(), "original\n");
    assert!(app.press_key("<Control>y"), "Ctrl+Y did nothing");
    assert_eq!(app.source(), "typed original\n", "Ctrl+Y did not redo");

    assert!(app.close().is_empty(), "the launch left processes behind");
}
