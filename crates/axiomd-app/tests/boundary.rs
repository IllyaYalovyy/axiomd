//! Issue #47: the chrome above the document separates from it with Adwaita's raised
//! shadow rather than a hard edge.
//!
//! The owner's ruling came from a side-by-side with another viewer on the same
//! document: where axiomd's header met the page, the page simply started. What replaces
//! that is `AdwToolbarView`'s `ADW_TOOLBAR_RAISED` — "opaque background with a
//! persistent shadow" in libadwaita 1.8's own words, against
//! `ADW_TOOLBAR_RAISED_BORDER`, which is the same thing "with the shadow replaced with a
//! more subtle border" and so is the treatment being replaced rather than the one asked
//! for.
//!
//! None of that is asserted by asking the widget what style it was set to — a property
//! read back says the call was made and nothing about what the reader sees. It is
//! asserted from the pixels of the window, one row at a time down the boundary: how much
//! darker than the settled page each row under the bar is. That profile tells apart the
//! three things the issue is about, which no screenshot comparison can say in words —
//! nothing at all is zeroes, a hard line is one shaded row, and a shadow fades over
//! several.
//!
//! The goldens at the end are the human's half of it: the numbers say the boundary is a
//! shadow, and the pictures say it is the *right* shadow. They are ignored until the
//! owner has looked at them and pinned them, which is theirs to do and never the
//! harness's (`docs/TESTING.md`).

use std::path::PathBuf;

use axiomd_e2e::{App, Bounds, Fixture, Preferences, Screenshot};

const NOTES: &str = "# Reading\n\nA paragraph.\n";

/// How far in from a part's edges the boundary is read. The corners are where a rounded
/// window and a scrollbar are, and neither of those is the boundary.
const MARGIN: i32 = 20;

/// How much of the bar above the boundary a pinned picture of it holds.
const BAND_ABOVE: u32 = 4;

/// How tall a pinned picture of the boundary is: the bottom of the bar, the shadow, and
/// enough of the content under it to show the shadow ending.
const BAND_ROWS: u32 = 20;

/// How dark high contrast's boundary has to be to be a line a reader can see, in channel
/// steps. The ordinary palette's shadow starts around 27 of 255 and the high-contrast
/// one is a solid rule; anything in between them would be neither.
const A_VISIBLE_LINE: u8 = 64;

/// The shading whatever sits above `boundary` casts onto `part` of the window under it.
fn shading(picture: &Screenshot, boundary: i32, part: Bounds) -> Vec<u8> {
    picture.shading_below(
        boundary.max(0) as u32,
        (
            (part.x + MARGIN).max(0) as u32,
            (part.right() - MARGIN).max(0) as u32,
        ),
    )
}

/// How many rows the shading reaches before the content is its own colour again.
fn depth(shading: &[u8]) -> usize {
    shading.iter().take_while(|&&row| row > 0).count()
}

/// Fails unless `shading` is a shadow: something at the boundary, fading as it goes, and
/// gone within a few rows.
///
/// The upper bound on the depth matters as much as the lower one. Too shallow is the
/// hard line this replaces; too deep is a painted band, which is not what Adwaita's
/// four-pixel blur draws and would mean the boundary had been reimplemented by hand.
fn assert_fades_like_a_shadow(shading: &[u8], what: &str) {
    assert!(
        shading[0] > 0,
        "{what} has no boundary at all — the content simply starts: {shading:?}",
    );
    let depth = depth(shading);
    assert!(
        (3..=8).contains(&depth),
        "{what} is shaded {depth} rows deep, which is a line or a band rather than a \
         shadow: {shading:?}",
    );
    assert!(
        shading[..depth].windows(2).all(|pair| pair[0] >= pair[1]),
        "{what} does not fade away from the bar above it: {shading:?}",
    );
}

/// A window reading `NOTES` on a desktop set up by `desktop`, with the fixture the
/// document is in kept alive beside it and the document's own path to hand.
fn reading(label: &str, desktop: &Preferences) -> (Fixture, PathBuf, App) {
    let fixture = Fixture::new(label);
    let document = fixture.write("notes.md", NOTES);
    let app = axiomd_e2e::launch_with(&document, desktop);
    (fixture, document, app)
}

/// The reader's own desktop, with nothing turned on.
fn ordinary(label: &str) -> Preferences {
    Preferences::new(label)
}

