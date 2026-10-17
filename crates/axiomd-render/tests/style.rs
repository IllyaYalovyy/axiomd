//! The stylesheet the reader's own choices arrive as.
//!
//! A layout preference must reach a document already on screen without the document
//! being parsed, rendered or loaded again (`design_decisions.md`, invariant 9), so it
//! travels as CSS and nothing else. These tests hold the two halves of that together:
//! what the default stylesheet reads the measure from, and what the override writes.
//!
//! The palette the override also states is `palette.rs`'s, and `palette.rs`'s tests.

use axiomd_render::{Contrast, Motion, Palette};

/// The reader's own stylesheet, which is the half of a [`axiomd_render::Reading`]
/// these tests are about. The other half — the colour the pane is painted before the
/// document arrives — is asserted in `arrival.rs`.
fn reader_stylesheet(reading_width: Option<u32>, contrast: Contrast) -> String {
    axiomd_render::reading(reading_width, Palette::Light, contrast, Motion::Full)
        .stylesheet()
        .to_owned()
}

/// The contract between the two stylesheets, spelled out in both directions. Rename
/// the property in one of them and this fails rather than the reading width quietly
/// ceasing to do anything.
#[test]
fn the_measure_of_a_document_is_the_property_the_reader_stylesheet_sets() {
    let default = axiomd_render::stylesheet();

    assert!(
        default.contains("max-width: var(--axiomd-reading-width)"),
        "the default stylesheet no longer takes its measure from the property the \
         reader's own stylesheet sets",
    );
    assert!(
        reader_stylesheet(Some(80), Contrast::Normal).contains("--axiomd-reading-width"),
        "the reader's stylesheet sets some other property than the one the document \
         reads its measure from",
    );
}

/// What the reader gets for each of the two states of the preference.
#[test]
fn the_reader_stylesheet_carries_the_measure_they_chose_or_removes_it() {
    let narrow = reader_stylesheet(Some(30), Contrast::Normal);
    assert!(narrow.contains("--axiomd-reading-width: 30rem"), "{narrow}");

    let wide = reader_stylesheet(Some(120), Contrast::Normal);
    assert!(wide.contains("--axiomd-reading-width: 120rem"), "{wide}");

    // "No limit" is a value like any other: `max-width: none` is what a document
    // filling the window is.
    let unlimited = reader_stylesheet(None, Contrast::Normal);
    assert!(
        unlimited.contains("--axiomd-reading-width: none"),
        "{unlimited}",
    );
    assert!(
        !unlimited.contains("rem"),
        "an unlimited document still carries a measure: {unlimited}",
    );
}

/// It is applied as a *user* stylesheet, and the document's own is an author one.
/// Under the cascade (CSS Cascading and Inheritance Level 5, §6.2) a normal user
/// declaration loses to a normal author one and an important user declaration beats
/// every author declaration — so without this the reader's choice would silently do
/// nothing.
#[test]
fn the_reader_stylesheet_outranks_the_documents_own() {
    for width in [Some(46), None] {
        let sheet = reader_stylesheet(width, Contrast::Normal);
        assert!(
            sheet.contains("!important"),
            "a user stylesheet without !important cannot override the document's own: \
             {sheet}",
        );
    }
}

/// Nothing but the reader's choices and the palette they are reading in: a user
/// stylesheet applies to every document this application shows, so a rule in here that
/// reached the document's own elements would be styling no reader could remove.
///
/// The measure and the palette are the whole of it (`palette.rs`), and neither of them
/// is a rule about a paragraph, a table or a link.
#[test]
fn the_reader_stylesheet_says_nothing_the_reader_did_not_ask_for() {
    let sheet = reader_stylesheet(Some(46), Contrast::Normal);

    assert_eq!(
        sheet.matches("--axiomd-reading-width").count(),
        1,
        "the measure is said more than once: {sheet}",
    );
    for element in [
        ".markdown",
        "article",
        "body ",
        "body{",
        "table",
        "@keyframes",
    ] {
        assert!(
            !sheet.contains(element),
            "the reader's own stylesheet restyles {element}: {sheet}",
        );
    }
}

/// A desktop that is not asking for high contrast gets a stylesheet that says nothing
/// about contrast at all — the palette it states is the one the document defines,
/// colour for colour, and not a repaint of it.
#[test]
fn a_normal_desktop_is_never_given_high_contrast_styling() {
    let sheet = reader_stylesheet(Some(46), Contrast::Normal);
    assert!(
        sheet.contains("--axiomd-fg: rgb(0 0 6 / 80%) !important"),
        "an ordinary light desktop is not given the document's own ink: {sheet}",
    );
    assert!(
        !sheet.contains("--axiomd-fg: #000000"),
        "an ordinary desktop had its ink repainted at full strength: {sheet}",
    );
    assert!(
        !sheet.contains("text-decoration"),
        "an ordinary desktop had its links underlined: {sheet}",
    );
}

