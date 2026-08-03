//! How a document looks while the reader is reading it, asserted against the running
//! application (issue #10, UT-008 and UT-011).
//!
//! Two things change it without the reader touching the document: the desktop's
//! accessibility setting, and zoom. Both are asserted the same way — computed styles
//! read out of the page in front of the reader, and the words the primary menu shows —
//! with the view's load count and the window's page count beside every one of them.
//! Neither may move: restyling and rescaling a document must cost no re-parse and no
//! reload (invariant 9), which is the whole of what separates axiomd from the app it
//! exists because of.
//!
//! The in-app light/dark override lives in `preferences.rs`, with the other rows of
//! the dialog it is turned in.

use axiomd_e2e::{Fixture, Preferences};

/// A document with the three things high contrast has to reach: prose with a link in
/// it, a table with borders, and a code block with a palette.
const NOTES: &str = "\
# Reading

A paragraph with [a link](https://example.com/) in it, long enough that the measure
the reader is held to is the thing deciding where it wraps.

| Option | Meaning |
| ------ | ------- |
| light  | bright  |
| dark   | dim     |

```rust
fn main() {
    println!(\"hello\");
}
```
";

/// The colour of the rule around a table cell — a 12%-alpha suggestion under the
/// ordinary palette and a line that is actually there under high contrast.
const CELL_BORDER: &str = "getComputedStyle(document.querySelector('table td')).borderTopColor";

/// What a link's underline is drawn in. Quiet under the ordinary palette — a 40%
/// wash of the link's own colour — and at full strength under high contrast, which is
/// the difference between an underline a reader can see and one they cannot.
const LINK_RULE: &str =
    "getComputedStyle(document.querySelector('.markdown p a')).textDecorationColor";

/// The colour the reader is reading on.
const PAGE_COLOUR: &str = "getComputedStyle(document.body).backgroundColor";

/// The colour of the words.
const INK: &str = "getComputedStyle(document.body).color";

/// How much bigger than a CSS pixel a device pixel is — which is exactly what a zoom
/// level is, as the page sees it.
const SCALE: &str = "window.devicePixelRatio";

/// How wide the page believes it is, in CSS pixels. Zoom is a relayout and not a
/// magnifying glass, so this has to shrink as the document grows.
const PAGE_WIDTH: &str = "document.documentElement.clientWidth";

/// The desktop asking for high contrast while the reader is reading: the document
/// repaints where it stands, and is neither loaded nor rendered again.
///
/// High contrast is not the dark palette — this launch is on a light desktop
/// throughout — and it is not a preference: the reader said it once, to their desktop.
#[test]
fn high_contrast_repaints_the_document_the_reader_is_looking_at() {
    let fixture = Fixture::new("contrast-live");
    let desktop = Preferences::new("contrast-live");
    let app = axiomd_e2e::launch_with(&fixture.write("notes.md", NOTES), &desktop);

    // The ordinary palette: a suggestion of a border, and a link told apart by colour.
    assert_eq!(app.dom(CELL_BORDER), "rgba(0, 0, 6, 0.12)");
    assert_eq!(
        app.dom(LINK_RULE),
        "color(srgb 0.105882 0.415686 0.796078 / 0.4)"
    );
    assert_eq!(app.dom(PAGE_COLOUR), "rgb(255, 255, 255)");

    let loads = app.navigation_count();
    let pages = app.render_count();

    desktop.set_high_contrast(true);
    app.wait_until(&format!("{CELL_BORDER} === 'rgb(0, 0, 0)'"));

    assert_eq!(
        app.dom(LINK_RULE),
        "rgb(0, 0, 204)",
        "high contrast left links underlined in a wash of their own colour",
    );
    assert_eq!(
        app.dom(INK),
        "rgb(0, 0, 0)",
        "high contrast left the words at four fifths of full strength",
    );
    assert_eq!(
        app.dom(PAGE_COLOUR),
        "rgb(255, 255, 255)",
        "high contrast turned a light desktop dark",
    );

    assert_eq!(app.navigation_count(), loads, "the document was reloaded");
    assert_eq!(app.render_count(), pages, "the document was rendered again");

    // And back: a reader who turns it off is reading the ordinary document again.
    desktop.set_high_contrast(false);
    app.wait_until(&format!("{CELL_BORDER} === 'rgba(0, 0, 6, 0.12)'"));
    assert_eq!(
        app.dom(LINK_RULE),
        "color(srgb 0.105882 0.415686 0.796078 / 0.4)"
    );
    assert_eq!(app.navigation_count(), loads, "the document was reloaded");
    assert_eq!(app.render_count(), pages, "the document was rendered again");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The combination the document's own stylesheet would otherwise win: a reader in
/// high contrast *and* in dark mode. The dark palette's 15%-alpha borders and
/// 90%-alpha ink are exactly what the setting exists to remove, so the desktop's
/// answer has to outrank them.
#[test]
fn high_contrast_outranks_the_dark_palette_rather_than_being_it() {
    let fixture = Fixture::new("contrast-dark");
    let desktop = Preferences::new("contrast-dark");
    let app = axiomd_e2e::launch_with(&fixture.write("notes.md", NOTES), &desktop);

    app.activate("app.preferences");
    app.set_preference("Theme", "Dark");
    app.wait_until(&format!("{PAGE_COLOUR} === 'rgb(29, 29, 32)'"));

    let loads = app.navigation_count();
    let pages = app.render_count();

    desktop.set_high_contrast(true);
    app.wait_until(&format!("{PAGE_COLOUR} === 'rgb(0, 0, 0)'"));

    assert_eq!(
        app.dom(INK),
        "rgb(255, 255, 255)",
        "a dark high-contrast document kept the dark palette's dimmed ink",
    );
    assert_eq!(
        app.dom(CELL_BORDER),
        "rgb(255, 255, 255)",
        "a dark high-contrast document kept the dark palette's suggested borders",
    );
    assert_eq!(app.dom(LINK_RULE), "rgb(140, 180, 255)");

    assert_eq!(app.navigation_count(), loads, "the document was reloaded");
    assert_eq!(app.render_count(), pages, "the document was rendered again");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A reader whose desktop was already in high contrast before axiomd started sees the
/// document that way from its first frame — never the ordinary one first.
#[test]
fn a_document_opened_on_a_high_contrast_desktop_arrives_in_high_contrast() {
    let fixture = Fixture::new("contrast-first");
    let desktop = Preferences::new("contrast-first");
    desktop.set_high_contrast(true);

    let app = axiomd_e2e::launch_with(&fixture.write("notes.md", NOTES), &desktop);

    assert_eq!(app.dom(CELL_BORDER), "rgb(0, 0, 0)");
    assert_eq!(app.dom(LINK_RULE), "rgb(0, 0, 204)");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Zoom, walked up and down its ladder and reset — UT-011's three keys, which are the
/// three actions the menu row is bound to.
#[test]
fn zoom_steps_the_document_up_and_down_and_ctrl_zero_restores_it() {
    let fixture = Fixture::new("zoom-steps");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", NOTES));

    assert_eq!(app.zoom(), "100%", "a window did not open at full size");
    assert_eq!(app.dom(SCALE), "1");
    let full_width: f64 = app.dom(PAGE_WIDTH).parse().expect("a page width");

    let loads = app.navigation_count();
    let pages = app.render_count();

    // Ctrl+plus, twice: 110% and then 125%.
    app.activate("win.zoom-in");
    app.wait_until_zoom("110%");
    app.activate("win.zoom-in");
    app.wait_until_zoom("125%");

    assert_eq!(app.dom(SCALE), "1.25");
    let zoomed: f64 = app.dom(PAGE_WIDTH).parse().expect("a page width");
    assert!(
        (zoomed - full_width / 1.25).abs() <= 1.0,
        "the document was magnified rather than relaid out: {full_width} CSS pixels \
         wide at 100% and {zoomed} at 125%",
    );

    // Ctrl+minus goes back down the same ladder.
    app.activate("win.zoom-out");
    app.wait_until_zoom("110%");

    // Ctrl+0 is the whole way back, from wherever the reader is.
    app.activate("win.zoom-reset");
    app.wait_until_zoom("100%");
    assert_eq!(app.dom(SCALE), "1");
    assert_eq!(
        app.dom(PAGE_WIDTH),
        full_width.to_string(),
        "the document did not come back to the width it started at",
    );

    assert_eq!(app.navigation_count(), loads, "the document was reloaded");
    assert_eq!(app.render_count(), pages, "the document was rendered again");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The ends of the ladder. Asking for more at 200% or less at 50% leaves the document
/// where it is — and the reader can see why, because the step that would leave the
/// ladder stops being offered.
#[test]
fn zoom_stops_at_half_size_and_at_double() {
    let fixture = Fixture::new("zoom-bounds");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", NOTES));

    for _ in 0..12 {
        app.activate("win.zoom-in");
    }
    app.wait_until_zoom("200%");
    assert_eq!(app.dom(SCALE), "2");
    // The wheel is the other way in, and it stops at the same place.
    app.ctrl_scroll(-1.0);
    app.wait_until_zoom("200%");

    for _ in 0..12 {
        app.activate("win.zoom-out");
    }
    app.wait_until_zoom("50%");
    assert_eq!(app.dom(SCALE), "0.5");
    app.ctrl_scroll(1.0);
    app.wait_until_zoom("50%");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The wheel: `Ctrl` and a turn resizes the document, and a turn on its own is the
/// reader reading. A viewer that zoomed on every scroll would be unusable.
#[test]
fn ctrl_and_the_wheel_zoom_where_the_wheel_alone_reads() {
    let fixture = Fixture::new("zoom-wheel");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", NOTES));

    app.ctrl_scroll(-1.0);
    app.wait_until_zoom("110%");
    app.ctrl_scroll(-1.0);
    app.wait_until_zoom("125%");
    app.ctrl_scroll(1.0);
    app.wait_until_zoom("110%");

    // Each way on its own, because one of each would cancel out and hide exactly the
    // bug this asserts: the wheel alone moves the reader through the document and
    // leaves its size alone.
    app.scroll(-1.0);
    assert_eq!(
        app.zoom(),
        "110%",
        "scrolling towards the top of the document made it bigger",
    );
    app.scroll(1.0);
    assert_eq!(
        app.zoom(),
        "110%",
        "scrolling towards the end of the document made it smaller",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A pinch on a touchpad, which is the same ladder reached by spreading two fingers:
/// a step once the gesture has travelled far enough, and nothing at all while a
/// resting hand wobbles.
#[test]
fn a_pinch_steps_the_document_once_it_has_travelled() {
    let fixture = Fixture::new("zoom-pinch");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", NOTES));

    // A hand that has barely moved.
    app.pinch(1.05);
    assert_eq!(app.zoom(), "100%", "a wobble resized the document");

    app.pinch(1.25);
    app.wait_until_zoom("110%");
    app.pinch(1.6);
    app.wait_until_zoom("125%");

    // And back the other way, measured from where the last step was taken.
    app.pinch(1.2);
    app.wait_until_zoom("110%");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Zoom is the reader's way of looking at this window, so it survives being given
/// another document — and it belongs to this window alone, so the next one opens at
/// full size (UT-011: per window, for the session).
#[test]
fn zoom_belongs_to_the_window_and_outlives_the_document_in_it() {
    let fixture = Fixture::new("zoom-window");
    let notes = fixture.write("notes.md", NOTES);
    let more = fixture.write("more.md", NOTES);

    let app = axiomd_e2e::launch(&notes);
    app.activate("win.zoom-in");
    app.activate("win.zoom-in");
    app.wait_until_zoom("125%");

    // The same window, another document — the file chooser's own path.
    app.open_here(&more);
    app.wait_until(&format!("{SCALE} === 1.25"));
    assert_eq!(app.zoom(), "125%", "a new document undid the reader's zoom");

    // A window of its own is a way of looking of its own.
    app.open(&notes);
    app.wait_until_windows(2);
    assert_eq!(
        app.zoom(),
        "100%",
        "a new window inherited another window's zoom",
    );
    assert_eq!(app.dom(SCALE), "1");

    app.select_window(0);
    assert_eq!(app.zoom(), "125%", "the first window lost its zoom");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The visual specification of a document on a light desktop.
///
/// Ignored until a human has looked at the picture and pinned it: approving a rendered
/// surface for the first time is theirs to do, not the harness's (`docs/TESTING.md`).
/// To pin it, look at `target/debug/e2e-artifacts/appearance-light.actual.png` from a
/// failing run and, if it is right, re-run this test with `AXIOMD_PIN_GOLDENS=1` set,
/// then remove the `#[ignore]`.
#[test]
#[ignore = "awaiting the first human visual approval; see the comment above"]
fn a_light_document_still_looks_the_way_it_was_approved() {
    let fixture = Fixture::new("golden-light");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", NOTES));

    assert_eq!(app.dom(PAGE_COLOUR), "rgb(255, 255, 255)");
    app.screenshot().assert_matches("appearance-light");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The same document on a dark one — the other half of UT-008's exit criterion, and
/// the picture that says the code palette went with it rather than staying light.
///
/// Pinned the same way, from `appearance-dark.actual.png`.
#[test]
#[ignore = "awaiting the first human visual approval; see the comment above"]
fn a_dark_document_still_looks_the_way_it_was_approved() {
    let fixture = Fixture::new("golden-dark");
    let dark = Preferences::with("golden-dark", "theme", "'dark'");
    let app = axiomd_e2e::launch_with(&fixture.write("notes.md", NOTES), &dark);

    assert_eq!(app.dom(PAGE_COLOUR), "rgb(29, 29, 32)");
    app.screenshot().assert_matches("appearance-dark");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// And on a desktop asking for high contrast, which is the surface this issue adds and
/// the one no palette test can show is *legible* rather than merely different.
///
/// Pinned the same way, from `appearance-high-contrast.actual.png`.
#[test]
#[ignore = "awaiting the first human visual approval; see the comment above"]
fn a_high_contrast_document_still_looks_the_way_it_was_approved() {
    let fixture = Fixture::new("golden-contrast");
    let desktop = Preferences::new("golden-contrast");
    desktop.set_high_contrast(true);
    let app = axiomd_e2e::launch_with(&fixture.write("notes.md", NOTES), &desktop);

    assert_eq!(app.dom(CELL_BORDER), "rgb(0, 0, 0)");
    app.screenshot().assert_matches("appearance-high-contrast");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The level in the menu is a button and not a caption: pressing it is `Ctrl+0`. An
/// affordance that renders and does nothing is a defect.
#[test]
fn the_zoom_the_menu_shows_is_the_button_that_resets_it() {
    let fixture = Fixture::new("zoom-indicator");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", NOTES));

    app.activate("win.zoom-in");
    app.activate("win.zoom-in");
    app.activate("win.zoom-in");
    app.wait_until_zoom("150%");

    app.press("150%");
    app.wait_until_zoom("100%");
    assert_eq!(app.dom(SCALE), "1");

    assert!(app.close().is_empty(), "the launch left processes behind");
}
