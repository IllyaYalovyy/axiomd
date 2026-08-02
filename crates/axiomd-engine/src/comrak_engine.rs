//! The comrak engine.
//!
//! This is the only module in the crate allowed to name a comrak type; see
//! `tests/public_api.rs`, which holds that line. Everything comrak-shaped —
//! its option struct, its arena, its AST — is consumed here and converted into the
//! boundary vocabulary before anything else in axiomd can see it.

use std::borrow::Cow;
use std::ops::Range;

use comrak::arena_tree::NodeEdge;
use comrak::nodes::{
    AlertType, AstNode, ListType, NodeValue, Sourcepos, TableAlignment as ComrakAlignment,
};
use comrak::{Arena, Options, parse_document};

use crate::boundary::{
    Alignment, Callout, CalloutKind, EngineId, Event, Extension, Extensions, MarkdownEngine,
    Parsed, Span, SpannedEvent, Tag, TagEnd,
};

/// axiomd's first markdown engine, backed by comrak.
///
/// Stateless and cheap to construct; safe to share across windows and worker
/// threads (per-window isolation depends on engines holding no document state).
#[derive(Debug, Clone, Copy, Default)]
pub struct ComrakEngine;

impl ComrakEngine {
    /// This engine's identifier, usable without constructing it.
    pub const ID: EngineId = EngineId::new("comrak");

    /// Creates the engine.
    pub const fn new() -> Self {
        Self
    }
}

impl MarkdownEngine for ComrakEngine {
    fn id(&self) -> EngineId {
        Self::ID
    }

    fn capabilities(&self) -> Extensions {
        Extensions::FULL
    }

    fn parse<'a>(&self, source: &'a str, extensions: Extensions) -> Parsed<'a> {
        let enabled = extensions.intersection(self.capabilities());
        let arena = Arena::new();
        let root = parse_document(&arena, source, &options(enabled));
        Walk::new(source).run(root)
    }
}

/// Translates the requested extension set into comrak's option struct.
fn options(enabled: Extensions) -> Options<'static> {
    let mut options = Options::default();
    let ext = &mut options.extension;
    ext.table = enabled.contains(Extension::Tables);
    ext.tasklist = enabled.contains(Extension::TaskLists);
    ext.strikethrough = enabled.contains(Extension::Strikethrough);
    ext.autolink = enabled.contains(Extension::Autolinks);
    ext.footnotes = enabled.contains(Extension::Footnotes);
    ext.math_dollars = enabled.contains(Extension::Math);
    ext.math_code = enabled.contains(Extension::Math);
    ext.wikilinks_title_after_pipe = enabled.contains(Extension::WikiLinks);
    ext.alerts = enabled.contains(Extension::Callouts);
    if enabled.contains(Extension::FrontMatter) {
        ext.front_matter_delimiter = Some("---".to_string());
    }
    options
}

/// Turns one comrak AST into the boundary event stream.
struct Walk<'a> {
    source: &'a str,
    /// Byte offset of the first character of each line, 0-based index by line - 1.
    line_starts: Vec<usize>,
    events: Vec<SpannedEvent<'a>>,
    front_matter: Option<&'a str>,
    /// Column alignments of every table currently open, innermost last.
    table_alignments: Vec<Vec<Alignment>>,
    /// Index of the cell being entered in the innermost open table row.
    cell_index: usize,
    /// Span of every open container, innermost last, so a `TagEnd` reports exactly
    /// the span its `Tag` reported.
    open: Vec<Span>,
    /// Byte range of every open *block*, innermost last. Everything inside a block is
    /// clamped into it, which is what makes the nesting invariant unconditional.
    blocks: Vec<Range<usize>>,
}

impl<'a> Walk<'a> {
    fn new(source: &'a str) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(source.match_indices('\n').map(|(i, _)| i + 1));
        Self {
            source,
            line_starts,
            events: Vec::new(),
            front_matter: None,
            table_alignments: Vec::new(),
            cell_index: 0,
            open: Vec::new(),
            blocks: Vec::new(),
        }
    }

