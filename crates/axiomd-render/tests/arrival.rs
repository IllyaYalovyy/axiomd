//! How a document arrives on screen (issue #40).
//!
//! Two halves of one thing. Before a document has drawn anything, the pane it will
//! draw into has to be the colour the document paints itself — otherwise the reader
//! is shown a black frame first, which is the bug. And once the bytes are there the
//! document appears rather than snapping in, unless the reader's desktop has asked
//! for less movement.
//!
//! The colour is asserted against the very hex the stylesheets declare, in both
//! directions: a palette repainted in the CSS and not here fails, and a colour changed
//! here and not in the CSS cannot even be written, because it is read out of the CSS
//! while the crate compiles.

mod support;

use axiomd_render::{Contrast, Motion, Palette, Plugins};
use support::parse;

/// The way a document is being read, for a test that only cares about one axis of it.
fn reading(palette: Palette, contrast: Contrast) -> axiomd_render::Reading {
    axiomd_render::reading(Some(46), palette, contrast, Motion::Full)
}

/// The pane and the page are painted the same colour, for every way of reading there
/// is — and that colour is the one the stylesheet declares, spelled the same on both
/// sides of this assertion.
///
/// This is the whole of the fix for the black frame: a view painted anything else
/// shows it for as long as it takes the document's own bytes to arrive.
#[test]
fn the_pane_is_painted_the_colour_the_page_is_painted() {
    let stylesheet = axiomd_render::stylesheet();
    let high_contrast = reading(Palette::Light, Contrast::High)
        .stylesheet()
        .to_owned();

    // The light document, which is `axiomd.css`'s own `:root`.
    assert!(stylesheet.contains("--axiomd-bg: #ffffff"));
    assert_eq!(
        reading(Palette::Light, Contrast::Normal).background(),
        (0xff, 0xff, 0xff),
    );

    // The dark palette, which `dark.css` declares.
    assert!(stylesheet.contains("--axiomd-bg: #1d1d20"));
    assert_eq!(
        reading(Palette::Dark, Contrast::Normal).background(),
        (0x1d, 0x1d, 0x20),
    );

    // High contrast, which is the desktop's accessibility answer and repaints both
    // readings from the reader's own stylesheet.
    assert!(high_contrast.contains("--axiomd-bg: #ffffff !important"));
    assert_eq!(
        reading(Palette::Light, Contrast::High).background(),
        (0xff, 0xff, 0xff),
    );
    assert!(high_contrast.contains("--axiomd-bg: #000000 !important"));
    assert_eq!(
        reading(Palette::Dark, Contrast::High).background(),
        (0x00, 0x00, 0x00),
    );
}

/// A document appears rather than snapping in — and only where it is read.
///
/// The fade is a page-load animation on the article, so it runs once per loaded page
/// and there is no class for a later render to re-apply. It is screen-only, like the
/// dark palette, so paper never gets it; and it is in the stylesheet the *application*
/// serves, so a document that has left axiomd carries no animation at all.
#[test]
fn a_document_fades_in_where_it_is_read_and_nowhere_else() {
    let stylesheet = axiomd_render::stylesheet();

    let painted = stylesheet
        .find("animation: axiomd-appear")
        .expect("the stylesheet no longer fades a document in");
    assert!(
        stylesheet.contains("@keyframes axiomd-appear"),
        "the document is told to run an animation the stylesheet does not define",
    );

    // ~150–200 ms, and no longer: this is the document arriving, not an effect.
    let duration = &stylesheet[painted..][.."animation: axiomd-appear".len() + 8];
    assert!(
        duration.contains("180ms"),
        "the first appearance is no longer a short fade: {duration}",
    );

    let screen = stylesheet[..painted]
        .rfind("@media screen")
        .expect("the fade is not inside a screen-only block");
    assert!(
        stylesheet[screen..painted].find("@media print").is_none(),
        "the fade is written inside the print block",
    );

    let exported = axiomd_render::standalone(
        &parse("# Title\n\nText.\n"),
        "notes",
        &Plugins::builtin(&[]),
        &axiomd_render::Folder::empty(),
        &|_| None,
    );
    assert!(
        !exported.contains("axiomd-appear"),
        "an exported document carries the fade to somebody else's browser",
    );
}

/// A reader who asked their desktop for less movement gets none of it.
///
/// Said by the application through the reader's own stylesheet, because WebKitGTK does
/// not answer `prefers-reduced-motion` from the desktop it is running on — so it has to
/// outrank the document's own declaration, and it has to stay off paper for the same
/// reason high contrast does.
#[test]
fn a_desktop_asking_for_less_movement_gets_a_document_that_does_not_move() {
    let moving = axiomd_render::reading(Some(46), Palette::Light, Contrast::Normal, Motion::Full);
    assert!(
        !moving.stylesheet().contains("animation"),
        "a desktop that asked for nothing had its animations turned off: {}",
        moving.stylesheet(),
    );

    let still = axiomd_render::reading(Some(46), Palette::Light, Contrast::Normal, Motion::Reduced);
    let sheet = still.stylesheet();
    let stopped = sheet
        .find("animation: none !important")
        .expect("reduced motion leaves the document moving");
    let screen = sheet[..stopped]
        .rfind("@media screen")
        .expect("reduced motion is not inside a screen-only block");
    assert!(
        sheet[screen..stopped].find("@media print").is_none(),
        "reduced motion is written inside a print block: {sheet}",
    );

    // It has to reach the element the fade is on, or it stops nothing.
    assert_eq!(
        selector_of("animation: axiomd-appear", axiomd_render::stylesheet()),
        selector_of("animation: none !important", sheet),
        "reduced motion stops something other than the element that fades",
    );
}

/// The selector of the rule `declaration` is written in.
///
/// The text between the rule's own `{` and whatever closed the block before it, which
/// is all the structure these two one-line rules have.
fn selector_of(declaration: &str, css: &str) -> String {
    let at = css
        .find(declaration)
        .unwrap_or_else(|| panic!("no rule declares {declaration}"));
    let opened = css[..at].rfind('{').expect("a rule the declaration is in");
    let before = css[..opened].rfind(['}', '{']).map_or(0, |at| at + 1);
    css[before..opened].trim().to_owned()
}
