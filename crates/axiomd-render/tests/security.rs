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
        "default-src 'none'; img-src axiomd:; style-src axiomd:; base-uri 'none'; form-action 'none'"
    );
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

/// The bundled stylesheet is bundled: rendering a document must not pull a font, an
/// image or another sheet off the network.
#[test]
fn the_stylesheet_fetches_nothing() {
    let stylesheet = axiomd_render::stylesheet();
    for forbidden in ["@import", "url(http", "url(\"http", "url(//", "://"] {
        assert!(
            !stylesheet.contains(forbidden),
            "the stylesheet references {forbidden}"
        );
    }
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
