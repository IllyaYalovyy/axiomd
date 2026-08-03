//! The stylesheet the reader's own choices arrive as.
//!
//! A layout preference must reach a document already on screen without the document
//! being parsed, rendered or loaded again (`design_decisions.md`, invariant 9), so it
//! travels as CSS and nothing else. These tests hold the two halves of that together:
//! what the default stylesheet reads the measure from, and what the override writes.

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
        axiomd_render::reader_stylesheet(Some(80)).contains("--axiomd-reading-width"),
        "the reader's stylesheet sets some other property than the one the document \
         reads its measure from",
    );
}

/// What the reader gets for each of the two states of the preference.
#[test]
fn the_reader_stylesheet_carries_the_measure_they_chose_or_removes_it() {
    let narrow = axiomd_render::reader_stylesheet(Some(30));
    assert!(narrow.contains("--axiomd-reading-width: 30rem"), "{narrow}");

    let wide = axiomd_render::reader_stylesheet(Some(120));
    assert!(wide.contains("--axiomd-reading-width: 120rem"), "{wide}");

    // "No limit" is a value like any other: `max-width: none` is what a document
    // filling the window is.
    let unlimited = axiomd_render::reader_stylesheet(None);
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
        let sheet = axiomd_render::reader_stylesheet(width);
        assert!(
            sheet.contains("!important"),
            "a user stylesheet without !important cannot override the document's own: \
             {sheet}",
        );
    }
}

/// Nothing but the reader's choices: a user stylesheet applies to every document this
/// application shows, so anything that crept in here would be unremovable styling.
#[test]
fn the_reader_stylesheet_says_nothing_the_reader_did_not_ask_for() {
    let sheet = axiomd_render::reader_stylesheet(Some(46));
    let declarations = sheet.matches(':').count() - sheet.matches(":root").count();
    assert_eq!(
        declarations, 1,
        "more than the reading width is set: {sheet}"
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
