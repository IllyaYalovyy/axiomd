//! Math, as the pipeline produces it (issue #11).
//!
//! Every assertion here is about the document a reader is handed: the equation is
//! markup in it, its source is still in it, and a formula the renderer could not read
//! costs the reader the formula and nothing else. What the equation *looks like* is
//! not this file's question — that is `crates/axiomd-app/tests/math.rs`, which puts
//! the same corpus in front of WebKitGTK.

mod support;

use axiomd_render::{Folder, Plugins};
use support::{parse, render};

/// Renders `source` with every built-in capability switched off but one.
fn without_math(source: &str) -> String {
    axiomd_render::render(
        &parse(source),
        "fixture",
        &Plugins::builtin(&["math".to_owned()]),
        &Folder::empty(),
    )
    .html()
    .to_owned()
}

/// The equation the reader is looking at is MathML, in the paragraph the author wrote
/// it in — not the LaTeX, and not a picture.
#[test]
fn inline_math_is_typeset_where_the_author_wrote_it() {
    let rendered = render("Einstein wrote $E = mc^2$ on a board.\n");
    let html = rendered.html().to_owned();

    assert!(
        html.contains(
            "<p data-line=\"1\">Einstein wrote <math display=\"inline\"><semantics><mrow>\
             <mi>E</mi><mo>=</mo><mi>m</mi><msup><mi>c</mi><mn>2</mn></msup></mrow>"
        ),
        "the equation is not typeset in the paragraph: {html}",
    );
    assert!(
        html.contains("</semantics></math> on a board.</p>"),
        "the prose after the equation did not survive it: {html}",
    );
    assert!(
        !html.contains("class=\"math math-inline\""),
        "the LaTeX is still standing where the equation should be: {html}",
    );
}

/// Display math is a block equation: `display="block"` is what makes WebKitGTK set it
/// centred, on its own line, with full-size operators.
#[test]
fn display_math_is_a_block_equation() {
    let rendered = render("Before.\n\n$$\n\\int_0^1 x^2 \\, dx\n$$\n\nAfter.\n");
    let html = rendered.html().to_owned();

    assert!(
        html.contains("<math display=\"block\">"),
        "display math was typeset inline: {html}",
    );
    assert!(
        html.contains("<msubsup><mo movablelimits=\"false\">∫</mo><mn>0</mn><mn>1</mn></msubsup>"),
        "the integral is not in the document: {html}",
    );
    // The blocks around it are untouched and still carry their own lines.
    assert!(html.contains("<p data-line=\"1\">Before.</p>"), "{html}");
    assert!(html.contains("<p data-line=\"7\">After.</p>"), "{html}");
    assert_eq!(
        rendered
            .anchors()
            .iter()
            .map(|anchor| anchor.line)
            .collect::<Vec<_>>(),
        vec![1, 3, 7],
        "an equation moved the source map (invariant 3)",
    );
}

/// The author's LaTeX travels inside the equation, in the annotation the MathML
/// specification keeps for it — which is what Obsidian's MathJax writes too, and what
/// lets anything downstream get the source back out of a rendered document.
#[test]
fn an_equation_carries_the_source_it_was_written_from() {
    let html = render("Let $x_1 < x_2$ hold.\n").html().to_owned();

    assert!(
        html.contains("<annotation encoding=\"application/x-tex\">x_1 &lt; x_2</annotation>"),
        "the equation does not carry its own source: {html}",
    );
}

/// The equation inside the annotation wrapper is exactly one element.
///
/// `<semantics>` draws its first child and hides everything after it — which is what
/// keeps the annotation out of sight, and what would silently cost the reader every
/// term of a formula after the first if the equation were written into it as the
/// sequence the renderer produces. The failure it guards against was real: an integral
/// reached the page as its integral sign and nothing else.
#[test]
fn an_equation_reaches_the_document_as_a_single_element() {
    let html = render("$$\n\\int_0^1 x \\, dx = 1\n$$\n").html().to_owned();
    let drawn = html
        .split_once("<semantics>")
        .and_then(|(_, rest)| rest.split_once("<annotation"))
        .map(|(drawn, _)| drawn)
        .expect("an equation with an annotation in it");

    assert!(
        drawn.starts_with("<mrow>") && drawn.ends_with("</mrow>"),
        "the equation is a sequence rather than one element, so only its first term \
         would be drawn: {drawn}",
    );
    assert!(
        !drawn["<mrow>".len()..drawn.len() - "</mrow>".len()].contains("</mrow>"),
        "the wrapping row closes early: {drawn}",
    );
}

