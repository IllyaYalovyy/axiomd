//! What a document offers the reader to click, and what it refuses to fetch.
//!
//! Three things are pinned here, because between them they are the whole of issue #6
//! on the pipeline's side of the boundary:
//!
//! * every heading carries the anchor id GitHub would give it, so a link written as
//!   `guide.md#getting-started` finds the same section in axiomd as on GitHub;
//! * a remote image is a placeholder card that *is* the load button, and nothing in
//!   the document can cause a fetch before the reader presses it;
//! * a document with remote images carries one inline "load all" affordance —
//!   never a dialog.
//!
//! The slug vectors are derived from `github-slugger`, the implementation behind
//! GitHub's own heading anchors (`index.js` lowercases, strips the punctuation and
//! symbol class in `regex.js`, maps spaces to hyphens, and disambiguates repeats
//! with `-1`, `-2`, …).

mod support;

use axiomd_render::Request;
use support::render;

/// The id of the first heading rendered from `source`.
fn heading_ids(source: &str) -> Vec<String> {
    let html = render(source).html().to_string();
    let mut ids = Vec::new();
    let mut rest = html.as_str();
    while let Some(at) = rest.find("<h") {
        rest = &rest[at + 2..];
        if !rest.starts_with(|c: char| c.is_ascii_digit()) {
            continue;
        }
        let tag_end = rest.find('>').expect("an open tag ends");
        let tag = &rest[..tag_end];
        ids.push(match tag.find("id=\"") {
            Some(from) => {
                let value = &tag[from + 4..];
                value[..value.find('"').expect("a quoted id")].to_string()
            }
            None => String::new(),
        });
        rest = &rest[tag_end..];
    }
    ids
}

fn slug_of(heading: &str) -> String {
    heading_ids(&format!("# {heading}\n"))
        .pop()
        .expect("the document has a heading")
}

/// The cases that decide whether `README.md#installing-on-fedora` written on GitHub
/// still lands on the right section here.
#[test]
fn headings_carry_the_anchor_id_github_would_give_them() {
    for (heading, expected) in [
        ("Hello World", "hello-world"),
        // Punctuation is removed, not replaced: the space beside it still becomes
        // the only hyphen.
        ("Hello, World!", "hello-world"),
        ("Getting Started", "getting-started"),
        ("1. Introduction", "1-introduction"),
        ("C++ and C#", "c-and-c"),
        ("a.b.c", "abc"),
        // The two ASCII characters the slug keeps besides letters and digits.
        ("snake_case", "snake_case"),
        ("already-hyphenated", "already-hyphenated"),
        ("100% done", "100-done"),
        // A removed character between two spaces leaves both of them, and both
        // become hyphens — GitHub's slugs really do contain `--`.
        ("Emoji \u{1f389} party", "emoji--party"),
        // Letters outside ASCII are letters: lowercased and kept.
        ("\u{dc}n\u{ef}c\u{f6}d\u{e9}", "\u{fc}n\u{ef}c\u{f6}d\u{e9}"),
        (
            "\u{422}\u{435}\u{441}\u{442}",
            "\u{442}\u{435}\u{441}\u{442}",
        ),
        ("\u{65e5}\u{672c}\u{8a9e}", "\u{65e5}\u{672c}\u{8a9e}"),
    ] {
        assert_eq!(slug_of(heading), expected, "the slug of {heading:?}");
    }
}

/// Inline markup is not part of the anchor; the words are. Code spans count, image
/// labels do not — the same text GitHub slugs.
#[test]
fn a_headings_anchor_is_made_of_its_words_and_not_its_markup() {
    assert_eq!(slug_of("*Emphatic* **words**"), "emphatic-words");
    assert_eq!(slug_of("Using `render()` here"), "using-render-here");
    assert_eq!(slug_of("Logo ![alt text](logo.png) here"), "logo--here");
}

/// Two sections may be called the same thing. Their links must not be.
#[test]
fn repeated_headings_are_disambiguated_the_way_github_disambiguates_them() {
    assert_eq!(
        heading_ids("# Notes\n\n## Notes\n\n### Notes\n"),
        ["notes", "notes-1", "notes-2"],
    );
    // A heading whose own slug collides with a generated one is pushed along too,
    // rather than silently taking the other section's link.
    assert_eq!(
        heading_ids("# Notes\n\n## Notes\n\n### Notes 1\n"),
        ["notes", "notes-1", "notes-1-1"],
    );
}

/// A heading with nothing sluggable in it gets no anchor at all rather than an
/// empty one that no link could ever name.
#[test]
fn a_heading_with_no_sluggable_text_carries_no_anchor() {
    assert_eq!(heading_ids("# ...\n"), [""]);
}

/// The heading id must not cost the anchor map its `data-line`: outline, scroll
/// sync, search and live reload all still ride on it.
#[test]
fn heading_anchors_keep_their_source_line() {
    let rendered = render("# Title\n\n## Second\n");

    assert!(
        rendered
            .html()
            .contains("<h1 id=\"title\" data-line=\"1\">Title</h1>"),
        "{}",
        rendered.html(),
    );
    assert_eq!(
        rendered
            .anchors()
            .iter()
            .map(|anchor| anchor.line)
            .collect::<Vec<_>>(),
        [1, 3],
    );
}

