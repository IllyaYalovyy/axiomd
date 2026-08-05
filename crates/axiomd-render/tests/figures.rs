//! A picture alone in its paragraph is a figure, and what the author called it
//! stands under it (issue #39).
//!
//! The reference documents this application is written for put a picture in a
//! paragraph of its own and the caption in the alt text — the shape Medium, Ghost and
//! every static-site generator write. Rendered as a bare `<img>`, that caption is
//! invisible: the reader sees the diagram and never the sentence explaining it.
//!
//! What is pinned here is the whole rule and its edges:
//!
//! * a paragraph holding one picture and nothing else is a `<figure>`, captioned with
//!   the title when there is one and the alt text otherwise;
//! * the alt text stays on the `<img>` either way — a caption is not a substitute for
//!   what a screen reader says;
//! * a picture with words beside it is an inline picture and nothing changes for it;
//! * the figure is what the paragraph was, so it carries the paragraph's `data-line`
//!   and the anchor map is the same map (invariant 3);
//! * a picture the reader has not asked for and a picture a file could not carry are
//!   still their cards, inside the figure, under the same caption.

mod support;

use axiomd_render::{Folder, Picture, Plugins};
use support::{parse, render};

/// Renders `source` the way a document is shown on screen.
fn html(source: &str) -> String {
    render(source).html().to_string()
}

/// The same source as a file that has left axiomd, with `carried` answering for the
/// pictures that can travel inside it.
fn exported(source: &str, carried: &[(&str, &[u8])]) -> String {
    let parsed = parse(source);
    axiomd_render::standalone(
        &parsed,
        "fixture",
        &Plugins::builtin(&[]),
        &Folder::empty(),
        &|reference| {
            carried
                .iter()
                .find(|(name, _)| *name == reference)
                .map(|(_, bytes)| Picture {
                    bytes: bytes.to_vec(),
                    content_type: "image/png".to_owned(),
                })
        },
    )
}

/// The Medium-style block image the reference article is written in: the picture
/// alone in its paragraph, its caption in the alt text.
#[test]
fn a_picture_alone_in_its_paragraph_is_a_figure_captioned_with_its_alt_text() {
    let html = html("![Where latency accumulates](diagram.png)\n");

    assert!(
        html.contains(
            "<figure data-line=\"1\"><img src=\"diagram.png\" alt=\"Where latency accumulates\">\
             <figcaption>Where latency accumulates</figcaption></figure>"
        ),
        "the picture is not a captioned figure:\n{html}",
    );
    assert!(
        !html.contains("<p data-line=\"1\">"),
        "the block image is still a paragraph:\n{html}",
    );
}

/// The title is what the author wrote *as* a caption, so it wins — and the alt text
/// stays where a screen reader looks for it. Accessibility is not traded for a
/// caption.
#[test]
fn a_title_becomes_the_caption_and_the_alt_text_stays_on_the_picture() {
    let html = html("![A tall thin server rack](rack.png \"Figure 2: the rack\")\n");

    assert!(
        html.contains(
            "<img src=\"rack.png\" alt=\"A tall thin server rack\" title=\"Figure 2: the rack\">\
             <figcaption>Figure 2: the rack</figcaption>"
        ),
        "the title is not the caption, or the alt text did not survive it:\n{html}",
    );
}

/// A picture with nothing to say gets no caption element — an empty `<figcaption>` is
/// a gap under the picture and a row of nothing for a screen reader to read out.
#[test]
fn a_picture_with_nothing_to_say_carries_no_caption_element() {
    for source in ["![](plain.png)\n", "![   ](plain.png)\n", "![]()\n"] {
        let html = html(source);
        assert!(
            html.contains("<figure data-line=\"1\">"),
            "{source:?} is not a figure:\n{html}",
        );
        assert!(
            !html.contains("<figcaption"),
            "{source:?} rendered an empty caption:\n{html}",
        );
    }
}

/// Words beside a picture make it an inline picture: nothing about it changes, and
/// the paragraph stays a paragraph.
#[test]
fn a_picture_with_anything_beside_it_is_not_a_figure() {
    for source in [
        "Here it is: ![a diagram](d.png)\n",
        "![a diagram](d.png) — and that is that.\n",
        "![one](a.png) ![two](b.png)\n",
        "[![a diagram](d.png)](https://example.com/)\n",
        "*![a diagram](d.png)*\n",
        "![a diagram](d.png)\nand a second line.\n",
        "<span>raw</span>![a diagram](d.png)\n",
    ] {
        let html = html(source);
        assert!(
            !html.contains("<figure"),
            "{source:?} became a figure:\n{html}",
        );
        assert!(
            !html.contains("<figcaption"),
            "{source:?} grew a caption:\n{html}",
        );
        assert!(
            html.contains("<p data-line=\"1\">"),
            "{source:?} lost its paragraph:\n{html}",
        );
        assert!(
            html.contains("alt=\"a diagram\"") || html.contains("alt=\"one\""),
            "{source:?} lost the picture's alt text:\n{html}",
        );
    }
}

