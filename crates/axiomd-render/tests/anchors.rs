//! The anchor map, which four features ride on.
//!
//! Outline navigation, scroll sync, search highlighting and live-reload position
//! preservation all map a source line to a rendered block through `data-line` and
//! [`Rendered::anchors`]. The two must agree with each other and with the parse, so
//! the expectations here are derived from the engine's event stream — never from the
//! renderer's own bookkeeping.

mod support;

use std::ops::Range;

use axiomd_engine::{Event, SpannedEvent, Tag, TagEnd};
use support::{fixtures, parse, render};

#[test]
fn every_top_level_block_is_anchored_to_the_source_it_came_from() {
    let mut checked = 0usize;
    for (name, source) in fixtures() {
        let expected = top_level_blocks(&source);
        let rendered = render(&source);
        let actual: Vec<(u32, Range<usize>)> = rendered
            .anchors()
            .iter()
            .map(|anchor| (anchor.line, anchor.source.clone()))
            .collect();
        assert_eq!(
            actual, expected,
            "{name}: the anchor map does not match the document's top-level blocks"
        );
        assert!(
            !expected.is_empty(),
            "{name}: fixture has no top-level blocks"
        );
        checked += expected.len();
    }
    assert!(
        checked > 40,
        "only {checked} anchors checked; the fixtures are not exercising the pipeline"
    );
}

#[test]
fn anchor_lines_increase_strictly_and_name_the_line_their_source_starts_on() {
    for (name, source) in fixtures() {
        let rendered = render(&source);
        let mut previous = 0;
        for anchor in rendered.anchors() {
            assert!(
                anchor.line > previous,
                "{name}: anchor line {} does not follow {previous}",
                anchor.line
            );
            previous = anchor.line;

            assert!(
                anchor.source.end <= source.len() && anchor.source.start < anchor.source.end,
                "{name}: anchor at line {} has an unusable range {:?}",
                anchor.line,
                anchor.source
            );
            let line = source[..anchor.source.start].matches('\n').count() + 1;
            assert_eq!(
                anchor.line as usize, line,
                "{name}: anchor claims line {} but its source starts on line {line}",
                anchor.line
            );
        }
    }
}

#[test]
fn the_document_carries_exactly_the_anchor_map_as_data_line_attributes() {
    for (name, source) in fixtures() {
        let rendered = render(&source);
        let expected: Vec<u32> = rendered.anchors().iter().map(|a| a.line).collect();
        assert_eq!(
            data_lines(rendered.html()),
            expected,
            "{name}: the document's data-line attributes are not the anchor map"
        );
    }
}

/// The non-happy path for anchoring: a document whose blocks are all nested inside
/// one container has exactly one anchor, not one per inner block.
#[test]
fn blocks_nested_inside_a_container_do_not_anchor_themselves() {
    let rendered = render("> A quote.\n>\n> - one\n> - two\n>\n> ```\n> code\n> ```\n");
    assert_eq!(
        rendered
            .anchors()
            .iter()
            .map(|a| a.line)
            .collect::<Vec<_>>(),
        vec![1],
        "only the block quote itself is a top-level block"
    );
    assert_eq!(data_lines(rendered.html()), vec![1]);
}

/// Raw HTML cannot be given an anchoring wrapper without the HTML parser closing it
/// early, so it is anchored by a zero-height element in front of it. The map must
/// still be complete.
#[test]
fn raw_html_blocks_are_anchored_without_being_wrapped() {
    let source = "<div align=\"center\">\n\nInside.\n\n</div>\n";
    let rendered = render(source);

    assert_eq!(
        rendered
            .anchors()
            .iter()
            .map(|a| a.line)
            .collect::<Vec<_>>(),
        vec![1, 3, 5]
    );
    assert_eq!(data_lines(rendered.html()), vec![1, 3, 5]);
    // The wrapper still wraps: the paragraph is inside the author's div.
    assert!(
        rendered
            .html()
            .contains("<div align=\"center\">\n<p data-line=\"3\">Inside.</p>\n"),
        "the raw HTML block no longer encloses the markdown between its halves:\n{}",
        rendered.html()
    );
}

