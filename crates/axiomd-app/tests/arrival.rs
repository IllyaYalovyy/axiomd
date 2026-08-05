//! How a document arrives in front of the reader (issues #40 and #41).
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
//!
//! # The frame the harness cannot photograph, and what is asserted instead
//!
//! Both of those are asked of WebKit: the picture is the web process's own answer about
//! its own page, and the colour it answers with is the one the view was told to paint.
//! The frame the owner still saw black after #40 is the one *before* the web process has
//! any answer at all — on the accelerated compositing path the pane is black until its
//! first composited frame, and the software harness renders through llvmpipe where that
//! path is never taken (issue #41). So what is asserted here is the structure that makes
//! the frame unreachable rather than the frame itself: the webview is not the thing on
//! screen until the page in it has reported, through the app's own bridge, that it has
//! been drawn — and what stands in front of it until then is the page's own colour, in
//! the window's own scene, photographed as the compositor is handed it.

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

/// Fails unless `frame` is the page the reader reads on, whatever is written on it.
///
/// The assertion the frames of a document arriving are held to (issue #41). Not "is not
/// black": these pictures are taken from the window's own scene, where a webview with no
/// frame to show draws nothing at all and reads back as transparent — which is zero in
/// every channel and would be counted as black by a picture that has no alpha in it. So
/// what is asserted is the thing that is true of every frame the reader may be shown and
/// false of every frame they may not: it is the page. A frame that is anything else —
/// black on the accelerated path, the window's own grey where nothing was drawn — counts
/// none of it.
///
/// Nine tenths rather than all of it, because a page with a document on it has the
/// document's own ink on it too, and the fixture below covers a few percent of the pane
/// with words.
fn assert_is_the_page(frame: &Screenshot, colour: (u8, u8, u8), what: &str) {
    let (width, height) = frame.size();
    let pixels = usize::try_from(width * height).expect("a pane of pixels");
    assert!(pixels > 0, "{what}: the pane has no pixels at all");
    let page = frame.pixels_coloured(colour);
    assert!(
        page * 10 >= pixels * 9,
        "{what}: only {page} of {pixels} pixels are the page {colour:?}",
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
    // And so does the page the document is arriving on, which is what the reader is
    // actually looking at in this moment (issue #41): it is painted from the same one
    // answer, so going dark repaints it rather than leaving a light rectangle standing
    // in front of a dark document.
    assert_painted(
        &app.presented_pane(),
        PAGE_DARK,
        "the page a document is arriving on, after a theme change",
    );
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

/// The webview is not what the reader is looking at until the page in it has been
/// drawn — the structural half of issue #41, and the half that holds on any compositor.
///
/// Asserted twice over: what the window says it is presenting, and what it presents.
/// The picture is taken from the window's own scene rather than from WebKit, so it is
/// the pixels a compositor is handed — the ones that were black on the owner's desktop
/// while WebKit was answering, quite truthfully, that its page is white.
#[test]
fn the_webview_is_not_on_screen_until_the_page_in_it_has_been_drawn() {
    let fixture = Fixture::new("arrival-first-frame");
    let desktop = Preferences::new("arrival-first-frame");
    let app = axiomd_e2e::launch_without_document_with(&desktop);
    let notes = fixture.write("notes.md", NOTES);

    app.open_to_the_empty_pane(&notes);
    assert_eq!(
        app.pane_showing(),
        "placeholder",
        "the webview was put on screen before it had a frame of the document to show",
    );
    assert_painted(
        &app.presented_pane(),
        PAGE_LIGHT,
        "the pane a document is arriving in",
    );

    // And it is the document itself that then takes its place, rather than the reader
    // being left in front of the page it arrives on.
    app.let_the_document_arrive();
    app.wait_until_pane_shows("document");
    assert_eq!(app.dom_text("h1"), "Reading");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// And on a dark desktop the page it arrives on is the dark page: the surface in front
/// of the webview is painted from the same one answer the document itself is
/// (`axiomd_render::Reading`), so it cannot be a colour the page is not.
#[test]
fn the_page_a_document_arrives_on_is_the_dark_page_on_a_dark_desktop() {
    let fixture = Fixture::new("arrival-first-frame-dark");
    let desktop = Preferences::with("arrival-first-frame-dark", "theme", "'dark'");
    let app = axiomd_e2e::launch_without_document_with(&desktop);
    let notes = fixture.write("notes.md", NOTES);

    app.open_to_the_empty_pane(&notes);
    assert_eq!(app.pane_showing(), "placeholder");
    assert_painted(
        &app.presented_pane(),
        PAGE_DARK,
        "the pane a document is arriving in on a dark desktop",
    );

    app.let_the_document_arrive();
    app.wait_until_pane_shows("document");
    assert_eq!(app.dom_text("h1"), "Reading");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Every frame between the window having a pane and the document being in it is the
/// page the reader reads on — photographed one after another, as fast as the window will
/// draw them.
///
/// The pictures are of the window's own scene, so this is the sequence a compositor is
/// handed: the frame issue #41 is about would be in it. It is taken with the document
/// deliberately held back at the origin that serves it and then released mid-loop, which
/// is what makes the moment worth photographing last longer than a frame — held, the
/// window really has its document surface on screen and WebKit really has been sent to
/// the document's URI with nothing answering.
#[test]
fn every_frame_from_the_empty_pane_to_the_document_is_the_page() {
    let fixture = Fixture::new("arrival-frames");
    let desktop = Preferences::new("arrival-frames");
    let app = axiomd_e2e::launch_without_document_with(&desktop);
    let notes = fixture.write("notes.md", NOTES);

    app.open_to_the_empty_pane(&notes);
    app.answer_the_held_pages();

    let frames = std::cell::Cell::new(0u32);
    app.wait_for("the document to be in the pane", || {
        assert_is_the_page(
            &app.presented_pane(),
            PAGE_LIGHT,
            &format!("frame {} of a document arriving", frames.get()),
        );
        frames.set(frames.get() + 1);
        app.pane_showing() == "document"
    });
    assert!(
        frames.get() > 1,
        "only {} frame was photographed, which is not a sequence",
        frames.get(),
    );

    // And on past the swap: the document is on the page it arrived on, and the swap
    // itself put nothing in between.
    app.wait_until("document.querySelector('.markdown') !== null");
    for frame in 0..5 {
        assert_is_the_page(
            &app.presented_pane(),
            PAGE_LIGHT,
            &format!("frame {frame} of the document just arrived"),
        );
    }

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A document loaded while the reader is somewhere else is still shown to them when
/// they arrive — the page it loaded behind does not become a page they are stuck on.
///
/// The edge the design has to answer for (issue #41): a webview that is not on screen
/// has no rendering updates, so a document loaded while the reader is in the editor
/// cannot report a frame at all. What it must not do is stay unreported once they come
/// back. A bare launch is exactly that reader — it opens on an untitled document in the
/// editor — and switching to reading is the moment the pane has to catch up.
#[test]
fn a_document_that_loaded_while_the_reader_was_editing_is_shown_when_they_come_back() {
    let app = axiomd_e2e::launch_without_document();

    app.activate("win.mode");
    app.wait_until_mode("read");
    app.wait_until_pane_shows("document");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Nothing that happens to a document already on screen brings that page back.
///
/// The reader's file changing under them, their going dark, and their going to the
/// source and back are patches and restyles of the page that is already drawn — none of
/// them is a load, so none of them leaves the webview with nothing to show. A
/// placeholder that came back for any of them would be a flash of its own, and the
/// navigation count is asserted beside it because a load is the one thing that would
/// legitimately have brought it back.
#[test]
fn nothing_after_the_document_has_arrived_puts_the_page_back_in_front_of_it() {
    let fixture = Fixture::new("arrival-stays");
    let desktop = Preferences::new("arrival-stays");
    let notes = fixture.write("notes.md", NOTES);
    let app = axiomd_e2e::launch_with(&notes, &desktop);

    app.wait_until_pane_shows("document");
    let loads = app.navigation_count();

    save(&notes, CHANGED);
    app.wait_until("document.body.textContent.includes('did not write')");
    assert_eq!(
        app.pane_showing(),
        "document",
        "a file changing under the reader put the page back in front of their document",
    );

    app.activate("win.mode");
    app.wait_until_mode("edit");
    app.activate("win.mode");
    app.wait_until_mode("read");
    assert_eq!(
        app.pane_showing(),
        "document",
        "coming back from the editor put the page back in front of the document",
    );

    app.activate("app.preferences");
    app.set_preference("Theme", "Dark");
    app.wait_until("getComputedStyle(document.body).backgroundColor === 'rgb(29, 29, 32)'");
    assert_eq!(
        app.pane_showing(),
        "document",
        "going dark put the page back in front of the document",
    );

    assert_eq!(app.navigation_count(), loads, "the document was reloaded");

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
