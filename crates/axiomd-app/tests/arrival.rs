//! How a document arrives in front of the reader (issue #40).
//!
//! Opening a document used to show a black rectangle and then the document popping
//! into it. Two things are asserted here against the running application, and the
//! first is a photograph: the pane a document is about to appear in, taken while the
//! document's own bytes are held back at the origin that serves them, so the picture
//! is of the very frame the reader used to see black in. It has to be the colour the
//! page is painted, in a light desktop and in a dark one, and it has to follow a
//! theme change without the document being reloaded (invariant 9).
//!
//! The second is the appearance itself: the document fades up onto that page once,
//! when the page is loaded, and never again — a live reload is patched into the
//! article that is already there, which is the same reason it does not flash. A reader
//! whose desktop asks for less movement is shown the document at once instead, which
//! is what every other launch in this suite is: the harness pins
//! `gtk-enable-animations` off so a screenshot can never catch a widget mid-transition.

use std::path::Path;

use axiomd_e2e::{App, Fixture, Preferences, Screenshot};

const NOTES: &str = "\
# Reading

A paragraph long enough to fill a line of the page it is read on.

## Details

A second section, so the document is more than a title.
";

const CHANGED: &str = "\
# Reading

A paragraph the reader did not write, arriving under them.

## Details

A second section, so the document is more than a title.
";

/// The colours the bundled stylesheet paints a page in, light and dark. The same two
/// numbers `appearance.rs` and `preferences.rs` hold the rendered document to.
const PAGE_LIGHT: (u8, u8, u8) = (255, 255, 255);
const PAGE_DARK: (u8, u8, u8) = (29, 29, 32);

/// What the bug looked like: WebKit's own background, with nothing said about it.
const BLACK: (u8, u8, u8) = (0, 0, 0);

/// The animation a document makes the first time it appears, by the name the
/// stylesheet gives it.
const APPEARANCE: &str = "getComputedStyle(document.querySelector('.markdown')).animationName";

/// Fails unless nearly every pixel of `pane` is `colour`.
///
/// Nearly, rather than every: a pane is a rectangle of one colour with nothing in it,
/// but it is drawn on a compositor and its very edge belongs to the widgets around it.
fn assert_painted(pane: &Screenshot, colour: (u8, u8, u8), what: &str) {
    let (width, height) = pane.size();
    let pixels = usize::try_from(width * height).expect("a pane of pixels");
    assert!(pixels > 0, "{what}: the pane has no pixels at all");
    let painted = pane.pixels_coloured(colour);
    assert!(
        painted * 100 >= pixels * 99,
        "{what}: {painted} of {pixels} pixels are {colour:?}",
    );
}

/// Fails unless nothing in `pane` is black.
fn assert_not_black(pane: &Screenshot, what: &str) {
    assert_eq!(
        pane.pixels_coloured(BLACK),
        0,
        "{what}: the reader was shown a black frame",
    );
}

