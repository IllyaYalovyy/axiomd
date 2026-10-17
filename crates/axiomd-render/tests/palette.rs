//! Which palette a document is read in, said by the application (issue #60).
//!
//! The document's own stylesheet carries both palettes and asks the page which one is
//! wanted. An engine that answers that question once and never again leaves a reader
//! who goes dark with a white document — WebKitGTK 2.54 does exactly that (WebKit bug
//! 327072) — so the application states the palette in the reader's own stylesheet
//! instead, in both directions and for everything a reader can see of a palette.
//!
//! These tests hold the statement to the stylesheets it stands in for: the same
//! properties, the same colours, the same code palette, and none of it on paper. What
//! a *window* does with it is asserted against the running application
//! (`axiomd-app/tests/appearance.rs`, `preferences.rs`, `mermaid.rs`) and against the
//! installed package (`axiomd-app/tests/packaging.rs`), which is where the defect was
//! visible.

use axiomd_render::{Contrast, Motion, Palette};

/// The stylesheet a reader reading in `palette` is given, with nothing else about the
/// way they read asked for.
fn reader_stylesheet(palette: Palette) -> String {
    axiomd_render::reading(Some(46), palette, Contrast::Normal, Motion::Full)
        .stylesheet()
        .to_owned()
}

/// The palette is said, and said as the very colours the document's own stylesheet
/// declares — so a palette repainted in the CSS and not here cannot happen.
#[test]
fn the_reader_stylesheet_says_which_palette_is_in_force() {
    let light = reader_stylesheet(Palette::Light);
    let dark = reader_stylesheet(Palette::Dark);

    assert!(
        light.contains("color-scheme: light !important"),
        "a light reader's stylesheet does not say the page is light: {light}",
    );
    assert!(
        light.contains("--axiomd-bg: #ffffff !important"),
        "a light reader's stylesheet does not paint the light page: {light}",
    );

    assert!(
        dark.contains("color-scheme: dark !important"),
        "a dark reader's stylesheet does not say the page is dark: {dark}",
    );
    assert!(
        dark.contains("--axiomd-bg: #1d1d20 !important"),
        "a dark reader's stylesheet does not paint the dark page: {dark}",
    );
}

/// Both directions, which is the half of this that is easy to forget: a page whose
/// answer is stuck is stuck at dark as readily as at light, so a reader who opened a
/// window on a dark desktop and took it back to light needs the light palette said to
/// them just as explicitly.
///
/// Said of exactly the same properties, too. A property the dark statement repaints
/// and the light one does not is a colour that would never come back.
#[test]
fn both_palettes_are_stated_and_they_state_the_same_properties() {
    let light = stated_properties(&reader_stylesheet(Palette::Light));
    let dark = stated_properties(&reader_stylesheet(Palette::Dark));

    assert!(
        dark.len() >= 20,
        "the dark palette states almost nothing: {dark:?}",
    );
    assert_eq!(
        light.iter().map(|(name, _)| name).collect::<Vec<_>>(),
        dark.iter().map(|(name, _)| name).collect::<Vec<_>>(),
        "the two palettes do not repaint the same properties, so one of them leaves a \
         colour behind",
    );

    // And every property either of them states is one the document is really painted
    // from: a statement about a property no stylesheet reads does nothing at all.
    let document = axiomd_render::stylesheet();
    for (property, _) in &dark {
        assert!(
            document.contains(&format!("{property}:")),
            "the palette states {property}, which no document stylesheet defines",
        );
    }

    // The page itself is the one worth naming: it is both the colour of the document
    // and the colour the pane is painted before the document has drawn (issue #40).
    let page = |palette: &[(String, String)]| {
        palette
            .iter()
            .find(|(property, _)| property == "--axiomd-bg")
            .map(|(_, value)| value.clone())
            .expect("a palette paints the page")
    };
    assert_ne!(
        page(&light),
        page(&dark),
        "the two palettes paint the page the same colour",
    );
}

/// The measure is the reader's own preference and no part of a palette. A palette that
/// restated it would pin every document to whatever width was in the stylesheet it was
/// copied from — the reader's narrow measure would survive going dark and not coming
/// back, or not survive at all.
#[test]
fn a_palette_leaves_the_reader_their_own_measure() {
    for palette in [Palette::Light, Palette::Dark] {
        let narrow = axiomd_render::reading(Some(30), palette, Contrast::Normal, Motion::Full)
            .stylesheet()
            .to_owned();

        assert_eq!(
            narrow.matches("--axiomd-reading-width").count(),
            1,
            "the measure is said more than once to a {palette:?} reader: {narrow}",
        );
        assert!(
            narrow.contains("--axiomd-reading-width: 30rem !important"),
            "a {palette:?} reader lost the measure they chose: {narrow}",
        );
    }
}