    fn run<'t>(mut self, root: &'t AstNode<'t>) -> Parsed<'a> {
        for edge in root.traverse() {
            match edge {
                NodeEdge::Start(node) => self.enter(node),
                NodeEdge::End(node) => self.leave(node),
            }
        }
        Parsed::new(self.events, self.front_matter)
    }

    /// Byte offset of a 1-based line/column pair, clamped into the source.
    fn offset(&self, line: usize, column: usize) -> usize {
        let base = self
            .line_starts
            .get(line.saturating_sub(1))
            .copied()
            .unwrap_or(self.source.len());
        base.saturating_add(column.saturating_sub(1))
            .min(self.source.len())
    }

    /// Converts comrak's inclusive line/column pair into a byte span.
    ///
    /// comrak reports columns as 1-based UTF-8 byte offsets within the line, with the
    /// end column pointing at the node's last character. The exclusive end is
    /// therefore the next character boundary after it.
    fn span(&self, sourcepos: Sourcepos) -> Span {
        let mut start = self.offset(sourcepos.start.line, sourcepos.start.column);
        while start > 0 && !self.source.is_char_boundary(start) {
            start -= 1;
        }

        let last = self.offset(sourcepos.end.line, sourcepos.end.column);
        let mut end = last.saturating_add(1).min(self.source.len());
        while end < self.source.len() && !self.source.is_char_boundary(end) {
            end += 1;
        }
        let end = end.max(start);

        Span {
            range: start..end,
            line: self.line_of(start),
        }
    }

    /// 1-based line number containing a byte offset.
    fn line_of(&self, offset: usize) -> u32 {
        self.line_starts.partition_point(|&start| start <= offset) as u32
    }

    /// Confines a span to the innermost open block.
    ///
    /// comrak occasionally reports a child that overshoots its parent — a list item
    /// that swallows the blank line after the list ends, or the phantom cell GFM adds
    /// to a short table row. Outline, scroll sync and search all assume a child's
    /// source lies inside its parent's, so the boundary makes that true rather than
    /// passing the inconsistency on.
    fn clamped(&self, span: Span) -> Span {
        let Some(parent) = self.blocks.last() else {
            return span;
        };
        let start = span.range.start.clamp(parent.start, parent.end);
        let end = span.range.end.clamp(start, parent.end);
        if start == span.range.start && end == span.range.end {
            return span;
        }
        Span {
            range: start..end,
            line: self.line_of(start),
        }
    }

    /// Widens a span leftwards over the whitespace that precedes it on its line.
    ///
    /// An indented code block *is* its indentation: comrak's position points at the
    /// first content character, so slicing by it would yield text that no longer
    /// parses as code. Stopping at the first non-whitespace byte keeps the span
    /// inside any container marker (`>`, `-`) on the same line.
    fn extended_over_indent(&self, span: Span) -> Span {
        let bytes = self.source.as_bytes();
        let mut start = span.range.start;
        while start > 0 && matches!(bytes[start - 1], b' ' | b'\t') {
            start -= 1;
        }
        Span {
            range: start..span.range.end.max(start),
            line: self.line_of(start),
        }
    }

    fn push(&mut self, event: Event<'a>, span: Span) {
        self.events.push(SpannedEvent { event, span });
    }