/// The pane a document is about to arrive in, on a light desktop, is the light page.
#[test]
fn the_pane_a_document_arrives_in_is_the_light_page_and_never_black() {
    let fixture = Fixture::new("arrival-light");
    let desktop = Preferences::new("arrival-light");
    let app = axiomd_e2e::launch_without_document_with(&desktop);
    let notes = fixture.write("notes.md", NOTES);

    app.open_to_the_empty_pane(&notes);
    let pane = app.pane_screenshot();
    assert_not_black(&pane, "a document opening on a light desktop");
    assert_painted(&pane, PAGE_LIGHT, "a document opening on a light desktop");

    // And the document that then arrives is painted the very colour the pane was, so
    // what the reader saw first was the page and not a stand-in for it.
    app.let_the_document_arrive();
    assert_eq!(
        app.dom("getComputedStyle(document.body).backgroundColor"),
        "rgb(255, 255, 255)",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// And on a dark desktop it is the dark page — the palette the reader is reading in,
/// not a lighter approximation of it and not black.
#[test]
fn the_pane_a_document_arrives_in_is_the_dark_page_on_a_dark_desktop() {
    let fixture = Fixture::new("arrival-dark");
    let desktop = Preferences::with("arrival-dark", "theme", "'dark'");
    let app = axiomd_e2e::launch_without_document_with(&desktop);
    let notes = fixture.write("notes.md", NOTES);

    app.open_to_the_empty_pane(&notes);
    let pane = app.pane_screenshot();
    assert_not_black(&pane, "a document opening on a dark desktop");
    assert_painted(&pane, PAGE_DARK, "a document opening on a dark desktop");

    app.let_the_document_arrive();
    assert_eq!(
        app.dom("getComputedStyle(document.body).backgroundColor"),
        "rgb(29, 29, 32)",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A reader who changes theme while a document is opening: the pane repaints under
/// them, and nothing is loaded or rendered to do it (invariant 9, issue #10).
///
/// The flip is done in the one moment the pane's own colour is all there is to see, so
/// this asserts the pane rather than the page drawn over it.
#[test]
fn a_theme_change_repaints_the_pane_without_loading_or_rendering_anything() {
    let fixture = Fixture::new("arrival-flip");
    let desktop = Preferences::new("arrival-flip");
    let app = axiomd_e2e::launch_without_document_with(&desktop);
    let notes = fixture.write("notes.md", NOTES);

    app.open_to_the_empty_pane(&notes);
    assert_painted(
        &app.pane_screenshot(),
        PAGE_LIGHT,
        "the desktop this launch pinned is a light one",
    );

    let loads = app.navigation_count();
    let pages = app.render_count();

    app.activate("app.preferences");
    app.set_preference("Theme", "Dark");
    desktop.wait_until("theme", "'dark'");
    app.wait_for("the pane to be repainted dark", || {
        let pane = app.pane_screenshot();
        let (width, height) = pane.size();
        let pixels = usize::try_from(width * height).expect("a pane of pixels");
        pane.pixels_coloured(PAGE_DARK) * 100 >= pixels * 99
    });

    assert_not_black(&app.pane_screenshot(), "the pane after a theme change");
    assert_eq!(app.navigation_count(), loads, "the view was sent somewhere");
    assert_eq!(app.render_count(), pages, "the document was rendered again");

    app.let_the_document_arrive();
    assert_eq!(
        app.dom("getComputedStyle(document.body).backgroundColor"),
        "rgb(29, 29, 32)",
        "the document arrived in a colour the pane was not",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The document appears rather than snapping in — once, on the load that put it there.
///
/// Everything that happens to a document afterwards happens to the article that is
/// already on screen: a live reload and a re-render are patched into it, a mode switch
/// puts the surface it is on back in front of the reader, and a theme change restyles
/// it where it stands. None of them is a load, so none of them may run the appearance
/// again — the same animation is still the one that started when the page loaded, at
/// the same moment, and the reader is never shown their document dissolving because a
/// file changed or because they went dark.
#[test]
fn a_document_appears_once_and_everything_after_it_is_effect_free() {
    let fixture = Fixture::new("arrival-fade");
    let desktop = Preferences::new("arrival-fade").animating();
    let notes = fixture.write("notes.md", NOTES);
    let app = axiomd_e2e::launch_with(&notes, &desktop);

    assert_eq!(
        app.dom(APPEARANCE),
        "axiomd-appear",
        "a document was put on screen without appearing at all",
    );
    // ~150–200 ms: long enough to be an arrival, short enough that nobody is waiting.
    let duration: f64 = app
        .dom("String(parseFloat(getComputedStyle(document.querySelector('.markdown')).animationDuration))")
        .parse()
        .expect("an animation duration in seconds");
    assert!(
        (0.15..=0.2).contains(&duration),
        "a document takes {duration}s to appear",
    );

    let loads = app.navigation_count();
    let started = appearance_started(&app);

    // The file changes under the reader.
    save(&notes, CHANGED);
    app.wait_until("document.body.textContent.includes('did not write')");
    assert_eq!(
        appearance_started(&app),
        started,
        "the document faded in again when the file changed under the reader",
    );

    // The reader goes to the source and comes back.
    app.activate("win.mode");
    app.wait_until_mode("edit");
    app.activate("win.mode");
    app.wait_until_mode("read");
    assert_eq!(
        appearance_started(&app),
        started,
        "the document faded in again on the way back from the editor",
    );

    // And the reader goes dark, which restyles the page where it stands (issue #10)
    // and re-installs the very stylesheet the appearance is declared in.
    app.activate("app.preferences");
    app.set_preference("Theme", "Dark");
    app.wait_until("getComputedStyle(document.body).backgroundColor === 'rgb(29, 29, 32)'");
    assert_eq!(
        appearance_started(&app),
        started,
        "the document faded in again when the reader changed theme",
    );

    assert_eq!(app.navigation_count(), loads, "the document was reloaded");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A reader whose desktop asks for less movement is shown the document at once.
///
/// This is every other launch in the suite: the harness pins `gtk-enable-animations`
/// off, which is exactly what a reader asking their desktop to reduce animation sets.
/// WebKitGTK does not answer `prefers-reduced-motion` from it (probed on WebKitGTK
/// 2.52.5 — the query stayed false with the setting off), so the application says it
/// through the reader's own stylesheet, and this is the proof that it reaches.
#[test]
fn a_desktop_asking_for_less_movement_is_shown_the_document_at_once() {
    let fixture = Fixture::new("arrival-still");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", NOTES));

    assert_eq!(
        app.dom(APPEARANCE),
        "none",
        "a desktop asking for less movement was given a fade",
    );
    assert_eq!(
        app.dom("getComputedStyle(document.querySelector('.markdown')).opacity"),
        "1",
        "the document was left part-way through an appearance it was never given",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// When the article's one animation began, as the page counts time. A document that
/// faded in a second time would have started a second one, at a later moment.
fn appearance_started(app: &App) -> String {
    assert_eq!(
        app.dom("String(document.querySelector('.markdown').getAnimations().length)"),
        "1",
        "the article is not carrying exactly one appearance",
    );
    app.dom("String(document.querySelector('.markdown').getAnimations()[0].startTime)")
}

/// Saves `contents` over `document` the way a plain editor does: in place.
fn save(document: &Path, contents: &str) {
    std::fs::write(document, contents).unwrap_or_else(|error| panic!("save {document:?}: {error}"));
}
