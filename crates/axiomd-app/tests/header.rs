//! Issue #28: the header bar's mode switch says where pressing it goes.
//!
//! A stateful affordance whose presentation never changes is a defect
//! (`ux_decisions.md`): the switch showed a pencil in both modes, so a reader in the
//! source could not tell from it which of the two they were in, and a screen reader
//! offered to take them editing while they were already there.
//!
//! Everything here is read from what the reader would be looking at — the icon the
//! theme resolved, the words hovering shows, the name a screen reader announces — and
//! driven through `win.mode`, which is the action `Ctrl+E`, the menu item and the
//! button itself all fire.

use axiomd_e2e::Fixture;

const NOTES: &str = "# Release Notes\n\nThe first paragraph.\n";

/// The switch offers the mode the reader is *not* in, both ways round and back again.
///
/// The icon, the tooltip and the announced name together: a control drawn as one mode
/// while it announces the other would be a defect this test would miss if it asked
/// about only one of them.
#[test]
fn the_mode_switch_offers_the_mode_the_reader_is_not_in() {
    let fixture = Fixture::new("mode-switch");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", NOTES));

    let reading = app.mode_switch();
    assert_eq!(
        (
            reading.icon.as_str(),
            reading.tooltip.as_str(),
            reading.announced.as_str(),
            reading.pressed
        ),
        (
            "document-edit-symbolic",
            "Edit the source (Ctrl+E)",
            "Edit the source",
            false,
        ),
        "reading, the switch does not offer the source",
    );

    // Ctrl+E.
    app.activate("win.mode");
    app.wait_until_mode("edit");

    let editing = app.mode_switch();
    assert_eq!(
        (
            editing.icon.as_str(),
            editing.tooltip.as_str(),
            editing.announced.as_str(),
            editing.pressed
        ),
        (
            "view-reveal-symbolic",
            "View the document (Ctrl+E)",
            "View the document",
            true,
        ),
        "editing, the switch does not offer the document",
    );

    // And back: a switch that only changes one way is one that gets stuck.
    app.activate("win.mode");
    app.wait_until_mode("read");
    assert_eq!(
        app.mode_switch(),
        reading,
        "coming back to the document left the switch offering it",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A window that opens with nothing in it starts in edit mode (`ux_decisions.md`), and
/// the switch has to have been drawn for that before the reader ever presses anything.
#[test]
fn a_window_that_opens_editing_starts_with_the_switch_offering_the_document() {
    let app = axiomd_e2e::launch_without_document();
    app.wait_until_mode("edit");

    let switch = app.mode_switch();
    assert_eq!(switch.icon, "view-reveal-symbolic");
    assert_eq!(switch.announced, "View the document");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The header as pixels, captured through the same path a golden is compared through.
///
/// Not ignored, unlike the goldens below: it says the capture is of a real header bar
/// rather than of a blank strip, which is what a picture nobody has pinned yet cannot
/// say for itself.
#[test]
fn the_header_bar_can_be_captured_as_pixels() {
    let fixture = Fixture::new("header-capture");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", NOTES));

    let captured = app.header_screenshot();
    let (width, height) = captured.size();

    // The header spans the window, which opens 900px wide, and is one control tall.
    assert!(
        width >= 800 && (20..200).contains(&height),
        "the captured header is {width}x{height}; the window is 900x700",
    );
    assert!(
        !captured.is_blank(),
        "the capture is a single colour, so no controls were drawn",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The visual specification of the header while the reader is reading.
///
/// Ignored until a human has looked at the picture and pinned it: approving a surface
/// for the first time is theirs to do, not the harness's (`docs/TESTING.md`). To pin
/// it, look at `target/debug/e2e-artifacts/header-reading.actual.png` from a failing
/// run and, if it is right, re-run this test with `AXIOMD_PIN_GOLDENS=1` set, then
/// remove the `#[ignore]`.
#[test]
#[ignore = "awaiting the first human visual approval; see the comment above"]
fn the_header_while_reading_still_looks_the_way_it_was_approved() {
    let fixture = Fixture::new("header-golden-reading");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", NOTES));

    app.header_screenshot().assert_matches("header-reading");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The same header with the window in the source — the other state, and the one the
/// pencil used to be shown in as well.
///
/// Pinned the same way, from `header-editing.actual.png`.
#[test]
#[ignore = "awaiting the first human visual approval; see the comment above"]
fn the_header_while_editing_still_looks_the_way_it_was_approved() {
    let fixture = Fixture::new("header-golden-editing");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", NOTES));

    app.activate("win.mode");
    app.wait_until_mode("edit");
    app.header_screenshot().assert_matches("header-editing");

    assert!(app.close().is_empty(), "the launch left processes behind");
}
