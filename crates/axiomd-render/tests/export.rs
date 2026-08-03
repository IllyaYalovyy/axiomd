//! The document as one file somebody else can open.
//!
//! An exported page leaves axiomd behind: it is read in a browser, on a machine that
//! may have no network, and it must still be the document the reader was looking at.
//! So everything it needs travels inside it — the stylesheet, the pictures — and
//! nothing in it can reach for anything: no stylesheet link, no `axiomd:` URI the app
//! alone answers for, no address that would be fetched on open.
//!
//! These tests read the exported file the way a browser does: what it fetches, what
//! it shows, and what it is called.

mod support;

use support::{golden_dir, parse};

/// A real PNG's first bytes, so an inlined image is asserted on image content rather
/// than on a text file wearing a `.png` name.
const PIXEL_PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR";

/// Exports `source` with `resolve` answering for the files it names.
fn export(source: &str, name: &str) -> String {
    let parsed = parse(source);
    axiomd_render::standalone(&parsed, name, &|reference| {
        (reference == "images/logo.png").then(|| axiomd_render::Picture {
            bytes: PIXEL_PNG.to_vec(),
            content_type: "image/png".to_owned(),
        })
    })
}

/// Every reference in `html` that a browser would fetch on open: what a stylesheet
/// link, an image or a style's `url()` points at.
///
/// Anchors are not here on purpose: a link the author wrote is content, and following
/// it is the reader's own click. Everything in this list is fetched without anyone
/// asking.
fn fetched_on_open(html: &str) -> Vec<String> {
    let mut fetched = Vec::new();
    for (opening, attribute) in [
        ("<img", "src=\""),
        ("<link", "href=\""),
        ("<script", "src=\""),
    ] {
        let mut rest = html;
        while let Some(at) = rest.find(opening) {
            rest = &rest[at + opening.len()..];
            let element = &rest[..rest.find('>').unwrap_or(rest.len())];
            if let Some(value) = element.split_once(attribute) {
                let value = value.1.split('"').next().unwrap_or_default();
                fetched.push(value.to_owned());
            }
        }
    }
    for marker in ["url(", "@import"] {
        let mut rest = html;
        while let Some(at) = rest.find(marker) {
            rest = &rest[at + marker.len()..];
            fetched.push(rest[..rest.len().min(60)].to_owned());
        }
    }
    fetched
}

/// The exported file itself, pinned — the one place a change to any of it shows up
/// as an exact diff rather than as a property that still happens to hold.
///
/// The stylesheet the file carries is stood aside from the pinned text and asserted
/// on its own below. It is fifteen kilobytes of the same CSS `axiomd.css` already
/// pins, so putting a copy of it in a golden would mean every colour change had to be
/// reviewed twice and would bury the thing this golden is actually for: the markup of
/// the document, and everything the export does to it.
#[test]
fn an_exported_document_is_the_file_that_was_pinned() {
    let source = std::fs::read_to_string(golden_dir().join("exported/document.md"))
        .expect("the export fixture");
    let path = golden_dir().join("exported/document.html");

    let exported = with_the_stylesheet_set_aside(&export(&source, "document"));
    let golden = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("no golden export at {}", path.display()));

    assert_eq!(
        exported,
        golden,
        "the exported document is no longer the file that was pinned{}",
        first_difference(&exported, &golden),
    );
}

/// The inlined stylesheet, replaced by a marker — after checking that it is the one
/// the export promises, so the pinned file still fails if the styling goes missing or
/// is swapped for another.
fn with_the_stylesheet_set_aside(exported: &str) -> String {
    let (before, rest) = exported
        .split_once("<style>\n")
        .expect("the exported document carries an inlined stylesheet");
    let (stylesheet, after) = rest
        .split_once("</style>")
        .expect("the inlined stylesheet is closed");
    assert!(
        stylesheet.contains(".markdown h1") && stylesheet.contains(".sy-keyword"),
        "the inlined stylesheet is not the document's and its code palette",
    );
    format!("{before}<style>[axiomd.css + the light code palette]</style>{after}")
}

/// The first differing line, with its neighbourhood: a whole-document `assert_eq!`
/// prints an unreadable wall.
fn first_difference(actual: &str, expected: &str) -> String {
    let (actual, expected): (Vec<&str>, Vec<&str>) =
        (actual.lines().collect(), expected.lines().collect());
    let at = (0..actual.len().max(expected.len()))
        .find(|&line| actual.get(line) != expected.get(line))
        .unwrap_or(0);
    fn line(lines: &[&str], at: usize) -> String {
        lines
            .get(at)
            .copied()
            .unwrap_or("<end of document>")
            .to_owned()
    }
    format!(
        "\n  first difference at line {}:\n    expected: {}\n    actual:   {}",
        at + 1,
        line(&expected, at),
        line(&actual, at),
    )
}

/// The whole point of a standalone export: the file is the document, pictures and
/// styling included.
#[test]
fn an_exported_document_carries_its_stylesheet_and_its_pictures_inside_it() {
    let exported = export("# Notes\n\n![Logo](images/logo.png)\n", "notes");

    assert!(
        exported.contains("<style>"),
        "the stylesheet was not inlined: {exported}",
    );
    assert!(
        exported.contains("max-width: var(--axiomd-reading-width)"),
        "the inlined stylesheet is not axiomd's own",
    );
    assert!(
        !exported.contains("<link"),
        "the exported document still links to a stylesheet it does not carry",
    );
    // The bytes themselves, base64 of the PNG above — not merely "some data URI".
    assert!(
        exported.contains("src=\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUg==\""),
        "the picture is not in the file: {exported}",
    );
    assert!(
        exported.contains("<h1 id=\"notes\""),
        "the document itself is missing",
    );
}

