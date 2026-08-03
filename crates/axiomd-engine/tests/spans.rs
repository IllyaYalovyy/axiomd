//! Source spans are load-bearing.
//!
//! Outline navigation, scroll sync, live-reload anchor preservation and search
//! highlighting all map through `Span`, so a span regression breaks four features at
//! once. These properties are checked exhaustively over both vendored spec suites —
//! 1324 real documents covering every construct the specs define — rather than over
//! generated input, so they are deterministic and cover the pathological cases the
//! spec authors already thought of.
//!
//! Every property runs over **every registered engine** (issue #17): a second engine
//! that got spans wrong would break outline, scroll sync, search and live reload the
//! moment a reader selected it, and none of those four features would say which engine
//! was at fault.

mod support;

use axiomd_engine::{Event, Extensions, MarkdownEngine, Span, SpannedEvent, Tag, TagEnd};
use support::load_examples;

/// The engines the span properties are checked against.
fn engines() -> &'static [&'static dyn MarkdownEngine] {
    let engines = axiomd_engine::engines();
    assert!(
        engines.len() >= 2,
        "the span suite is not covering a second engine (issue #17)",
    );
    engines
}

/// Every document in both suites, as `(label, markdown)`.
fn corpus() -> Vec<(String, String)> {
    let mut docs = Vec::new();
    for file in ["commonmark-0.31.2.spec.txt", "gfm-0.29.spec.txt"] {
        for example in load_examples(file) {
            docs.push((format!("{file}#{}", example.number), example.markdown));
        }
    }
    docs
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

/// The tag sequence of a parse, ignoring spans and inline payloads — enough to tell
/// whether re-parsing a slice reproduced the same block structure.
fn block_shape(events: &[SpannedEvent<'_>]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match &e.event {
            Event::Start(tag) if is_block(tag) => Some(format!("{tag:?}")),
            _ => None,
        })
        .collect()
}

#[test]
fn every_span_slices_the_source() {
    let mut checked = 0usize;
    for engine in engines() {
        for (document, source) in corpus() {
            let label = format!("{} {document}", engine.id());
            let parsed = engine.parse(&source, Extensions::FULL);
            for SpannedEvent { event, span } in parsed.events() {
                let Span { range, line } = span;
                assert!(
                    range.start <= range.end && range.end <= source.len(),
                    "{label}: {event:?} has out-of-bounds span {span:?} for a {}-byte source",
                    source.len()
                );
                assert!(
                    source.is_char_boundary(range.start) && source.is_char_boundary(range.end),
                    "{label}: {event:?} has a span {span:?} that splits a character"
                );
                let expected_line = source[..range.start].matches('\n').count() + 1;
                assert_eq!(
                    *line as usize, expected_line,
                    "{label}: {event:?} reports line {line} but its span starts on line \
                     {expected_line}"
                );
                checked += 1;
            }
        }
    }
    assert!(
        checked > 16_000,
        "only {checked} events checked; suite is not exercising every engine"
    );
}

#[test]
fn block_spans_nest_inside_their_parents() {
    let mut checked = 0usize;
    for engine in engines() {
        for (document, source) in corpus() {
            let label = format!("{} {document}", engine.id());
            let parsed = engine.parse(&source, Extensions::FULL);
            let mut stack: Vec<(String, Span)> = Vec::new();
            for SpannedEvent { event, span } in parsed.events() {
                match event {
                    Event::Start(tag) if is_block(tag) => {
                        if let Some((parent_tag, parent)) = stack.last() {
                            assert!(
                                parent.range.start <= span.range.start
                                    && span.range.end <= parent.range.end,
                                "{label}: {tag:?} span {span:?} escapes its parent \
                                 {parent_tag} span {parent:?}"
                            );
                            checked += 1;
                        }
                        stack.push((format!("{tag:?}"), span.clone()));
                    }
                    Event::End(end) if is_block_end(end) => {
                        stack.pop();
                    }
                    _ => {}
                }
            }
            assert!(stack.is_empty(), "{label}: unbalanced block events");
        }
    }
    assert!(
        checked > 1_600,
        "only {checked} nested blocks checked; suite is not exercising every engine"
    );
}

