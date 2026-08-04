//! Issue #30: the window a reader is given — how much chrome it shows when it is
//! narrow, and where it opens when they come back.
//!
//! Both halves are read from the running application the way the reader meets them:
//! the title as the header bar is actually drawing it (cut off, or whole), the
//! controls as the strip actually holds them, and the size a window opens at as the
//! window really is on screen — never as a number the application was asked to store.
//!
//! Nothing here reads a width without first waiting for the window to have been laid
//! out at it. A resize reaches the screen a frame later, so the document filling the
//! window is the signal that the pass which also placed the header has happened; an
//! assertion made before it would be about the window the reader used to have.

use axiomd_e2e::{Fixture, Preferences};

/// A document whose name is long enough for the defect to show. With the whole header
/// drawn, a 400px window left the title 84px to say this in and the reader read
/// "release-not…"; with the narrow header it has 204px and reads all of it.
const NAME: &str = "release-notes-v2.md";

const NOTES: &str = "# Release Notes\n\nThe first paragraph.\n\nA [link](second.md).\n";

/// The widths this issue is about: the one the HIG review screenshotted, and the
/// narrowest a GNOME application is expected to be usable at.
const NARROW: [i32; 2] = [400, 360];

/// What hovering each header control says — the only name a symbolic icon has.
const OUTLINE: &str = "Outline (F9)";
const BACK: &str = "Back (Alt+Left)";
const FORWARD: &str = "Forward (Alt+Right)";
const OPEN: &str = "Open Document (Ctrl+O)";
const EDIT: &str = "Edit the Source (Ctrl+E)";
const MENU: &str = "Main Menu";
/// The window's own close button, which carries no tooltip and is named by the icon it
/// is drawn as. It is in every list below: a window the reader cannot close is not an
/// improvement on one whose title they cannot read.
const CLOSE: &str = "window-close-symbolic";