/// LaTeX the renderer cannot read costs the reader that formula and nothing else:
/// their source stands where they wrote it, marked, with the reason beside it — and
/// every other block of the document, equations included, is exactly as it was.
#[test]
fn unreadable_latex_keeps_the_source_in_a_marked_error_span() {
    let rendered = render(
        "# Notes\n\nGood: $a + b$.\n\nBroken: $a^b^c$ here.\n\n$$\n\\frac{1}\n$$\n\nAfter.\n",
    );
    let html = rendered.html().to_owned();

    assert!(
        html.contains(
            "<span class=\"math-error math-error-inline\">\
             <span class=\"math-error-source\">a^b^c</span>\
             <span class=\"math-error-reason\">trying to add a superscript twice to the same \
             element</span></span>"
        ),
        "the unreadable formula did not degrade to its own marked source: {html}",
    );
    assert!(
        html.contains("class=\"math-error math-error-display\""),
        "a display equation that failed did not degrade as a block: {html}",
    );
    assert!(
        html.contains("<span class=\"math-error-source\">\n\\frac{1}\n</span>"),
        "the reader lost the source of the equation that failed: {html}",
    );
    // The document is whole: the heading, the good equation, and the prose after.
    assert!(
        html.contains("<h1 id=\"notes\" data-line=\"1\">Notes</h1>"),
        "{html}"
    );
    assert!(
        html.contains("<math display=\"inline\">"),
        "one broken formula took a good one with it: {html}",
    );
    assert!(html.contains("<p data-line=\"11\">After.</p>"), "{html}");
    assert_eq!(
        rendered
            .anchors()
            .iter()
            .map(|anchor| anchor.line)
            .collect::<Vec<_>>(),
        vec![1, 3, 5, 7, 11],
    );
}

/// The whole point of MathML: a document full of equations still runs nothing. There
/// is no script for the page to load and none for the app to run beside it.
#[test]
fn typesetting_needs_no_script_at_all() {
    let rendered = render("$$\n\\sum_{i=1}^n i = \\frac{n(n+1)}{2}\n$$\n");

    assert_eq!(
        rendered.scripts(),
        &[] as &[String],
        "an equation asked the app to run something",
    );
    assert!(
        !rendered.html().contains("<script"),
        "a script reached a document that only has maths in it: {}",
        rendered.html(),
    );
}

/// The styling — and with it the font — reaches the documents that have an equation in
/// them and no others. A reader whose notes have no maths in them downloads no maths.
#[test]
fn the_math_styling_reaches_only_the_documents_that_used_it() {
    let with = render("An equation: $\\pi r^2$.\n").html().to_owned();
    let without = render("No equation here, just a dollar: $5.\n")
        .html()
        .to_owned();

    assert!(
        with.contains("<link rel=\"stylesheet\" href=\"axiomd://assets/plugin/math/math.css\">"),
        "a document with an equation carries no maths styling: {with}",
    );
    assert!(
        !without.contains("math.css"),
        "a document with no equation loaded the maths styling: {without}",
    );
}

/// The font is a file that shipped with the application, served by name and by nothing
/// else — which is what "typesetting fetches nothing" rests on.
#[test]
fn the_math_font_is_served_from_the_application_itself() {
    let font =
        axiomd_render::asset("/plugin/math/stix-two-math.woff2").expect("the bundled math font");

    assert_eq!(font.content_type, "font/woff2");
    assert_eq!(
        &font.bytes[..4],
        b"wOF2",
        "what is served under the font's name is not a WOFF2 file",
    );
    assert_eq!(axiomd_render::asset("/plugin/math/stix-two-math.otf"), None);
}