    fn enter(&mut self, node: &AstNode<'_>) {
        let ast = node.data();
        let mut span = self.span(ast.sourcepos);
        if let NodeValue::CodeBlock(code) = &ast.value
            && !code.fenced
        {
            span = self.extended_over_indent(span);
        }
        let span = self.clamped(span);

        if let Some((block, _)) = closing(&ast.value) {
            self.open.push(span.clone());
            if block {
                self.blocks.push(span.range.clone());
            }
        }

        match &ast.value {
            NodeValue::Document => {}

            NodeValue::FrontMatter(_) => {
                self.front_matter = Some(&self.source[span.range.clone()]);
            }

            NodeValue::Paragraph => self.push(Event::Start(Tag::Paragraph), span),
            NodeValue::Heading(heading) => self.push(
                Event::Start(Tag::Heading {
                    level: heading.level,
                }),
                span,
            ),
            NodeValue::BlockQuote | NodeValue::MultilineBlockQuote(_) => {
                self.push(Event::Start(Tag::BlockQuote { callout: None }), span)
            }
            NodeValue::Alert(alert) => {
                let callout = Callout {
                    kind: callout_kind(alert.alert_type),
                    title: alert.title.as_ref().map(|t| Cow::Owned(t.clone())),
                };
                self.push(
                    Event::Start(Tag::BlockQuote {
                        callout: Some(callout),
                    }),
                    span,
                );
            }
            NodeValue::CodeBlock(code) => {
                let info = code.info.trim();
                let (language, meta) = match info.split_once(char::is_whitespace) {
                    Some((language, meta)) => (language, meta.trim()),
                    None => (info, ""),
                };
                self.push(
                    Event::Start(Tag::CodeBlock {
                        language: (!info.is_empty()).then(|| Cow::Owned(language.to_string())),
                        meta: (!meta.is_empty()).then(|| Cow::Owned(meta.to_string())),
                        fenced: code.fenced,
                    }),
                    span.clone(),
                );
                // The literal is verbatim source, but comrak strips fence indentation
                // from it, so it carries the block's span rather than a slice that
                // might not match byte for byte.
                self.push(Event::Text(Cow::Owned(code.literal.clone())), span);
            }
            NodeValue::List(list) => self.push(
                Event::Start(Tag::List {
                    start: (list.list_type == ListType::Ordered).then_some(list.start as u64),
                    tight: list.tight,
                }),
                span,
            ),
            NodeValue::Item(_) => self.push(Event::Start(Tag::Item { task: None }), span),
            NodeValue::TaskItem(task) => self.push(
                Event::Start(Tag::Item {
                    task: Some(task.symbol.is_some()),
                }),
                span,
            ),
            NodeValue::FootnoteDefinition(definition) => self.push(
                Event::Start(Tag::FootnoteDefinition {
                    label: Cow::Owned(definition.name.clone()),
                }),
                span,
            ),
            NodeValue::Table(table) => {
                let alignments: Vec<Alignment> =
                    table.alignments.iter().copied().map(alignment).collect();
                self.table_alignments.push(alignments.clone());
                self.push(Event::Start(Tag::Table { alignments }), span);
            }
            NodeValue::TableRow(header) => {
                self.cell_index = 0;
                let tag = if *header {
                    Tag::TableHead
                } else {
                    Tag::TableRow
                };
                self.push(Event::Start(tag), span);
            }
            NodeValue::TableCell => {
                let alignment = self
                    .table_alignments
                    .last()
                    .and_then(|a| a.get(self.cell_index))
                    .copied()
                    .unwrap_or_default();
                self.cell_index += 1;
                self.push(Event::Start(Tag::TableCell { alignment }), span);
            }
            NodeValue::ThematicBreak => self.push(Event::ThematicBreak, span),
            NodeValue::HtmlBlock(html) => {
                self.push(Event::HtmlBlock(Cow::Owned(html.literal.clone())), span)
            }

            // Cloning a `Cow` keeps a borrow a borrow, and `Cow<'static, _>` is
            // usable wherever `Cow<'a, _>` is, so short literal runs cost nothing.
            NodeValue::Text(text) => self.push(Event::Text(text.clone()), span),
            NodeValue::SoftBreak => self.push(Event::SoftBreak, span),
            NodeValue::LineBreak => self.push(Event::HardBreak, span),
            NodeValue::Code(code) => self.push(Event::Code(Cow::Owned(code.literal.clone())), span),
            NodeValue::HtmlInline(html) => {
                self.push(Event::InlineHtml(Cow::Owned(html.clone())), span)
            }
            NodeValue::Emph => self.push(Event::Start(Tag::Emphasis), span),
            NodeValue::Strong => self.push(Event::Start(Tag::Strong), span),
            NodeValue::Strikethrough => self.push(Event::Start(Tag::Strikethrough), span),
            NodeValue::Link(link) => self.push(
                Event::Start(Tag::Link {
                    url: Cow::Owned(link.url.clone()),
                    title: Cow::Owned(link.title.clone()),
                }),
                span,
            ),
            NodeValue::Image(link) => self.push(
                Event::Start(Tag::Image {
                    url: Cow::Owned(link.url.clone()),
                    title: Cow::Owned(link.title.clone()),
                }),
                span,
            ),
            NodeValue::WikiLink(link) => self.push(
                Event::Start(Tag::WikiLink {
                    target: Cow::Owned(link.url.clone()),
                }),
                span,
            ),
            NodeValue::FootnoteReference(reference) => self.push(
                Event::FootnoteReference(Cow::Owned(reference.name.clone())),
                span,
            ),
            NodeValue::Math(math) => self.push(
                Event::Math {
                    display: math.display_math,
                    latex: Cow::Owned(math.literal.clone()),
                },
                span,
            ),

            // Constructs whose comrak extensions axiomd never enables. Their children
            // are still walked, so the document degrades to plain markdown rather
            // than losing content.
            _ => {}
        }
    }