/// The defect, both halves of it: at the narrow widths the reader reads the whole name
/// of the document they are in, the header bar fits in the window it is in, and
/// everything the header stopped drawing is in the menu.
#[test]
fn a_narrow_window_keeps_its_title_whole_and_offers_the_rest_from_the_menu() {
    let fixture = Fixture::new("window-narrow");
    let app = axiomd_e2e::launch(&fixture.write(NAME, NOTES));

    // With room, the header draws all of it and the menu says nothing about history:
    // a menu repeating the buttons beside it would be saying the same thing twice.
    let wide = app.header();
    assert_eq!(wide.title, NAME);
    assert!(!wide.cut, "a 900px window cut the title off");
    assert_eq!(
        wide.controls,
        [OUTLINE, BACK, FORWARD, OPEN, EDIT, MENU, CLOSE],
        "a window with room is not drawing the whole header",
    );
    assert!(
        !offers_history(&app),
        "the menu is offering history the header is already showing",
    );

    for width in NARROW {
        app.resize(width, 700);
        laid_out_at(&app, width);

        // The whole of the strip is on screen: a header that cannot be drawn as narrow
        // as the window it is in is one the reader loses the end of off the edge.
        let laid_out = app.layout();
        assert!(
            laid_out.header.least <= laid_out.window.width,
            "at {width}px the header cannot be drawn narrower than {}px",
            laid_out.header.least,
        );

        let narrow = app.header();
        assert_eq!(
            narrow.controls,
            [OUTLINE, EDIT, MENU, CLOSE],
            "a {width}px header is still drawing controls it has no room for",
        );
        assert_eq!(narrow.title, NAME);
        assert!(
            !narrow.cut,
            "at {width}px the reader cannot read the whole of {NAME}",
        );

        // Nothing the reader could press has gone away: what left the strip is in the
        // menu, and the menu still shows how big the document is.
        let menu = app.menu();
        assert_eq!(
            menu.offers().into_iter().take(2).collect::<Vec<_>>(),
            [("Back", "win.back"), ("Forward", "win.forward")],
            "at {width}px the history the header gave up is not at the top of the menu",
        );
        assert!(
            menu.offers().contains(&("Open…", "app.open")),
            "the open button left the header and the menu does not offer opening",
        );
        assert_eq!(menu.zoom, "100%", "the menu stopped showing the zoom row");
    }

    // And with room again: the header takes its controls back and the menu stops
    // offering them.
    app.resize(900, 700);
    app.wait_for("the header to take its controls back", || {
        app.header().offers(BACK)
    });
    assert_eq!(app.header().controls, wide.controls);
    assert!(
        !offers_history(&app),
        "the menu kept the history a wide header is drawing again",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The demoted controls are controls, not decoration: in a narrow window the menu's
/// own Back takes the reader back through the document they followed a link from.
#[test]
fn the_history_the_menu_takes_over_still_walks_the_reader_back() {
    let fixture = Fixture::new("window-narrow-history");
    let first = fixture.write(NAME, NOTES);
    fixture.write(
        "second.md",
        "# Second\n\nThe document that was linked to.\n",
    );

    let app = axiomd_e2e::launch(&first);
    app.resize(400, 700);
    laid_out_at(&app, 400);
    assert!(
        !app.header().offers(BACK),
        "the header kept its history buttons in a 400px window",
    );

    app.click("a[href=\"second.md\"]");
    app.wait_until("document.querySelector('h1').textContent === 'Second'");

    // The action the menu's own item fires, which is all a menu item is.
    app.activate("win.back");
    app.wait_until("document.querySelector('h1').textContent === 'Release Notes'");
    assert_eq!(app.window_title(), NAME);

    app.activate("win.forward");
    app.wait_until("document.querySelector('h1').textContent === 'Second'");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The other half of the issue: a window opens where the reader left the last one.
#[test]
fn a_window_opens_at_the_size_the_reader_last_left_one() {
    let fixture = Fixture::new("window-geometry");
    let preferences = Preferences::new("window-geometry");
    let document = fixture.write(NAME, NOTES);

    let app = axiomd_e2e::launch_with(&document, &preferences);
    let first = app.layout().window;
    assert_eq!(
        (first.width, first.height),
        (900, 700),
        "a reader who has never resized a window did not get the usual one",
    );

    app.resize(760, 540);
    app.wait_for("the window to be the size it was dragged to", || {
        app.layout().window.width == 760
    });

    // Window state, not a preference: it is written down when the window goes, and no
    // dialog was involved (`ux_decisions.md`).
    app.close_window();
    preferences.wait_until("window-width", "760");
    preferences.wait_until("window-height", "540");
    assert!(app.close().is_empty(), "the launch left processes behind");

    let again = axiomd_e2e::launch_with(&document, &preferences);
    let reopened = again.layout().window;
    assert_eq!(
        (reopened.width, reopened.height),
        (760, 540),
        "the window did not open where the reader left the last one",
    );
    assert_eq!(
        again.dom_text("h1"),
        "Release Notes",
        "the restored window is not showing its document",
    );
    assert!(again.close().is_empty(), "the launch left processes behind");
}

/// The non-happy path: a reader who leaves a window maximized comes back to a
/// maximized window — and the size they had chosen before maximizing is still what
/// they get when they take it out of the screen's hands.
#[test]
fn a_maximized_window_comes_back_maximized_without_forgetting_its_size() {
    let fixture = Fixture::new("window-maximized");
    let preferences = Preferences::new("window-maximized");
    let document = fixture.write(NAME, NOTES);

    let app = axiomd_e2e::launch_with(&document, &preferences);
    app.resize(760, 540);
    app.wait_for("the window to be the size it was dragged to", || {
        app.layout().window.width == 760
    });

    app.maximize();
    app.wait_until_maximized(true);
    // The compositor answers a maximize when it is ready to, and the window is laid
    // out again after that.
    app.wait_for("the maximized window to fill the screen", || {
        app.layout().window.width > 760
    });

    app.close_window();
    preferences.wait_until("is-maximized", "true");
    // The screen's size is not the reader's choice: what they chose before maximizing
    // is what the window is when it stops filling the screen.
    preferences.wait_until("window-width", "760");
    preferences.wait_until("window-height", "540");
    assert!(app.close().is_empty(), "the launch left processes behind");

    let again = axiomd_e2e::launch_with(&document, &preferences);
    again.wait_until_maximized(true);
    assert_eq!(
        again.dom_text("h1"),
        "Release Notes",
        "the maximized window is not showing its document",
    );
    assert!(again.close().is_empty(), "the launch left processes behind");
}

/// Waits until the window really is `width` wide *and* has been laid out at it.
///
/// The document filling the window is the signal: below the breakpoint the outline
/// overlays the document rather than sitting beside it, so the surface the document is
/// on is the window's own width — and it is given that width by the same layout pass
/// that places the header bar and the title in it.
fn laid_out_at(app: &axiomd_e2e::App, width: i32) {
    app.wait_for(&format!("the window to be laid out at {width}px"), || {
        let laid_out = app.layout();
        laid_out.window.width == width && laid_out.document.width == width
    });
}

/// Whether the main menu is offering the history the header bar usually draws.
fn offers_history(app: &axiomd_e2e::App) -> bool {
    app.menu()
        .offers()
        .iter()
        .any(|(_, action)| *action == "win.back")
}