/// High contrast is a repaint of the palette the document defines, so every property
/// it repaints has to be one the document actually reads. A property renamed on one
/// side and not the other is an accessibility setting that silently does nothing.
#[test]
fn high_contrast_repaints_properties_the_document_stylesheet_defines() {
    let document = axiomd_render::stylesheet();
    let sheet = reader_stylesheet(Some(46), Contrast::High);

    let repainted: Vec<&str> = sheet
        .match_indices("--axiomd-")
        .map(|(at, _)| {
            let name = &sheet[at..];
            &name[..name.find(':').expect("a custom property is declared")]
        })
        .filter(|name| *name != "--axiomd-reading-width")
        .collect();

    assert!(
        repainted.len() >= 5,
        "high contrast repaints almost nothing: {repainted:?}",
    );
    for property in &repainted {
        assert!(
            document.contains(&format!("{property}:")),
            "high contrast repaints {property}, which no document stylesheet defines",
        );
    }
    // The three the issue names as the difference between high contrast and a dark
    // palette: full-strength ink, borders that are actually there, and links that are
    // not told apart by colour alone.
    for property in ["--axiomd-fg", "--axiomd-border"] {
        assert!(
            repainted.contains(&property),
            "high contrast leaves {property} as it was: {repainted:?}",
        );
    }
    assert!(
        sheet.contains("text-decoration: underline"),
        "high contrast leaves links told apart by colour alone: {sheet}",
    );
    // The underline itself is not the change — a browser draws one anyway. What high
    // contrast changes is that it is drawn at full strength rather than in the 40%
    // wash of the link's colour the document's own stylesheet asks for.
    assert!(
        sheet.contains("text-decoration-color: currentColor"),
        "high contrast leaves link underlines in a wash of their own colour: {sheet}",
    );
}

/// Both ways of reading. High contrast is not "the dark palette": a reader on a light
/// desktop gets dark ink on white and a reader on a dark one gets the reverse, and the
/// reader's stylesheet carries the reading that is in force — chosen by the palette the
/// application states, since the page cannot be asked which one it is in
/// (`palette.rs`, issue #60).
#[test]
fn high_contrast_has_a_light_reading_and_a_dark_one() {
    let light = reader_stylesheet(Some(46), Contrast::High);
    let dark = axiomd_render::reading(Some(46), Palette::Dark, Contrast::High, Motion::Full)
        .stylesheet()
        .to_owned();

    assert!(
        light.contains("--axiomd-fg: #000000 !important"),
        "a light high-contrast reader is not given full-strength black ink: {light}",
    );
    assert!(
        !light.contains("--axiomd-fg: #ffffff"),
        "a light high-contrast reader is given the dark reading too: {light}",
    );

    // The dark reading comes last, which is the whole of why it wins: both readings are
    // important user declarations of the same property, so the cascade is decided by
    // which was written later (CSS Cascading and Inheritance Level 5, §6.2).
    let light_ink = dark
        .find("--axiomd-fg: #000000 !important")
        .expect("the dark reader's stylesheet carries the light high-contrast reading");
    let dark_ink = dark
        .find("--axiomd-fg: #ffffff !important")
        .expect("high contrast says nothing about a dark reader");
    assert!(
        dark_ink > light_ink,
        "the dark high-contrast reading is overridden by the light one it repaints: \
         {dark}",
    );
}

/// And none of it reaches paper. A print job and an exported PDF are paginated from
/// the very page on screen (`export.rs`) with this stylesheet still installed, so
/// without a `screen` scope a reader whose desktop is in high contrast would print
/// black-bordered tables and underlined links they never saw on screen — and the
/// exported file would carry them to somebody whose desktop is not.
#[test]
fn high_contrast_never_reaches_paper() {
    let sheet = reader_stylesheet(Some(46), Contrast::High);
    let ink = sheet
        .find("--axiomd-fg")
        .expect("high contrast repaints the ink");
    let screen = sheet[..ink]
        .rfind("@media screen")
        .expect("high contrast is painted outside a screen-only block");
    assert!(
        sheet[screen..ink].find("@media print").is_none(),
        "high contrast is painted from inside a print block: {sheet}",
    );
    assert!(
        sheet[..ink].matches('}').count() <= sheet[..screen].matches('}').count(),
        "the screen block high contrast is painted in was already closed: {sheet}",
    );
}

/// The reader's search never reaches paper (issue #8).
///
/// A print job and an exported PDF are paginated from the very page on screen
/// (`export.rs`), marks and all, so the only thing keeping a search off the printed
/// document is where its colours are written: inside a `screen` block, and neutralised
/// again inside the `print` one. Without both, a reader who pressed Ctrl+P with the bar
/// open would get every occurrence of their word highlighted on paper — in the
/// browser's own yellow, which is the UA default a `<mark>` falls back to.
#[test]
fn a_search_highlight_is_screen_only_and_neutralised_on_paper() {
    let stylesheet = axiomd_render::stylesheet();
    // The rule rather than the words: the stylesheet talks about the search in prose
    // above it, and prose is not what paints anything.
    let painted = stylesheet
        .find(".markdown mark.axiomd-find {")
        .expect("the stylesheet paints the search highlight");
    let screen = stylesheet[..painted]
        .rfind("@media screen")
        .expect("the search highlight is inside a screen-only block");
    assert!(
        stylesheet[screen..painted].find("@media print").is_none(),
        "the search highlight is painted from inside the print block",
    );

    let print = stylesheet
        .find("@media print")
        .expect("the stylesheet has a print block");
    let on_paper = declarations_for(&stylesheet[print..], ".markdown mark.axiomd-find {")
        .expect("the print block says nothing about a search highlight");
    assert!(
        on_paper.contains("background: none"),
        "a search highlight keeps a background on paper: {on_paper}",
    );
}

/// The declarations of the first rule in `stylesheet` whose selector mentions
/// `selector`.
fn declarations_for<'a>(stylesheet: &'a str, selector: &str) -> Option<&'a str> {
    let at = stylesheet.find(selector)?;
    let open = stylesheet[at..].find('{')? + at;
    let close = stylesheet[open..].find('}')? + open;
    Some(&stylesheet[open + 1..close])
}
