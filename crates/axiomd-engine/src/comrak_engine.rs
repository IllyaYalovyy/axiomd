//! The comrak engine.
//!
//! This is the only module in the crate allowed to name a comrak type; see
//! `tests/public_api.rs`, which holds that line. Everything comrak-shaped —
//! its option struct, its arena, its AST — is consumed here and converted into the
//! boundary vocabulary before anything else in axiomd can see it.

use std::borrow::Cow;
use std::ops::Range;

use comrak::arena_tree::NodeEdge;
use comrak::nodes::{AstNode, ListType, NodeValue, Sourcepos, TableAlignment as ComrakAlignment};
use comrak::{Arena, Options, parse_document};

use crate::boundary::{
    Alignment, EngineId, Event, Extension, Extensions, MarkdownEngine, Parsed, Span, SpannedEvent,
    Tag, TagEnd, Task,
};
use crate::obsidian;
use crate::source::Source;

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

    fn display_name(&self) -> &'static str {
        "Comrak"
    }

    fn capabilities(&self) -> Extensions {
        Extensions::FULL
    }

    fn parse<'a>(&self, source: &'a str, extensions: Extensions) -> Parsed<'a> {
        let enabled = extensions.intersection(self.capabilities());
        let arena = Arena::new();
        let root = parse_document(&arena, source, &options(enabled));
        let walk = Walk::new(source).run(root);
        let mut events = walk.events;
        // The two Obsidian shapes comrak leaves as ordinary prose, recognised on the
        // finished stream so that every engine behind the boundary reads them the
        // same way (`obsidian.rs`).
        if enabled.contains(Extension::Callouts) {
            obsidian::recognise_callouts(&mut events);
        }
        if enabled.contains(Extension::WikiLinks) {
            obsidian::recognise_embeds(&mut events, source);
        }
        Parsed::new(events, walk.front_matter)
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
    // Deliberately never on: callouts are recognised on the event stream instead, for
    // the whole Obsidian vocabulary and with the fold marker intact (`obsidian.rs`).
    ext.alerts = false;
    if enabled.contains(Extension::FrontMatter) {
        ext.front_matter_delimiter = Some("---".to_string());
    }
    options
}

/// Turns one comrak AST into the boundary event stream.
struct Walk<'a> {
    source: Source<'a>,
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
        Self {
            source: Source::new(source),
            events: Vec::new(),
            front_matter: None,
            table_alignments: Vec::new(),
            cell_index: 0,
            open: Vec::new(),
            blocks: Vec::new(),
        }
    }

    fn run<'t>(mut self, root: &'t AstNode<'t>) -> Self {
        for edge in root.traverse() {
            match edge {
                NodeEdge::Start(node) => self.enter(node),
                NodeEdge::End(node) => self.leave(node),
            }
        }
        self
    }

    /// Converts comrak's inclusive line/column pair into a byte span.
    ///
    /// comrak reports columns as 1-based UTF-8 byte offsets within the line, with the
    /// end column pointing at the node's last character. The exclusive end is
    /// therefore the next character boundary after it, which `Source::span` finds.
    fn span(&self, sourcepos: Sourcepos) -> Span {
        let start = self
            .source
            .offset(sourcepos.start.line, sourcepos.start.column);
        let last = self.source.offset(sourcepos.end.line, sourcepos.end.column);
        self.source.span(start..last.saturating_add(1))
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
            span = self.source.over_indent(span);
        }
        let span = self.source.clamped(span, self.blocks.last());

        if let Some((block, _)) = closing(&ast.value) {
            self.open.push(span.clone());
            if block {
                self.blocks.push(span.range.clone());
            }
        }

        match &ast.value {
            NodeValue::Document => {}

            NodeValue::FrontMatter(_) => {
                self.front_matter = Some(&self.source.text()[span.range.clone()]);
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
            NodeValue::TaskItem(task) => {
                // The character between the brackets, which is what a reader pressing
                // the box rewrites. comrak reports its position separately from the
                // item's, so the offset is exact rather than searched for.
                let marker = self.source.offset(
                    task.symbol_sourcepos.start.line,
                    task.symbol_sourcepos.start.column,
                );
                self.push(
                    Event::Start(Tag::Item {
                        task: Some(Task {
                            checked: task.symbol.is_some(),
                            marker,
                        }),
                    }),
                    span,
                )
            }
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
                    embed: false,
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