/// The strongest form of "the span is the block": cutting a top-level block out of
/// the document by its span and parsing that slice on its own reproduces the same
/// block structure.
#[test]
fn top_level_block_spans_reparse_to_the_same_blocks() {
    let mut checked = 0usize;
    for engine in engines() {
        for (document, source) in corpus() {
            let label = format!("{} {document}", engine.id());
            let parsed = engine.parse(&source, Extensions::FULL);
            let events = parsed.events();

            let mut depth = 0usize;
            let mut top_level_start: Option<usize> = None;
            for (index, SpannedEvent { event, .. }) in events.iter().enumerate() {
                match event {
                    Event::Start(tag) if is_block(tag) => {
                        if depth == 0 {
                            top_level_start = Some(index);
                        }
                        depth += 1;
                    }
                    Event::End(end) if is_block_end(end) => {
                        depth -= 1;
                        if depth == 0 {
                            let start = top_level_start.take().expect("block end without start");
                            let block = &events[start..=index];
                            let slice = &source[events[start].span.range.clone()];
                            let reparsed = engine.parse(slice, Extensions::FULL);
                            assert_eq!(
                                block_shape(reparsed.events()),
                                block_shape(block),
                                "{label}: slicing {:?} by its span {:?} does not re-parse to \
                                 the same blocks\nslice: {slice:?}",
                                events[start].event,
                                events[start].span,
                            );
                            checked += 1;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    assert!(
        checked > 2_000,
        "only {checked} top-level blocks checked; suite is not exercising every engine"
    );
}

/// A fenced code block's literal content is verbatim source, so it must appear
/// inside the block's own span.
#[test]
fn code_block_content_lies_inside_the_block_span() {
    let source = "intro\n\n```rust\nlet x = 1;\nlet y = 2;\n```\n\noutro\n";
    for engine in engines() {
        let parsed = engine.parse(source, Extensions::FULL);
        let events = parsed.events();

        let start = events
            .iter()
            .position(|e| matches!(e.event, Event::Start(Tag::CodeBlock { .. })))
            .unwrap_or_else(|| panic!("{}: no code block parsed", engine.id()));
        let block = &source[events[start].span.range.clone()];
        assert_eq!(
            block,
            "```rust\nlet x = 1;\nlet y = 2;\n```",
            "{}",
            engine.id()
        );

        let Event::Text(literal) = &events[start + 1].event else {
            panic!(
                "{}: code block content is not a text event: {:?}",
                engine.id(),
                events[start + 1]
            );
        };
        assert_eq!(
            literal.as_ref(),
            "let x = 1;\nlet y = 2;\n",
            "{}",
            engine.id()
        );
        assert!(
            block.contains("let x = 1;\nlet y = 2;\n"),
            "{}",
            engine.id()
        );
        assert_eq!(events[start].span.line, 3, "{}", engine.id());
        let content = &events[start + 1].span.range;
        let outer = &events[start].span.range;
        assert!(
            outer.start <= content.start && content.end <= outer.end,
            "{}: the literal's span {content:?} escapes the block's {outer:?}",
            engine.id(),
        );
    }
}

/// Position preservation depends on spans surviving multi-byte text: a span must not
/// drift by the difference between byte and character counts.
#[test]
fn spans_are_byte_accurate_across_multibyte_text() {
    let source = "# Größe — 好\n\nnächste Zeile\n";
    for engine in engines() {
        let parsed = engine.parse(source, Extensions::FULL);
        let events = parsed.events();

        let heading = events
            .iter()
            .find(|e| matches!(e.event, Event::Start(Tag::Heading { .. })))
            .unwrap_or_else(|| panic!("{}: no heading parsed", engine.id()));
        assert_eq!(
            source[heading.span.range.clone()].trim_end(),
            "# Größe — 好",
            "{}",
            engine.id(),
        );
        assert_eq!(heading.span.line, 1, "{}", engine.id());

        let paragraph = events
            .iter()
            .find(|e| matches!(e.event, Event::Start(Tag::Paragraph)))
            .unwrap_or_else(|| panic!("{}: no paragraph parsed", engine.id()));
        assert_eq!(
            source[paragraph.span.range.clone()].trim_end(),
            "nächste Zeile",
            "{}",
            engine.id(),
        );
        assert_eq!(paragraph.span.line, 3, "{}", engine.id());
    }
}