/// Every declaration in it is important, because the statement has to outrank the
/// document's own: it is delivered as a *user* stylesheet, and under the cascade (CSS
/// Cascading and Inheritance Level 5, §6.2) a normal user declaration loses to a
/// normal author one. An unimportant palette would be a palette that does nothing.
#[test]
fn the_stated_palette_outranks_the_documents_own() {
    for palette in [Palette::Light, Palette::Dark] {
        let sheet = reader_stylesheet(palette);
        let statement = &sheet[..sheet
            .find(":root { --axiomd-reading-width")
            .expect("the measure follows the palette")];

        let declarations = statement.matches(';').count();
        assert!(
            declarations >= 40,
            "the {palette:?} palette states {declarations} declarations, which is not a \
             palette: {statement}",
        );
        for declaration in statement.split(';') {
            let Some((property, _)) = declaration.split_once(':') else {
                continue;
            };
            let property = property.trim();
            if property.starts_with('/') || property.contains('{') || property.is_empty() {
                continue;
            }
            assert!(
                declaration.contains("!important"),
                "the {palette:?} palette states {property} without outranking the \
                 document's own declaration of it",
            );
        }
    }
}

/// Code goes with it. The code palettes are the one part of a palette that is rules
/// rather than properties, and a reader whose document went dark around a light code
/// block has not been given the dark palette.
#[test]
fn the_code_palette_goes_with_the_palette() {
    let light = reader_stylesheet(Palette::Light);
    let dark = reader_stylesheet(Palette::Dark);

    let light_code = declarations_for(&light, ".sy-code {").expect("a light code palette");
    let dark_code = declarations_for(&dark, ".sy-code {").expect("a dark code palette");
    assert_ne!(
        light_code.trim(),
        dark_code.trim(),
        "both palettes paint code blocks the same",
    );

    // And it is the document stylesheet's own palette, said again rather than a second
    // one that could drift from it: the dark code block is painted the colour the
    // stylesheet's dark block paints it.
    let document = axiomd_render::stylesheet();
    let dark_block = &document[document
        .rfind("@media screen and (prefers-color-scheme: dark)")
        .expect("the stylesheet has a dark code palette")..];
    let declared = declarations_for(dark_block, ".sy-code {").expect("a dark code palette");
    for colour in declared.split(';').filter(|value| value.contains('#')) {
        assert!(
            dark_code.contains(colour.trim()),
            "the stated dark code palette paints a code block {dark_code}, where the \
             document's own dark palette paints it {colour}",
        );
    }

    assert!(
        dark.contains(".sy-keyword") && light.contains(".sy-keyword"),
        "a stated palette is missing its syntax classes",
    );
}

/// None of it reaches paper. A print job and an exported PDF are paginated from the
/// very page on screen with this stylesheet installed (`export.rs`), so a dark palette
/// outside a `screen` block would print a reader's document white on black and carry
/// the same to a PDF they send somebody.
#[test]
fn a_stated_palette_never_reaches_paper() {
    for palette in [Palette::Light, Palette::Dark] {
        let sheet = reader_stylesheet(palette);
        let page = sheet
            .find("--axiomd-bg")
            .expect("the palette paints the page");
        let screen = sheet[..page]
            .rfind("@media screen")
            .expect("the palette is stated outside a screen-only block");
        assert!(
            sheet[screen..page].find("@media print").is_none(),
            "the palette is stated from inside a print block: {sheet}",
        );
        assert!(
            sheet[..page].matches('}').count() <= sheet[..screen].matches('}').count(),
            "the screen block the palette is stated in was already closed: {sheet}",
        );
    }
}

/// The one thing on the page a drawing plugin can read the palette off.
///
/// A rendered document runs no event listener — it is displayed with scripting off —
/// so a plugin that has to *draw* in the right palette cannot be told by a media
/// query or a signal. What it can do is observe a box resizing, and this is that box:
/// one pixel wide in the light palette and two in the dark one, stated here with the
/// rest of the palette so the two can never disagree. That a diagram really follows it
/// is asserted against the running application
/// (`axiomd-app/tests/mermaid.rs::a_diagram_follows_the_desktop_from_light_to_dark`).
#[test]
fn a_drawing_plugin_can_read_the_palette_off_the_page() {
    let light = reader_stylesheet(Palette::Light);
    let dark = reader_stylesheet(Palette::Dark);

    assert_eq!(
        declarations_for(&light, ".axiomd-scheme {")
            .expect("a light reader's stylesheet sizes the box a plugin reads")
            .trim_matches([' ', ';']),
        "width: 1px !important",
    );
    assert_eq!(
        declarations_for(&dark, ".axiomd-scheme {")
            .expect("a dark reader's stylesheet sizes the box a plugin reads")
            .trim_matches([' ', ';']),
        "width: 2px !important",
    );
}

/// The custom properties `sheet` states before it says anything else — the palette in
/// force, as a list of what it paints and what it paints it.
fn stated_properties(sheet: &str) -> Vec<(String, String)> {
    let open = sheet.find(":root").expect("a stylesheet states a palette");
    let open = sheet[open..].find('{').expect("a `:root` rule with a body") + open + 1;
    let close = sheet[open..]
        .find('}')
        .expect("a `:root` rule that is closed")
        + open;
    sheet[open..close]
        .split(';')
        .filter_map(|declaration| {
            let (property, value) = declaration.split_once(':')?;
            let property = property.trim();
            property.starts_with("--").then(|| {
                (
                    property.to_owned(),
                    value.replace("!important", "").trim().to_owned(),
                )
            })
        })
        .collect()
}

/// The declarations of the first rule in `stylesheet` whose selector mentions
/// `selector`.
fn declarations_for<'a>(stylesheet: &'a str, selector: &str) -> Option<&'a str> {
    let at = stylesheet.find(selector)? + selector.len();
    let close = stylesheet[at..].find('}')? + at;
    Some(&stylesheet[at..close])
}
