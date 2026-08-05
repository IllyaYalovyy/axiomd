//! A document is untrusted input. It renders as text, never as behaviour.
//!
//! Two mechanisms have to hold together: the body is sanitised after templating, and
//! the document declares a policy that would stop anything the sanitiser somehow let
//! through. Both are asserted on what the reader would end up with — the document
//! itself — rather than on how it was produced.

mod support;

use support::render;

/// Everything a document could try, in one file, checked against the rendered
/// result. Every check here fails against the same pipeline with the sanitising step
/// removed.
#[test]
fn a_crafted_malicious_document_renders_inert() {
    let source = std::fs::read_to_string(support::golden_dir().join("malicious.md"))
        .expect("reading the malicious fixture");
    let html = render(&source).html().to_string();
    let body = body_of(&html);

    for forbidden in [
        "<script",
        "<iframe",
        "<object",
        "<embed",
        "<form",
        "<style",
        "<base",
        "<svg",
        "<link",
        "<meta",
        "javascript:",
        "document.cookie",
        "alert(",
    ] {
        assert!(
            !body.contains(forbidden),
            "the rendered body still contains {forbidden}:\n{body}"
        );
    }
    assert!(
        event_handler(body).is_none(),
        "the rendered body still carries the event handler {:?}",
        event_handler(body)
    );

    // And the document is still a document: the parts that were never dangerous
    // survive, so the checks above are not passing on an empty page.
    assert!(body.contains("<code>&lt;script&gt;</code>"));
    assert!(body.contains(r#"<a href="https://example.com/ok""#));
    assert!(body.contains("<h1 id=\"malicious\" data-line=\"1\">Malicious</h1>"));
}

/// Displaying a document fetches nothing. A remote source keeps its URL — the reader
/// can ask for it with one click later — but never as something the view will load.
#[test]
fn no_image_source_survives_that_would_fetch_from_the_network() {
    let source = "![markdown](https://example.com/a.png)\n\n\
                  <img src=\"https://example.com/b.png\" alt=\"raw html\">\n\n\
                  <img src=\"//example.com/c.png\">\n\n\
                  ![data uri](data:image/svg+xml;base64,PHN2Zy8+)\n\n\
                  ![local](assets/d.png)\n";
    let html = render(source).html().to_string();
    let body = body_of(&html);

    // The leading space keeps this from matching `data-remote-src`, which is the
    // attribute a remote source is moved into and is never loaded.
    for remote in [" src=\"https:", " src=\"http:", " src=\"//", " src=\"data:"] {
        assert!(
            !body.contains(remote),
            "the rendered body would fetch {remote}:\n{body}"
        );
    }
    assert!(
        body.contains(
            r#"<a class="remote-image" href="axiomd://request/image?src=https%3A%2F%2Fexample.com%2Fa.png" data-remote-src="https://example.com/a.png""#
        ),
        "a remote markdown image keeps its URL for the one-click load affordance:\n{body}"
    );
    assert!(
        !body.contains("<img class=\"remote-image\""),
        "a remote source became an image element, which the view would fetch:\n{body}"
    );
    assert!(
        body.contains(r#"<img src="assets/d.png" alt="local">"#),
        "a document-relative image is still displayed:\n{body}"
    );
}

/// The policy is part of the document, so it travels with it into the webview, the
/// print path and an exported file alike.
#[test]
fn the_document_declares_a_policy_that_permits_no_script_and_no_remote_content() {
    let html = render("# Anything\n").html().to_string();
    let policy = between(
        &html,
        "<meta http-equiv=\"Content-Security-Policy\" content=\"",
        "\"",
    )
    .expect("the document declares a content security policy");

    assert_eq!(
        policy,
        "default-src 'none'; img-src axiomd:; style-src axiomd:; font-src axiomd:; \
         base-uri 'none'; form-action 'none'"
    );
}

/// A face is the one thing beyond a picture and a stylesheet a document may ask the
/// application for, and `axiomd:` is the only place it may ask. Nothing in the policy
/// says `https:`, and nothing in it says `data:` either: a document on screen carries
/// no bytes of its own.
#[test]
fn the_policy_admits_a_font_only_from_the_application_itself() {
    let html = render("An equation: $x^2$.\n").html().to_string();
    let policy = between(
        &html,
        "<meta http-equiv=\"Content-Security-Policy\" content=\"",
        "\"",
    )
    .expect("the document declares a content security policy");

    assert!(policy.contains("font-src axiomd:;"), "{policy}");
    for scheme in ["https:", "http:", "data:", "'unsafe-inline'", "*"] {
        assert!(
            !policy.contains(scheme),
            "the policy of a document with an equation in it admits {scheme}: {policy}",
        );
    }
}

/// The one interactive element the pipeline emits is a task-list checkbox. A
/// document that writes its own `<input>` gets that and nothing else — no text
/// field, no file picker, no submit button in the middle of someone's prose.
#[test]
fn a_document_cannot_introduce_an_input_that_is_not_a_checkbox() {
    let html = render("<input type=\"text\" name=\"secret\" value=\"x\">\n")
        .html()
        .to_string();
    let body = body_of(&html);

    assert!(body.contains("<input type=\"checkbox\">"), "{body}");
    assert!(!body.contains("text"), "{body}");
    assert!(!body.contains("secret"), "{body}");
}

/// A foldable callout is a `<details>`, so the sanitiser had to start letting one
/// through. What it lets through is the element and the one boolean attribute that
/// says whether it starts open — and nothing else a document might hang on it.
#[test]
fn a_details_element_keeps_only_what_folding_needs() {
    let html = render(
        "<details open onclick=\"steal()\" style=\"position:fixed\" data-x=\"y\">\n\n\
         <summary onmouseover=\"steal()\">Title</summary>\n\n</details>\n",
    )
    .html()
    .to_string();
    let body = body_of(&html);

    assert!(body.contains("<details open"), "{body}");
    assert!(body.contains("<summary>Title</summary>"), "{body}");
    for forbidden in [
        "onclick",
        "onmouseover",
        "steal",
        "position:fixed",
        "data-x",
    ] {
        assert!(!body.contains(forbidden), "{forbidden} survived:\n{body}");
    }
}

/// A block image is a `<figure>` with a `<figcaption>` (issue #39), so both have to
/// survive this gate — with the `data-line` the source map rides on, and with nothing
/// a document could hang on them.
#[test]
fn a_figure_survives_with_its_caption_and_nothing_a_document_hung_on_it() {
    let pipeline = render("![A diagram](d.png)\n").html().to_string();
    let body = body_of(&pipeline);
    assert!(
        body.contains("<figure data-line=\"1\">"),
        "the pipeline's own figure did not survive sanitising:\n{body}",
    );
    assert!(
        body.contains("<figcaption>A diagram</figcaption>"),
        "the caption did not survive sanitising:\n{body}",
    );

    let written = render(
        "<figure onclick=\"steal()\" style=\"position:fixed\" data-x=\"y\" class=\"kept\">\n\n\
         <figcaption onmouseover=\"steal()\" srcset=\"https://evil.example/x\">Caption\
         </figcaption>\n\n</figure>\n",
    )
    .html()
    .to_string();
    let body = body_of(&written);

    assert!(body.contains("<figure class=\"kept\">"), "{body}");
    assert!(body.contains("<figcaption>Caption</figcaption>"), "{body}");
    for forbidden in [
        "onclick",
        "onmouseover",
        "steal",
        "position:fixed",
        "data-x",
        "srcset",
        "evil.example",
    ] {
        assert!(!body.contains(forbidden), "{forbidden} survived:\n{body}");
    }
}

/// The bundled stylesheet is bundled: rendering a document must not pull a font, an
/// image or another sheet off the network.
///
/// Two rules, and the second is what the callout icons made necessary. The sheet may
/// not `@import` anything at all, and every URL it resolves must be an `axiomd://`
/// asset the application itself answers for — checked by asking for the bytes, so a
/// rule naming a file that was never bundled fails here rather than showing the reader
/// a callout with a hole in it. A relative URL fails too: it would be a file beside the
/// sheet, and there is no such place.
///
/// A scheme written inside an attribute selector is not a fetch:
/// `a[href^="https://"]` is a test the browser runs against the document's own links,
/// and matching one fetches nothing. Those brackets — the only place in this sheet a
/// scheme may appear outside a `url()` — are stood aside before the search, rather
/// than the search being loosened.
#[test]
fn the_stylesheet_fetches_nothing_the_application_does_not_carry() {
    let stylesheet = without_attribute_selectors(axiomd_render::stylesheet());
    assert!(
        !stylesheet.contains("@import"),
        "the stylesheet imports another sheet"
    );

    let mut named = 0;
    let mut rest = stylesheet.as_str();
    while let Some(at) = rest.find("url(") {
        rest = &rest[at + "url(".len()..];
        let end = rest.find(')').unwrap_or(rest.len());
        let uri = rest[..end].trim().trim_matches(['"', '\'']).to_owned();
        rest = &rest[end..];
        named += 1;

        let path = uri.strip_prefix("axiomd://assets").unwrap_or_else(|| {
            panic!("the stylesheet fetches {uri}, which is not a bundled asset")
        });
        assert!(
            axiomd_render::asset(path).is_some(),
            "the stylesheet names {uri}, and nothing is served there",
        );
    }
    assert!(
        named >= 13,
        "only {named} assets are named by the stylesheet; the callout icons are missing",
    );

    // And nothing else in it speaks of anywhere at all.
    let without_assets = stylesheet.replace("axiomd://assets", "");
    assert!(
        !without_assets.contains("://"),
        "the stylesheet references somewhere outside the application",
    );
}

/// `css` with every `[…]` taken out.
fn without_attribute_selectors(css: &str) -> String {
    let mut kept = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(at) = rest.find('[') {
        kept.push_str(&rest[..at]);
        rest = &rest[at..];
        match rest.find(']') {
            Some(end) => rest = &rest[end + 1..],
            None => break,
        }
    }
    kept.push_str(rest);
    kept
}

/// The first `on…=` attribute in `html`, if any.
fn event_handler(html: &str) -> Option<String> {
    let mut rest = html;
    while let Some(at) = rest.find(" on") {
        rest = &rest[at + 1..];
        let name: String = rest.chars().take_while(char::is_ascii_alphabetic).collect();
        if rest[name.len()..].starts_with('=') {
            return Some(name);
        }
    }
    None
}

fn body_of(html: &str) -> &str {
    between(html, "<body>", "</body>").expect("the document has a body")
}

fn between<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let from = text.find(start)? + start.len();
    let len = text[from..].find(end)?;
    Some(&text[from..from + len])
}