/// A task item's box belongs to its own words, so an item holding a picture is words
/// and a picture — not a figure that would swallow the box.
#[test]
fn a_task_item_holding_a_picture_keeps_its_box_and_is_not_a_figure() {
    let html = html("- [x] ![a diagram](d.png)\n\n- [ ] another\n");

    assert!(
        !html.contains("<figure"),
        "the item became a figure:\n{html}"
    );
    assert!(
        html.contains("<input checked=\"\" type=\"checkbox\">"),
        "the item lost its box:\n{html}",
    );
    assert!(
        html.contains("alt=\"a diagram\""),
        "the item lost its picture:\n{html}",
    );
}

/// The figure *is* the paragraph, so it carries the line the paragraph carried and
/// the anchor map — which outline navigation, scroll sync, search and live reload all
/// ride on — is the same map it was.
#[test]
fn the_figure_carries_the_line_its_paragraph_carried() {
    let rendered = render("# Title\n\n![A diagram](d.png)\n\nAfter.\n");
    let html = rendered.html().to_string();

    assert_eq!(
        rendered
            .anchors()
            .iter()
            .map(|anchor| anchor.line)
            .collect::<Vec<_>>(),
        [1, 3, 5],
        "the block image changed the anchor map",
    );
    assert!(
        html.contains("<figure data-line=\"3\">"),
        "the figure does not carry its source line:\n{html}",
    );
}

/// A figure is written wherever a paragraph is written, so a quoted picture is a
/// captioned figure inside the quote.
#[test]
fn a_picture_alone_in_a_quoted_paragraph_is_a_figure_too() {
    let html = html("> ![A quoted diagram](d.png)\n");

    assert!(
        html.contains(
            "<figure><img src=\"d.png\" alt=\"A quoted diagram\">\
             <figcaption>A quoted diagram</figcaption></figure>"
        ),
        "a quoted block image is not a captioned figure:\n{html}",
    );
    assert!(
        !html.contains("data-line") || html.contains("<blockquote data-line=\"1\">"),
        "the anchor moved off the quote and onto the figure inside it:\n{html}",
    );
}

/// A tight list item writes no paragraph at all — its picture sits in the run of text
/// the item is — so there is no block there to make a figure of.
#[test]
fn a_picture_in_a_tight_list_item_stays_where_it_is() {
    let html = html("- ![a diagram](d.png)\n- text\n");

    assert!(!html.contains("<figure"), "{html}");
    assert!(
        html.contains("<li><img src=\"d.png\" alt=\"a diagram\"></li>"),
        "the item's picture moved or changed:\n{html}",
    );
}

/// The caption is the author's text and only ever text — including the author of a
/// document that would rather it were markup. A title is the shortest way out of a
/// caption there is, so it is the one tried here.
#[test]
fn the_caption_is_text_and_cannot_be_anything_else() {
    let html = html("![alt](d.png \"</figcaption><script>alert(1)</script> a & b\")\n");

    assert!(
        html.contains(
            "<figcaption>&lt;/figcaption&gt;&lt;script&gt;alert(1)&lt;/script&gt; \
             a &amp; b</figcaption>"
        ),
        "the caption is not escaped text:\n{html}",
    );
    assert!(!html.contains("<script"), "{html}");
    assert!(
        html.matches("<figcaption").count() == 1 && html.matches("</figcaption>").count() == 1,
        "the caption did not stay one element:\n{html}",
    );
}

/// D4 composes with the caption: the card the reader has not pressed sits inside the
/// figure, the caption is already under it, and the document still fetches nothing.
/// When the picture arrives it replaces the card inside the figure, so the caption
/// does not move.
#[test]
fn a_remote_picture_keeps_its_card_inside_the_figure_and_its_caption_under_it() {
    let html = html("![A diagram](https://cdn.example.com/d.png)\n");

    assert!(
        html.contains("<figure data-line=\"1\"><a class=\"remote-image\""),
        "the placeholder card is not inside the figure:\n{html}",
    );
    assert!(
        html.contains("</a><figcaption>A diagram</figcaption></figure>"),
        "the caption is not under the card:\n{html}",
    );
    assert!(
        html.contains("data-remote-src=\"https://cdn.example.com/d.png\""),
        "the card can no longer be found when the picture arrives:\n{html}",
    );
    for fetchable in [" src=\"https:", " src=\"http:", " src=\"//"] {
        assert!(
            !html.contains(fetchable),
            "the figure would fetch {fetchable}:\n{html}",
        );
    }
}

/// A picture a file could not carry says so where it would have been — inside the
/// figure, under the same caption.
#[test]
fn a_picture_a_file_cannot_carry_is_still_captioned() {
    let html = exported(
        "![The logo](logo.png)\n\n![Gone](nowhere.png)\n",
        &[("logo.png", b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR")],
    );

    assert!(
        html.contains("<figcaption>The logo</figcaption></figure>"),
        "a carried picture lost its caption on the way into a file:\n{html}",
    );
    assert!(
        html.contains("<span class=\"remote-image\">"),
        "the missing picture is not a card:\n{html}",
    );
    assert!(
        html.contains("</span><figcaption>Gone</figcaption></figure>"),
        "the missing picture's caption is not under its card:\n{html}",
    );
}
