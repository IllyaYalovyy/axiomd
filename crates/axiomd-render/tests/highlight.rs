//! Code highlighting is classes, never colours.
//!
//! The palettes live in the stylesheet so that switching theme restyles a rendered
//! document without re-parsing or reloading it. A colour written into the document
//! would pin the code block to whichever theme was active when it was rendered, so
//! the absence of one is the property worth testing.

mod support;

use support::render;

#[test]
fn a_highlighted_fence_carries_classes_and_no_colour() {
    let rendered = render("```rust\nfn main() { let x = \"hi\"; }\n```\n");
    let html = rendered.html();

    assert!(
        html.contains("<span class=\"sy-storage sy-type sy-function sy-rust\">fn</span>"),
        "the keyword is not classed:\n{html}"
    );
    assert!(
        html.contains("<code class=\"language-rust\">"),
        "the fence's language is not on the code element:\n{html}"
    );
    for colour in ["style=", "color:", "#0000", "rgb("] {
        assert!(
            !html.contains(colour),
            "the document contains {colour}:\n{html}"
        );
    }
}

/// Both palettes ship, and they differ — a dark reader gets dark code, and the
/// switch is a media query rather than a re-render.
#[test]
fn the_stylesheet_carries_a_light_and_a_dark_palette() {
    let stylesheet = axiomd_render::stylesheet();
    let dark_at = stylesheet
        .rfind("@media (prefers-color-scheme: dark)")
        .expect("the stylesheet has a dark block");
    let (light, dark) = stylesheet.split_at(dark_at);

    let light_code = declaration(light, ".sy-code").expect("the light palette styles code");
    let dark_code = declaration(dark, ".sy-code").expect("the dark palette styles code");
    assert_ne!(
        light_code, dark_code,
        "both palettes paint code blocks the same"
    );
    assert!(
        light.contains(".sy-keyword") && dark.contains(".sy-keyword"),
        "a palette is missing its syntax classes"
    );
}

/// A fence naming a language nothing can parse is still a readable code block, and
/// its content is still escaped.
#[test]
fn an_unknown_language_degrades_to_plain_escaped_code() {
    let rendered = render("```wingdings\nplain & <text>\n```\n");
    let html = rendered.html();

    assert!(
        html.contains(
            "<pre class=\"sy-code\" data-line=\"1\"><code class=\"language-wingdings\">plain &amp; &lt;text&gt;\n</code></pre>"
        ),
        "an unknown fence did not degrade to plain text:\n{html}"
    );
}

/// The body of the first rule for `selector`.
fn declaration<'a>(css: &'a str, selector: &str) -> Option<&'a str> {
    let at = css.find(&format!("{selector} {{"))? + selector.len() + 2;
    let end = css[at..].find('}')?;
    Some(css[at..at + end].trim())
}