/// D4: the placeholder IS the load button. It carries what the reader needs to
/// decide — the alt text and where the image would come from — and a link that asks
/// the app to fetch that one image.
#[test]
fn a_remote_image_renders_as_a_placeholder_that_is_its_own_load_button() {
    let html = render("![A diagram](https://cdn.example.com/a/diagram.png)\n")
        .html()
        .to_string();

    let expected = Request::LoadImage("https://cdn.example.com/a/diagram.png".to_owned()).uri();
    assert!(
        html.contains(&format!("href=\"{expected}\"")),
        "the placeholder does not link to its own load request:\n{html}",
    );
    assert!(
        html.contains("class=\"remote-image\""),
        "the placeholder is not a remote-image card:\n{html}",
    );
    assert!(html.contains("A diagram"), "the alt text is lost:\n{html}");
    assert!(
        html.contains("cdn.example.com"),
        "the reader cannot see where the image would come from:\n{html}",
    );
    assert!(
        html.contains("data-remote-src=\"https://cdn.example.com/a/diagram.png\""),
        "the placeholder does not say which image it stands for:\n{html}",
    );
}

/// Rendering a document performs no request, and leaves nothing behind that could.
#[test]
fn nothing_in_a_rendered_document_names_a_source_the_view_would_fetch() {
    let html = render(
        "![one](https://example.com/1.png)\n\n\
         ![two](http://example.com/2.png)\n\n\
         ![three](//example.com/3.png)\n\n\
         <img src=\"https://example.com/4.png\">\n\n\
         ![local](assets/5.png)\n",
    )
    .html()
    .to_string();

    for fetchable in [" src=\"https:", " src=\"http:", " src=\"//", " src=\"data:"] {
        assert!(
            !html.contains(fetchable),
            "the document would fetch {fetchable}:\n{html}",
        );
    }
    assert_eq!(
        html.matches("class=\"remote-image\"").count(),
        3,
        "every markdown remote image becomes a placeholder:\n{html}",
    );
    assert!(
        html.contains("<img src=\"assets/5.png\" alt=\"local\">"),
        "a document-relative image is still shown:\n{html}",
    );
}

/// One affordance per document, inline, never a dialog.
#[test]
fn a_document_with_remote_images_offers_to_load_them_all_inline() {
    let rendered =
        render("![one](https://example.com/1.png)\n\n![two](https://example.com/2.png)\n");
    let html = rendered.html().to_string();

    // What "all" is, in document order: the list the app works through, taken from
    // the parse rather than from whatever the page has been patched into since.
    assert_eq!(
        rendered.remote_images(),
        ["https://example.com/1.png", "https://example.com/2.png"],
    );

    assert!(
        html.contains(&format!("href=\"{}\"", Request::LoadAllImages.uri())),
        "there is no inline load-all affordance:\n{html}",
    );
    assert!(
        html.contains("remote-banner"),
        "the load-all affordance is not the inline banner:\n{html}",
    );
    assert_eq!(
        html.matches(&format!("href=\"{}\"", Request::LoadAllImages.uri()))
            .count(),
        1,
        "more than one load-all affordance:\n{html}",
    );
}

#[test]
fn a_document_with_no_remote_images_offers_nothing_to_load() {
    let html = render("# Title\n\n![local](assets/logo.png)\n")
        .html()
        .to_string();

    let rendered = render("# Title\n\n![local](assets/logo.png)\n");

    assert!(!html.contains("remote-banner"), "{html}");
    assert!(!html.contains("remote-image"), "{html}");
    assert!(rendered.remote_images().is_empty());
}

/// The requests a document can make are a closed vocabulary, and it survives the
/// round trip through the link the reader clicks — including URLs carrying the
/// characters that would otherwise end the query.
#[test]
fn a_documents_requests_survive_the_link_the_reader_clicks() {
    for url in [
        "https://example.com/a.png",
        "https://example.com/a b&c=d#e.png",
        "http://example.com/\u{e9}t\u{e9}.png",
        "//example.com/protocol-relative.png",
    ] {
        let request = Request::LoadImage(url.to_owned());
        assert_eq!(Request::from_uri(&request.uri()), Some(request.clone()));
    }

    assert_eq!(
        Request::from_uri(&Request::LoadAllImages.uri()),
        Some(Request::LoadAllImages),
    );
}

/// Anything else is not a request, so the app never mistakes an ordinary link —
/// or a link a hostile document wrote by hand — for one.
#[test]
fn nothing_but_a_request_uri_is_read_as_a_request() {
    for other in [
        "https://example.com/",
        "axiomd://doc-3/other.md",
        "axiomd://assets/axiomd.css",
        "axiomd://request/",
        "axiomd://request/image",
        "file:///etc/passwd",
        "",
    ] {
        assert_eq!(
            Request::from_uri(other),
            None,
            "{other} was read as a request"
        );
    }
}