/// Opened on a machine with no network — or with one, which is worse — an exported
/// document asks for nothing at all.
#[test]
fn an_exported_document_fetches_nothing_when_it_is_opened() {
    let exported = export(
        "# Notes\n\n![Logo](images/logo.png)\n\n![Far](https://cdn.example.com/far.png)\n\n\
         See [the site](https://example.com/page).\n\n\
         <img src=\"http://tracker.example.com/pixel.png\">\n",
        "notes",
    );

    for reference in fetched_on_open(&exported) {
        assert!(
            reference.starts_with("data:"),
            "the exported document fetches {reference:?} when it is opened",
        );
    }
    assert!(
        !exported.contains("axiomd:"),
        "the exported document still speaks to the app it left: {exported}",
    );
    // The link the author wrote is content, and survives as content.
    assert!(exported.contains("href=\"https://example.com/page\""));
}

/// A placeholder card is a button while axiomd is showing it. In a file that axiomd
/// will never see again it is a note about a picture, and pressing it does nothing —
/// so it is not a button any more.
#[test]
fn a_remote_image_exports_as_a_card_with_nothing_to_press() {
    let exported = export("![Far](https://cdn.example.com/far.png)\n", "notes");

    assert!(
        exported.contains("cdn.example.com"),
        "the reader is no longer told what the missing picture was: {exported}",
    );
    assert!(
        !exported.contains("Load image") && !exported.contains("Load all"),
        "the exported document offers a button that cannot do anything: {exported}",
    );
    assert!(
        !exported.contains("<a class=\"remote-image\""),
        "the placeholder is still a link: {exported}",
    );
}

/// What the file is called when it is opened, and what a PDF made from it is called
/// in its metadata: the document's own name for itself, and the file name only when
/// it has none.
#[test]
fn an_exported_document_is_titled_the_way_the_document_titles_itself() {
    let from_front_matter = export(
        "---\ntitle: The Real Title\nauthor: Someone\n---\n\n# A Heading\n\nText.\n",
        "notes",
    );
    assert!(
        from_front_matter.contains("<title>The Real Title</title>"),
        "{from_front_matter}",
    );

    let from_heading = export("# A Heading\n\nText.\n", "notes");
    assert!(
        from_heading.contains("<title>A Heading</title>"),
        "{from_heading}"
    );

    let from_the_file = export("Just some text.\n", "notes");
    assert!(
        from_the_file.contains("<title>notes</title>"),
        "{from_the_file}"
    );

    let hostile = export(
        "---\ntitle: \"</title><script>x</script>\"\n---\n\ntext\n",
        "notes",
    );
    assert!(
        !hostile.contains("<script>"),
        "a document's own title escaped into markup: {hostile}",
    );
}

/// Owner ruling (2026-08-02): an exported document is light, whatever the machine it
/// is opened on prefers. Nothing in it may switch to a dark palette.
#[test]
fn an_exported_document_is_light_whoever_opens_it() {
    let exported = export("# Notes\n\n`code`\n", "notes");

    assert!(
        !exported.contains("prefers-color-scheme"),
        "the exported document still follows the reader's machine into dark: {exported}",
    );
    assert!(
        exported.contains("color-scheme: light;"),
        "the exported document does not declare itself light: {exported}",
    );
}

/// The page the reader is looking at is the page that gets printed, so it has to say
/// what it is called: that name is the print job's, and the PDF's metadata title.
#[test]
fn the_document_on_screen_says_what_it_is_called() {
    let parsed = parse("# Release Notes\n\nText.\n");
    let rendered = axiomd_render::render(&parsed, "notes");

    assert!(
        rendered.html().contains("<title>Release Notes</title>"),
        "{}",
        rendered.html(),
    );
}

/// Print is the one medium where the reader's colour scheme is not theirs to choose:
/// paper is white. The dark palette must therefore be unreachable from print — not
/// merely overridden later in the file, which the next stylesheet edit would undo.
#[test]
fn nothing_dark_can_reach_paper() {
    let stylesheet = axiomd_render::stylesheet();

    let mut at = 0;
    let mut found = 0;
    while let Some(offset) = stylesheet[at..].find("prefers-color-scheme: dark") {
        let start = at + offset;
        let query = &stylesheet[start.saturating_sub(40)..start];
        assert!(
            query.contains("screen and"),
            "a dark palette applies to every medium, paper included: ...{query}",
        );
        found += 1;
        at = start + 1;
    }
    assert!(found >= 2, "the dark palettes have gone missing entirely");
}

/// What the print stylesheet is for, asserted where it is written. What it does to a
/// real page is asserted on the PDF itself, in the app's print suite.
#[test]
fn the_print_stylesheet_sets_the_page_up_and_spells_out_link_addresses() {
    let stylesheet = axiomd_render::stylesheet();
    let print = stylesheet
        .split("@media print")
        .nth(1)
        .expect("the stylesheet has a print block");

    assert!(print.contains("@page"), "print has no page setup: {print}");
    assert!(
        print.contains("attr(href)"),
        "a printed link no longer says where it goes: {print}",
    );
    assert!(
        print.contains("remote-banner"),
        "app chrome still prints: {print}",
    );
}