/// The document meets the header with a shadow that fades, not with the edge of a
/// differently-coloured strip — all the way across the window, over the outline beside
/// the document as well as over the document.
#[test]
fn the_header_casts_a_fading_shadow_onto_what_is_under_it() {
    let (_fixture, _document, app) = reading("boundary-light", &ordinary("boundary-light"));

    let layout = app.layout();
    let picture = app.window_screenshot();

    assert_fades_like_a_shadow(
        &shading(&picture, layout.document.y, layout.document),
        "the boundary over the document",
    );
    assert_fades_like_a_shadow(
        &shading(&picture, layout.sidebar.y, layout.sidebar),
        "the boundary over the outline",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The dark palette is a palette and not a different design: the same shadow, drawn in
/// what the dark desktop shades with.
#[test]
fn the_dark_palette_separates_the_header_the_same_way() {
    let dark = Preferences::with("boundary-dark", "theme", "'dark'");
    let (_fixture, _document, app) = reading("boundary-dark", &dark);

    let layout = app.layout();
    let picture = app.window_screenshot();

    assert_fades_like_a_shadow(
        &shading(&picture, layout.document.y, layout.document),
        "the boundary over a dark document",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// High contrast draws the same separation as a rule the reader can actually see. That
/// is Adwaita's own answer — its high-contrast stylesheet replaces the raised shadow's
/// faint first pixel with a solid one — and it is the answer this keeps: a reader who
/// asked their desktop for contrast must not be the one reader who cannot tell where the
/// header ends.
#[test]
fn high_contrast_keeps_a_boundary_a_reader_can_see() {
    let desktop = ordinary("boundary-contrast");
    desktop.set_high_contrast(true);
    let (_fixture, _document, app) = reading("boundary-contrast", &desktop);

    let layout = app.layout();
    let picture = app.window_screenshot();
    let boundary = shading(&picture, layout.document.y, layout.document);

    assert_fades_like_a_shadow(&boundary, "the boundary under high contrast");
    assert!(
        boundary[0] >= A_VISIBLE_LINE,
        "high contrast left the boundary as faint as the ordinary palette's: {boundary:?}",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The external-change banner is a second top bar in the same box as the header, so the
/// reader sees one boundary that has moved down rather than two stacked on each other.
///
/// Both halves are asserted, because either one alone would pass on the bug: the
/// boundary under the banner is the very same shadow it was without it, and the seam
/// between the header and the banner has no shadow of its own at all.
#[test]
fn the_banner_moves_the_one_boundary_down_instead_of_adding_a_second() {
    let (_fixture, document, app) = reading("boundary-banner", &ordinary("boundary-banner"));

    let plain = app.layout();
    let alone = shading(&app.window_screenshot(), plain.document.y, plain.document);
    assert_fades_like_a_shadow(&alone, "the boundary with no banner");

    // A document whose file goes away is the banner the reader meets while reading, and
    // the one that leaves the document on screen behind it.
    std::fs::remove_file(&document).expect("delete the document out from under the window");
    app.wait_for_banner("notes.md");

    let bannered = app.layout();
    assert!(
        bannered.document.y > plain.document.y,
        "the banner did not push the document down: {bannered:?}",
    );
    let picture = app.window_screenshot();

    assert_eq!(
        shading(&picture, bannered.document.y, bannered.document),
        alone,
        "the banner changed the boundary under it instead of moving it",
    );
    let seam = shading(&picture, plain.document.y, bannered.document);
    assert_eq!(
        depth(&seam),
        0,
        "a second boundary was drawn between the header and the banner: {seam:?}",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The find bar is the document's own top bar, inside the split rather than across the
/// window (issue #26), so it is a separate `AdwToolbarView` — and it has to speak the
/// same separation language, or pressing Ctrl+F would put a bar over the document with
/// nothing between them.
///
/// The seam above it is checked too: the header's shadow falls onto the bar, and nothing
/// the bar draws is stacked underneath it.
#[test]
fn the_find_bar_carries_the_same_boundary_onto_the_document() {
    let (_fixture, _document, app) = reading("boundary-find", &ordinary("boundary-find"));

    let shut = app.layout();
    let alone = shading(&app.window_screenshot(), shut.document.y, shut.document);
    assert_fades_like_a_shadow(&alone, "the boundary with the find bar shut");

    // Ctrl+F.
    app.activate("win.find");
    app.wait_for("the find bar to open", || app.layout().search.height > 0);

    let open = app.layout();
    assert!(
        open.document.y > shut.document.y,
        "the find bar did not take room above the document: {open:?}",
    );
    let picture = app.window_screenshot();

    assert_eq!(
        shading(&picture, open.document.y, open.document),
        alone,
        "the find bar meets the document differently from the way the header does",
    );
    let seam = shading(&picture, open.search.y, open.document);
    assert!(
        seam[0] > 0 && seam[0] <= alone[0],
        "the header and the find bar are separated by more than the one shadow the \
         header casts: {seam:?} against {alone:?}",
    );

    // Escape puts the boundary back where it was, undoubled.
    app.activate("win.find-close");
    app.wait_for("the find bar to shut", || app.layout().search.height == 0);
    assert_eq!(
        shading(&app.window_screenshot(), shut.document.y, shut.document),
        alone,
        "shutting the find bar left the boundary changed",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A picture of the boundary, cut from the window across the row the content starts on:
/// the bottom of the bar, the shadow, and the content settling under it.
fn boundary_band(app: &App) -> Screenshot {
    let content = app.layout().document.y.max(0) as u32;
    app.window_screenshot()
        .band(content.saturating_sub(BAND_ABOVE), BAND_ROWS)
}

/// The visual specification of the boundary on a light desktop.
///
/// Ignored until a human has looked at the picture and pinned it: approving a rendered
/// surface for the first time is theirs to do, not the harness's (`docs/TESTING.md`). To
/// pin it, look at `target/debug/e2e-artifacts/boundary-light.actual.png` from a failing
/// run and, if it is right, re-run this test with `AXIOMD_PIN_GOLDENS=1` set, then remove
/// the `#[ignore]`.
#[test]
#[ignore = "awaiting the first human visual approval; see the comment above"]
fn the_light_boundary_still_looks_the_way_it_was_approved() {
    let (_fixture, _document, app) =
        reading("boundary-golden-light", &ordinary("boundary-golden-light"));

    boundary_band(&app).assert_matches("boundary-light");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The same boundary on a dark desktop. Pinned the same way, from
/// `boundary-dark.actual.png`.
#[test]
#[ignore = "awaiting the first human visual approval; see the comment above"]
fn the_dark_boundary_still_looks_the_way_it_was_approved() {
    let dark = Preferences::with("boundary-golden-dark", "theme", "'dark'");
    let (_fixture, _document, app) = reading("boundary-golden-dark", &dark);

    boundary_band(&app).assert_matches("boundary-dark");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// And on a desktop asking for high contrast, where the shadow is legitimately a line
/// and the picture is how the owner says whether that line is the right one. Pinned the
/// same way, from `boundary-high-contrast.actual.png`.
#[test]
#[ignore = "awaiting the first human visual approval; see the comment above"]
fn the_high_contrast_boundary_still_looks_the_way_it_was_approved() {
    let desktop = ordinary("boundary-golden-contrast");
    desktop.set_high_contrast(true);
    let (_fixture, _document, app) = reading("boundary-golden-contrast", &desktop);

    boundary_band(&app).assert_matches("boundary-high-contrast");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The boundary with the banner showing — the state a stacked second shadow would be
/// visible in. Pinned the same way, from `boundary-banner.actual.png`.
#[test]
#[ignore = "awaiting the first human visual approval; see the comment above"]
fn the_bannered_boundary_still_looks_the_way_it_was_approved() {
    let (_fixture, document, app) = reading(
        "boundary-golden-banner",
        &ordinary("boundary-golden-banner"),
    );

    std::fs::remove_file(&document).expect("delete the document out from under the window");
    app.wait_for_banner("notes.md");

    boundary_band(&app).assert_matches("boundary-banner");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// And with the find bar open, the other state two bars meet the document in. Pinned the
/// same way, from `boundary-find.actual.png`.
#[test]
#[ignore = "awaiting the first human visual approval; see the comment above"]
fn the_boundary_under_the_find_bar_still_looks_the_way_it_was_approved() {
    let (_fixture, _document, app) =
        reading("boundary-golden-find", &ordinary("boundary-golden-find"));

    app.activate("win.find");
    app.wait_for("the find bar to open", || app.layout().search.height > 0);

    boundary_band(&app).assert_matches("boundary-find");

    assert!(app.close().is_empty(), "the launch left processes behind");
}