/// Switching the capability off gives back exactly the document a build without it
/// produces — the LaTeX, as text, and not one byte of maths anywhere near it.
#[test]
fn a_reader_who_switches_math_off_gets_their_latex_back() {
    let source = "Einstein wrote $E = mc^2$.\n\n$$\n\\frac{a}{b}\n$$\n";
    let off = without_math(source);
    let none = axiomd_render::render(
        &parse(source),
        "fixture",
        &Plugins::of([]),
        &Folder::empty(),
    )
    .html()
    .to_owned();

    assert_eq!(off, none, "a switched-off capability still cost something");
    assert!(
        off.contains("<span class=\"math math-inline\">E = mc^2</span>"),
        "the reader did not get their source back: {off}",
    );
    assert!(!off.contains("<math"), "{off}");
    assert!(!off.contains("math.css"), "{off}");
}

/// An equation in a heading is still a heading: the outline says what the section is
/// called, and the link the document's own text gives it does not move because a
/// capability is switched on.
#[test]
fn an_equation_in_a_heading_keeps_the_outline_and_the_link() {
    let rendered = render("## Sorting in $O(n \\log n)$\n\nText.\n");

    assert_eq!(rendered.outline().len(), 1);
    assert_eq!(rendered.outline()[0].text, "Sorting in O(n \\log n)");
    assert_eq!(rendered.outline()[0].line, 1);
    assert!(
        rendered
            .html()
            .contains("<h2 id=\"sorting-in-on-log-n\" data-line=\"1\">"),
        "{}",
        rendered.html(),
    );
    assert!(
        rendered.html().contains("<math display=\"inline\">"),
        "the heading's equation was not typeset: {}",
        rendered.html(),
    );
}

/// A picture's label is words, not markup: an equation in alt text reads as the source
/// it was written from, exactly as it did before this capability existed.
#[test]
fn an_equation_in_alt_text_stays_the_words_it_was() {
    let html = render("![the identity $e^{i\\pi} = -1$](picture.png)\n")
        .html()
        .to_owned();

    assert!(
        html.contains("alt=\"the identity e^{i\\pi} = -1\""),
        "the alt text lost the equation: {html}",
    );
    assert!(
        !html.contains("<math"),
        "markup reached an alt attribute: {html}",
    );
}

/// MathML written by hand in a document is markup like any other: what the layout
/// engine needs survives, and what could act does not.
#[test]
fn hand_written_mathml_is_admitted_and_still_cleaned() {
    let html = render(
        "<math display=\"block\"><mrow onclick=\"steal()\"><mi>x</mi>\
         <mo stretchy=\"true\" style=\"color: rgb(1 2 3); position: fixed\">+</mo>\
         <mglyph src=\"https://example.com/x.png\"></mglyph></mrow>\
         <script>alert(1)</script></math>\n",
    )
    .html()
    .to_owned();

    assert!(html.contains("<math display=\"block\">"), "{html}");
    assert!(html.contains("<mo stretchy=\"true\""), "{html}");
    assert!(
        html.contains("style=\"color:rgb(1 2 3)\""),
        "the colour a formula may set did not survive: {html}",
    );
    assert!(
        !html.contains("position: fixed"),
        "a document positioned an element through a style attribute: {html}",
    );
    assert!(!html.contains("onclick"), "{html}");
    assert!(!html.contains("<script"), "{html}");
    assert!(
        !html.contains("example.com"),
        "a hand-written equation reached the network: {html}",
    );
}

/// An exported file has no application behind it, so the face its equations are set in
/// travels inside the file — as do the equations themselves.
#[test]
fn an_exported_document_carries_the_math_font_rather_than_naming_it() {
    let parsed = parse("The area is $\\pi r^2$.\n");
    let exported = axiomd_render::standalone(
        &parsed,
        "notes",
        &Plugins::builtin(&[]),
        &Folder::empty(),
        &|_| None,
    );

    assert!(exported.contains("<math display=\"inline\">"), "{exported}");
    assert!(
        exported.contains("src: url(\"data:font/woff2;base64,d09GM"),
        "the exported file names a font it cannot fetch instead of carrying it",
    );
    assert!(
        !exported.contains("axiomd://"),
        "an exported file names something only the app can answer for",
    );
}