/// GFM collects footnote definitions to the foot of the document in the order they
/// are first referred to, which is not the order they were written in. A block that
/// therefore sits *after* one written below it carries no line at all: an anchor map
/// that went backwards would make `place.js` stop its walk early and land the reader
/// on a block they were never looking at.
///
/// What it must not do is lose the block. The reader still has every word of both
/// definitions, and both are still what their references link to.
#[test]
fn a_block_the_engine_moved_is_left_out_of_the_map() {
    let rendered = render(
        "# Notes\n\nA[^second] and B[^first].\n\n[^first]: Written first.\n\n\
         [^second]: Written second.\n",
    );

    assert_eq!(
        rendered
            .anchors()
            .iter()
            .map(|a| a.line)
            .collect::<Vec<_>>(),
        vec![1, 3, 7],
        "the definition written on line 5 is rendered after the one on line 7",
    );
    assert_eq!(data_lines(rendered.html()), vec![1, 3, 7]);

    let html = rendered.html();
    assert!(html.contains("Written first."), "{html}");
    assert!(html.contains("Written second."), "{html}");
    assert!(html.contains("id=\"fn-first\""), "{html}");
    assert!(
        html.find("id=\"fn-second\"") < html.find("id=\"fn-first\""),
        "the definitions are not in reference order, so this test proves nothing:\n{html}",
    );
}

/// The `data-line` values the document carries, in document order.
fn data_lines(html: &str) -> Vec<u32> {
    let mut lines = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find("data-line=\"") {
        rest = &rest[at + "data-line=\"".len()..];
        let end = rest.find('"').expect("unterminated data-line attribute");
        lines.push(rest[..end].parse().expect("data-line is a line number"));
        rest = &rest[end..];
    }
    lines
}

/// The blocks a document opens at top level, as `(line, byte range)`, taken straight
/// from the parse — minus any the engine put out of source order.
///
/// The map is in document order with strictly increasing lines, and everything that
/// navigates a document depends on it: `place.js` walks the blocks on screen and stops
/// at the first one past the line it wants, which a block reaching backwards would end
/// early. GFM footnote definitions are the case that makes this real — they are
/// collected to the foot of the document in the order they are first referred to — and
/// [`a_block_the_engine_moved_is_left_out_of_the_map`] is that case on its own.
fn top_level_blocks(source: &str) -> Vec<(u32, Range<usize>)> {
    let parsed = parse(source);
    let mut depth = 0usize;
    let mut blocks: Vec<(u32, Range<usize>)> = Vec::new();
    let anchor = |line: u32, range: Range<usize>, blocks: &mut Vec<(u32, Range<usize>)>| {
        if blocks.last().is_some_and(|(last, _)| *last >= line) {
            return;
        }
        blocks.push((line, range));
    };
    for SpannedEvent { event, span } in parsed.events() {
        match event {
            Event::Start(tag) if is_block(tag) => {
                if depth == 0 {
                    anchor(span.line, span.range.clone(), &mut blocks);
                }
                depth += 1;
            }
            Event::End(end) if is_block_end(end) => depth -= 1,
            Event::ThematicBreak | Event::HtmlBlock(_) if depth == 0 => {
                anchor(span.line, span.range.clone(), &mut blocks);
            }
            _ => {}
        }
    }
    blocks
}

fn is_block(tag: &Tag<'_>) -> bool {
    matches!(
        tag,
        Tag::Paragraph
            | Tag::Heading { .. }
            | Tag::BlockQuote { .. }
            | Tag::CodeBlock { .. }
            | Tag::List { .. }
            | Tag::Item { .. }
            | Tag::FootnoteDefinition { .. }
            | Tag::Table { .. }
            | Tag::TableHead
            | Tag::TableRow
            | Tag::TableCell { .. }
    )
}

fn is_block_end(end: &TagEnd) -> bool {
    matches!(
        end,
        TagEnd::Paragraph
            | TagEnd::Heading(_)
            | TagEnd::BlockQuote
            | TagEnd::CodeBlock
            | TagEnd::List { .. }
            | TagEnd::Item
            | TagEnd::FootnoteDefinition
            | TagEnd::Table
            | TagEnd::TableHead
            | TagEnd::TableRow
            | TagEnd::TableCell
    )
}
