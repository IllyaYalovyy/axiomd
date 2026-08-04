//! Issue #32: every control the reader can press is named by one rule.
//!
//! Hovering a control says `"Name (Key)"`; a screen reader announces `"Name"` alone,
//! because GTK announces the key itself and a name carrying it would say it twice.
//! Before this the rule was applied by hand and therefore unevenly: `Back`, `Forward`
//! and `Open` had no key in their words, `Show the outline (F9)` had the key inside the
//! only name a screen reader could find, and the search bar's close button had no name
//! at all.
//!
//! The sweep below is deliberately not a list of controls to check. It asks the running
//! window for *every* control the reader can reach and holds all of them to the rule —
//! a test naming the controls it wanted would pass over exactly the control nobody
//! remembered to name.

use axiomd_e2e::{App, Fixture, Named};

const NOTES: &str = "# Release Notes\n\n## Fixed\n\nThe first paragraph.\n";

/// Every control in the window, with the search bar up and the main menu open, so that
/// the bar's own controls and the menu's zoom row are among them.
fn every_control(app: &App) -> Vec<Named> {
    app.activate("win.find");
    app.wait_for("the search bar to be drawn", || {
        app.controls()
            .iter()
            .any(|control| control.action == "win.find-next")
    });
    assert!(app.press_key("F10"), "F10 is on nothing in this window");
    app.wait_for("the main menu to open", || app.menu().open);
    app.wait_for("the menu's zoom row to be drawn", || {
        app.controls()
            .iter()
            .any(|control| control.action == "win.zoom-in")
    });
    app.controls()
}

/// The rule itself, over every control at once.
///
/// Two halves, and both matter: a control whose tooltip says the key but whose
/// accessible name does not exist is the defect a screen reader meets, and a control
/// named for a screen reader whose tooltip forgot the key is the one a pointer meets.
#[test]
fn every_control_says_its_name_with_its_key_and_announces_the_name_alone() {
    let fixture = Fixture::new("naming-rule");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", NOTES));

    let controls = every_control(&app);
    assert!(
        controls.len() > 10,
        "only {} controls were found, so the sweep is not sweeping: {controls:#?}",
        controls.len(),
    );

    for control in &controls {
        assert_ne!(
            control.announced, "undefined",
            "a screen reader has no name for this control: {control:#?}",
        );
        // A control with no tooltip is one of GTK's own — the window's close button —
        // which GTK names for itself and this application never writes words for.
        if control.tooltip.is_empty() || control.tooltip.ends_with("-symbolic") {
            continue;
        }
        let expected = match control.key.is_empty() {
            true => control.announced.clone(),
            false => format!("{} ({})", control.announced, control.key),
        };
        assert_eq!(
            control.tooltip, expected,
            "hovering says something other than the announced name and the installed \
             key: {control:#?}",
        );
    }

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// And the words themselves, pinned: the rule above would be satisfied by a control
/// named `Thing`, and header capitalisation and the key a reader would actually press
/// are what the reader is reading.
#[test]
fn the_controls_are_called_what_the_shortcuts_dialog_calls_them() {
    let fixture = Fixture::new("naming-words");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", NOTES));

    let said: Vec<String> = every_control(&app)
        .iter()
        .filter(|control| !control.tooltip.ends_with("-symbolic"))
        .map(|control| format!("{}\t{}", control.action, control.tooltip))
        .collect();

    assert_eq!(
        said,
        [
            "\tMatch Case",
            "win.find-previous\tFind Previous (Shift+Ctrl+G)",
            "win.find-next\tFind Next (Ctrl+G)",
            "win.find-close\tClose Search (Escape)",
            "win.outline\tOutline (F9)",
            "win.back\tBack (Alt+Left)",
            "win.forward\tForward (Alt+Right)",
            "app.open\tOpen Document (Ctrl+O)",
            "win.mode\tEdit the Source (Ctrl+E)",
            // The main menu fires no action of ours, so `F10` is GTK's to announce.
            "\tMain Menu",
            "win.zoom-out\tZoom Out (Ctrl+-)",
            "win.zoom-reset\tReset Zoom (Ctrl+0)",
            "win.zoom-in\tZoom In (Ctrl++)",
        ],
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The search bar's close button is a control the reader can press, not only one they
/// can read: a named affordance that does nothing would be the worse defect.
#[test]
fn the_search_bars_close_button_puts_the_bar_away() {
    let fixture = Fixture::new("naming-close-search");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", NOTES));

    app.activate("win.find");
    app.search_for("paragraph");
    app.wait_until_counter("1 of 1");

    app.press_control("Close Search");
    app.wait_for("the search bar to go away", || !app.search().shown);
    assert_eq!(app.dom_text("h1"), "Release Notes");

    assert!(app.close().is_empty(), "the launch left processes behind");
}