    fn leave(&mut self, node: &AstNode<'_>) {
        let ast = node.data();
        let Some((block, end)) = closing(&ast.value) else {
            return;
        };
        let span = self.open.pop().expect("container closed without opening");
        if block {
            self.blocks.pop();
        }
        if matches!(ast.value, NodeValue::Table(_)) {
            self.table_alignments.pop();
        }
        self.push(Event::End(end), span);
    }
}

/// For a node that opens a container: whether it is a block, and the event that
/// closes it. `None` for leaves and for constructs axiomd does not model.
///
/// This is the single source of truth for what `enter` pushes and `leave` pops; the
/// two cannot drift apart.
fn closing(value: &NodeValue) -> Option<(bool, TagEnd)> {
    let closing = match value {
        NodeValue::Paragraph => (true, TagEnd::Paragraph),
        NodeValue::Heading(heading) => (true, TagEnd::Heading(heading.level)),
        NodeValue::BlockQuote | NodeValue::MultilineBlockQuote(_) | NodeValue::Alert(_) => {
            (true, TagEnd::BlockQuote)
        }
        NodeValue::CodeBlock(_) => (true, TagEnd::CodeBlock),
        NodeValue::List(list) => (
            true,
            TagEnd::List {
                ordered: list.list_type == ListType::Ordered,
            },
        ),
        NodeValue::Item(_) | NodeValue::TaskItem(_) => (true, TagEnd::Item),
        NodeValue::FootnoteDefinition(_) => (true, TagEnd::FootnoteDefinition),
        NodeValue::Table(_) => (true, TagEnd::Table),
        NodeValue::TableRow(header) => (
            true,
            if *header {
                TagEnd::TableHead
            } else {
                TagEnd::TableRow
            },
        ),
        NodeValue::TableCell => (true, TagEnd::TableCell),
        NodeValue::Emph => (false, TagEnd::Emphasis),
        NodeValue::Strong => (false, TagEnd::Strong),
        NodeValue::Strikethrough => (false, TagEnd::Strikethrough),
        NodeValue::Link(_) => (false, TagEnd::Link),
        NodeValue::Image(_) => (false, TagEnd::Image),
        NodeValue::WikiLink(_) => (false, TagEnd::WikiLink),
        _ => return None,
    };
    Some(closing)
}

fn alignment(alignment: ComrakAlignment) -> Alignment {
    match alignment {
        ComrakAlignment::None => Alignment::None,
        ComrakAlignment::Left => Alignment::Left,
        ComrakAlignment::Center => Alignment::Center,
        ComrakAlignment::Right => Alignment::Right,
    }
}

fn callout_kind(alert: AlertType) -> CalloutKind {
    match alert {
        AlertType::Note => CalloutKind::Note,
        AlertType::Tip => CalloutKind::Tip,
        AlertType::Important => CalloutKind::Important,
        AlertType::Warning => CalloutKind::Warning,
        AlertType::Caution => CalloutKind::Caution,
    }
}
