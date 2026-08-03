//! The pulldown-cmark engine.
//!
//! This is the only module in the crate allowed to name a pulldown-cmark type; see
//! `tests/public_api.rs`, which holds that line for every engine.
//!
//! # What this engine has to reconcile
//!
//! pulldown-cmark is a streaming parser, so three of its shapes are not the boundary's
//! and are translated here rather than leaked:
//!
//! * **Tight lists have no paragraphs.** pulldown-cmark expresses tightness by *not*
//!   emitting `Paragraph` inside a tight item; the boundary expresses it as a flag on
//!   the list, with the paragraph always there ([`Tag::List::tight`]). The walk below
//!   opens a paragraph around the inline run of a tight item and marks its list.
//! * **Task markers are events, not item state.** `- [x] done` arrives as a marker
//!   event inside the item; the boundary carries it on the item itself, because that
//!   is what a renderer needs to draw a box and what a reader presses.
//! * **An HTML block arrives line by line.** The boundary's `HtmlBlock` is one literal,
//!   so the lines are gathered.
//!
//! # Where it differs from comrak, honestly
//!
//! pulldown-cmark has no GFM extended autolinks — a bare `www.example.com` stays
//! prose — so [`Extension::Autolinks`] is not in its capability report. Everything else
//! the boundary can express, it parses. Callouts and embeds are recognised on the
//! finished stream by `obsidian.rs`, exactly as they are for comrak, so the two engines
//! read Obsidian's vocabulary the same way.

use std::borrow::Cow;
use std::ops::Range;

use pulldown_cmark::{
    Alignment as PulldownAlignment, CodeBlockKind, CowStr, Event as Emitted, HeadingLevel,
    LinkType, Options, Parser, Tag as Opened, TagEnd as Closed, TextMergeWithOffset,
};

use crate::boundary::{
    Alignment, EngineId, Event, Extension, Extensions, MarkdownEngine, Parsed, Span, SpannedEvent,
    Tag, TagEnd, Task,
};
use crate::obsidian;
use crate::source::Source;

/// axiomd's second markdown engine, backed by pulldown-cmark.
///
/// Stateless and cheap to construct; safe to share across windows and worker threads
/// (per-window isolation depends on engines holding no document state).
#[derive(Debug, Clone, Copy, Default)]
pub struct PulldownEngine;

impl PulldownEngine {
    /// This engine's identifier, usable without constructing it.
    pub const ID: EngineId = EngineId::new("pulldown-cmark");

    /// Creates the engine.
    pub const fn new() -> Self {
        Self
    }
}

impl MarkdownEngine for PulldownEngine {
    fn id(&self) -> EngineId {
        Self::ID
    }

    fn capabilities(&self) -> Extensions {
        // Everything but GFM extended autolinks, which pulldown-cmark does not
        // implement: `www.example.com` in prose stays prose. Advertising it would make
        // the capability report a wish rather than a fact.
        Extensions::COMMONMARK
            | Extension::Tables
            | Extension::TaskLists
            | Extension::Strikethrough
            | Extension::Footnotes
            | Extension::Math
            | Extension::WikiLinks
            | Extension::Callouts
            | Extension::FrontMatter
    }

    fn parse<'a>(&self, source: &'a str, extensions: Extensions) -> Parsed<'a> {
        let enabled = extensions.intersection(self.capabilities());
        let walk = Walk::new(source).run(options(enabled, source));
        let mut events = walk.events;
        // The two Obsidian shapes recognised on the finished stream, so that every
        // engine behind the boundary reads them the same way (`obsidian.rs`).
        if enabled.contains(Extension::Callouts) {
            obsidian::recognise_callouts(&mut events);
        }
        if enabled.contains(Extension::WikiLinks) {
            obsidian::recognise_embeds(&mut events, source);
        }
        Parsed::new(events, walk.front_matter)
    }
}

/// Translates the requested extension set into pulldown-cmark's option flags.
///
/// Metadata blocks are asked for only when the document opens with the delimiter.
/// pulldown-cmark recognises a `---`-fenced block anywhere, so a `---\nkey: v\n---`
/// halfway down a document would become metadata — content the reader would simply
/// stop seeing. Front matter is by definition at the front, so gating on that is what
/// keeps a document's own prose out of its metadata.
fn options(enabled: Extensions, source: &str) -> Options {
    let mut options = Options::empty();
    options.set(Options::ENABLE_TABLES, enabled.contains(Extension::Tables));
    options.set(
        Options::ENABLE_TASKLISTS,
        enabled.contains(Extension::TaskLists),
    );
    options.set(
        Options::ENABLE_STRIKETHROUGH,
        enabled.contains(Extension::Strikethrough),
    );
    options.set(
        Options::ENABLE_FOOTNOTES,
        enabled.contains(Extension::Footnotes),
    );
    options.set(Options::ENABLE_MATH, enabled.contains(Extension::Math));
    options.set(
        Options::ENABLE_WIKILINKS,
        enabled.contains(Extension::WikiLinks),
    );
    options.set(
        Options::ENABLE_YAML_STYLE_METADATA_BLOCKS,
        enabled.contains(Extension::FrontMatter) && source.starts_with("---"),
    );
    // Deliberately never on: callouts are recognised on the event stream instead, for
    // the whole Obsidian vocabulary and with the fold marker intact (`obsidian.rs`).
    options.remove(Options::ENABLE_GFM);
    options
}

/// One container pulldown-cmark has opened and the boundary has not yet closed.
struct Open {
    /// The span the opening event reported, so the closing one reports exactly it.
    span: Span,
    /// What closes it, or `None` for a container the boundary does not model as one.
    end: Option<TagEnd>,
    /// Whether it is a block, and so confines everything inside it.
    block: bool,
    kind: Kind,
}

/// What the walk still has to remember about an open container.
enum Kind {
    /// Where the list's `Start` event is, and whether any of its items held a
    /// paragraph of pulldown-cmark's own — which is what makes the list loose.
    List { at: usize, loose: bool },
    /// Where the item's `Start` event is, so a task marker can be written onto it, and
    /// the paragraph the walk opened around the item's inline run, if any.
    Item {
        at: usize,
        paragraph: Option<Synthetic>,
    },
    /// The column alignments of a table, and which cell of the current row is next.
    Table {
        alignments: Vec<Alignment>,
        cell: usize,
    },
    /// A metadata block, and whether it is the document's front matter.
    Metadata { front_matter: bool },
    /// An HTML block, whose lines are gathered into one literal.
    Html(String),
    /// Everything else.
    Plain,
}

/// A paragraph the walk opened because a tight list item's content had none.
struct Synthetic {
    /// Where its `Start` event is, so its span can be completed once its extent is
    /// known.
    at: usize,
    range: Range<usize>,
}

/// Turns one pulldown-cmark event stream into the boundary event stream.
struct Walk<'a> {
    source: Source<'a>,
    events: Vec<SpannedEvent<'a>>,
    front_matter: Option<&'a str>,
    open: Vec<Open>,
    /// Byte range of every open *block*, innermost last. Everything inside a block is
    /// clamped into it, which is what makes the nesting invariant unconditional.
    blocks: Vec<Range<usize>>,
}

impl<'a> Walk<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            source: Source::new(text),
            events: Vec::new(),
            front_matter: None,
            open: Vec::new(),
            blocks: Vec::new(),
        }
    }

    fn run(mut self, options: Options) -> Self {
        // Adjacent text runs are merged, because the boundary's vocabulary — and the
        // callout recogniser that reads it — is one text event per run of prose:
        // pulldown-cmark splits `[!NOTE]` into three.
        let parser = Parser::new_ext(self.source.text(), options).into_offset_iter();
        for (event, range) in TextMergeWithOffset::new(parser) {
            self.step(event, range);
        }
        self
    }

    fn step(&mut self, event: Emitted<'a>, range: Range<usize>) {
        match event {
            Emitted::Start(tag) => self.enter(tag, range),
            Emitted::End(tag) => self.leave(tag),
            Emitted::TaskListMarker(checked) => self.mark_task(checked, &range),
            Emitted::Html(html) => self.html_line(html, range),
            // A block, so the paragraph a tight item's prose is in ends before it.
            Emitted::Rule => {
                self.close_paragraph();
                let span = self.spanned(range);
                self.push(Event::ThematicBreak, span);
            }
            leaf => {
                let Some(event) = self.leaf(leaf) else {
                    return;
                };
                let span = self.spanned(range);
                self.inline(&span);
                self.push(event, span);
            }
        }
    }

    /// The boundary event an inline leaf becomes, or `None` when it is swallowed.
    ///
    /// Two are: the text of the document's own front matter, which is metadata and
    /// never content; and the text pulldown-cmark emits at the head of an indented HTML
    /// block, which is that block's own leading whitespace and belongs in its literal
    /// rather than beside it (probed on pulldown-cmark 0.13.4, where ` <div>` opens the
    /// block at the `<` and reports the space as an empty-range text event inside it).
    fn leaf(&mut self, event: Emitted<'a>) -> Option<Event<'a>> {
        if let Some(Open {
            kind: Kind::Html(literal),
            ..
        }) = self.open.last_mut()
        {
            if let Emitted::Text(text) = &event {
                literal.push_str(text.as_ref());
            }
            return None;
        }
        if matches!(
            self.open.last().map(|open| &open.kind),
            Some(Kind::Metadata { .. })
        ) {
            return None;
        }
        Some(match event {
            Emitted::Text(text) => Event::Text(text.into()),
            Emitted::Code(code) => Event::Code(code.into()),
            Emitted::InlineMath(latex) => Event::Math {
                display: false,
                latex: latex.into(),
            },
            Emitted::DisplayMath(latex) => Event::Math {
                display: true,
                latex: latex.into(),
            },
            Emitted::InlineHtml(html) => Event::InlineHtml(html.into()),
            Emitted::FootnoteReference(label) => Event::FootnoteReference(label.into()),
            Emitted::SoftBreak => Event::SoftBreak,
            Emitted::HardBreak => Event::HardBreak,
            // Handled before this is reached.
            _ => return None,
        })
    }

    fn enter(&mut self, tag: Opened<'a>, range: Range<usize>) {
        if !is_inline(&tag) {
            self.close_paragraph();
        }
        // pulldown-cmark's own paragraph inside an item is what makes a list loose.
        if matches!(tag, Opened::Paragraph) {
            self.mark_list_loose();
        }

        let mut span = self.spanned(range);
        if matches!(tag, Opened::CodeBlock(CodeBlockKind::Indented)) {
            // An indented code block *is* its indentation, and widening leftwards must
            // still not escape the item or quote the block sits in.
            let widened = self.source.over_indent(span);
            span = self.source.clamped(widened, self.blocks.last());
        }
        if is_inline(&tag) {
            self.inline(&span);
        }

        let at = self.events.len();
        let (end, kind) = match &tag {
            Opened::List(_) => (
                Some(TagEnd::List { ordered: false }),
                Kind::List { at, loose: false },
            ),
            Opened::Item => (
                Some(TagEnd::Item),
                Kind::Item {
                    at,
                    paragraph: None,
                },
            ),
            Opened::Table(alignments) => (
                Some(TagEnd::Table),
                Kind::Table {
                    alignments: alignments.iter().copied().map(alignment).collect(),
                    cell: 0,
                },
            ),
            Opened::MetadataBlock(_) => {
                let front_matter = span.range.start == 0 && self.front_matter.is_none();
                if front_matter {
                    self.front_matter = Some(&self.source.text()[span.range.clone()]);
                }
                (None, Kind::Metadata { front_matter })
            }
            Opened::HtmlBlock => (None, Kind::Html(String::new())),
            _ => (closing(&tag), Kind::Plain),
        };
        let block = !is_inline(&tag);
        self.open.push(Open {
            span: span.clone(),
            end,
            block,
            kind,
        });
        if block {
            self.blocks.push(span.range.clone());
        }

        if let Some(event) = self.opening(tag) {
            self.push(event, span);
        }
    }

    /// The boundary event an opening tag becomes, or `None` for a container the
    /// boundary does not model — a metadata block, or an HTML block whose literal is
    /// emitted whole when it closes.
    fn opening(&mut self, tag: Opened<'a>) -> Option<Event<'a>> {
        Some(match tag {
            Opened::Paragraph => Event::Start(Tag::Paragraph),
            Opened::Heading { level, .. } => Event::Start(Tag::Heading {
                level: heading_level(level),
            }),
            Opened::BlockQuote(_) => Event::Start(Tag::BlockQuote { callout: None }),
            Opened::CodeBlock(kind) => {
                let info = match &kind {
                    CodeBlockKind::Fenced(info) => info.as_ref().trim(),
                    CodeBlockKind::Indented => "",
                };
                let (language, meta) = match info.split_once(char::is_whitespace) {
                    Some((language, meta)) => (language, meta.trim()),
                    None => (info, ""),
                };
                Event::Start(Tag::CodeBlock {
                    language: (!info.is_empty()).then(|| Cow::Owned(language.to_owned())),
                    meta: (!meta.is_empty()).then(|| Cow::Owned(meta.to_owned())),
                    fenced: matches!(kind, CodeBlockKind::Fenced(_)),
                })
            }
            // `tight` is written when the list closes, by which time whether any item
            // held a paragraph of its own is known.
            Opened::List(start) => Event::Start(Tag::List { start, tight: true }),
            Opened::Item => Event::Start(Tag::Item { task: None }),
            Opened::FootnoteDefinition(label) => Event::Start(Tag::FootnoteDefinition {
                label: label.into(),
            }),
            Opened::Table(_) => Event::Start(Tag::Table {
                alignments: self.alignments(),
            }),
            Opened::TableHead => {
                self.next_row();
                Event::Start(Tag::TableHead)
            }
            Opened::TableRow => {
                self.next_row();
                Event::Start(Tag::TableRow)
            }
            Opened::TableCell => Event::Start(Tag::TableCell {
                alignment: self.next_cell(),
            }),
            Opened::Emphasis => Event::Start(Tag::Emphasis),
            Opened::Strong => Event::Start(Tag::Strong),
            Opened::Strikethrough => Event::Start(Tag::Strikethrough),
            Opened::Link {
                link_type,
                dest_url,
                title,
                ..
            } => match link_type {
                LinkType::WikiLink { .. } => Event::Start(Tag::WikiLink {
                    target: dest_url.into(),
                    embed: false,
                }),
                // `<someone@example.com>`: the boundary carries destinations as
                // resolved, so a reader clicking one writes an email rather than
                // following a relative path (CommonMark 0.31.2 §6.5, examples 604-605).
                LinkType::Email => Event::Start(Tag::Link {
                    url: Cow::Owned(format!("mailto:{dest_url}")),
                    title: title.into(),
                }),
                _ => Event::Start(Tag::Link {
                    url: dest_url.into(),
                    title: title.into(),
                }),
            },
            Opened::Image {
                link_type,
                dest_url,
                title,
                ..
            } => match link_type {
                // `![[target]]` — an embed, which is a reference to something axiomd
                // does not transclude (issue #12) rather than a picture.
                LinkType::WikiLink { .. } => Event::Start(Tag::WikiLink {
                    target: dest_url.into(),
                    embed: true,
                }),
                _ => Event::Start(Tag::Image {
                    url: dest_url.into(),
                    title: title.into(),
                }),
            },
            // Neither is a container of the boundary's, and both are handled where
            // they close.
            Opened::HtmlBlock | Opened::MetadataBlock(_) => return None,
            // Constructs whose pulldown-cmark options axiomd never enables. Their
            // children are still walked, so the document degrades to plain markdown
            // rather than losing content.
            _ => return None,
        })
    }

    fn leave(&mut self, tag: Closed) {
        if matches!(tag, Closed::Item) {
            self.close_paragraph();
        }
        let Some(open) = self.open.pop() else {
            return;
        };
        if open.block {
            self.blocks.pop();
        }

        match open.kind {
            Kind::List { at, loose } => {
                if let Event::Start(Tag::List { tight, .. }) = &mut self.events[at].event {
                    *tight = !loose;
                }
            }
            Kind::Metadata { front_matter } => {
                if !front_matter {
                    // Not the document's front matter, so it is prose that merely looks
                    // like it. The reader keeps every line of it.
                    let text = &self.source.text()[open.span.range.clone()];
                    self.push(Event::Start(Tag::Paragraph), open.span.clone());
                    self.push(Event::Text(Cow::Borrowed(text)), open.span.clone());
                    self.push(Event::End(TagEnd::Paragraph), open.span);
                }
                return;
            }
            Kind::Html(literal) => {
                self.push(Event::HtmlBlock(Cow::Owned(literal)), open.span);
                return;
            }
            _ => {}
        }

        if let Some(end) = open.end {
            let end = match (end, tag) {
                (TagEnd::List { .. }, Closed::List(ordered)) => TagEnd::List { ordered },
                (end, _) => end,
            };
            self.push(Event::End(end), open.span);
        }
    }

    /// Gathers one line of an open HTML block, or emits a stray one on its own.
    fn html_line(&mut self, html: CowStr<'a>, range: Range<usize>) {
        if let Some(Open {
            kind: Kind::Html(literal),
            ..
        }) = self.open.last_mut()
        {
            literal.push_str(html.as_ref());
            return;
        }
        let span = self.spanned(range);
        self.push(Event::HtmlBlock(html.into()), span);
    }

    /// Writes a task list marker onto the item it belongs to.
    ///
    /// The marker's range is the whole `[x]`, so the character between the brackets —
    /// the one a reader pressing the box rewrites — is one byte in. It is checked to
    /// really be that character rather than trusted, so an offset can never name a
    /// piece of somebody's prose (invariant 3: never a text search, and never a guess).
    fn mark_task(&mut self, checked: bool, range: &Range<usize>) {
        let marker = range.start + 1;
        if !matches!(
            self.source.text().as_bytes().get(marker),
            Some(b' ' | b'x' | b'X')
        ) {
            return;
        }
        let at = self.open.iter().rev().find_map(|open| match open.kind {
            Kind::Item { at, .. } => Some(at),
            _ => None,
        });
        if let Some(at) = at
            && let Event::Start(Tag::Item { task }) = &mut self.events[at].event
        {
            *task = Some(Task { checked, marker });
        }
    }

    /// Notes that an item held a paragraph of pulldown-cmark's own, which is how it
    /// says the enclosing list is loose.
    fn mark_list_loose(&mut self) {
        if !matches!(
            self.open.last().map(|open| &open.kind),
            Some(Kind::Item { .. })
        ) {
            return;
        }
        for open in self.open.iter_mut().rev() {
            if let Kind::List { loose, .. } = &mut open.kind {
                *loose = true;
                return;
            }
        }
    }

    /// Puts a tight item's inline content in a paragraph, because the boundary always
    /// has one and pulldown-cmark's tight items do not.
    fn inline(&mut self, span: &Span) {
        let at = self.events.len();
        let Some(Open {
            kind: Kind::Item { paragraph, .. },
            ..
        }) = self.open.last_mut()
        else {
            return;
        };
        match paragraph {
            Some(open) => {
                open.range.end = open.range.end.max(span.range.end);
                return;
            }
            None => {
                *paragraph = Some(Synthetic {
                    at,
                    range: span.range.clone(),
                })
            }
        }
        self.push(Event::Start(Tag::Paragraph), span.clone());
    }

    /// Closes the paragraph [`Walk::inline`] opened, once its extent is known.
    fn close_paragraph(&mut self) {
        let opened = match self.open.last_mut() {
            Some(Open {
                kind: Kind::Item { paragraph, .. },
                ..
            }) => paragraph.take(),
            _ => None,
        };
        let Some(opened) = opened else {
            return;
        };
        let span = Span {
            line: self.source.line_of(opened.range.start),
            range: opened.range,
        };
        self.events[opened.at].span = span.clone();
        self.push(Event::End(TagEnd::Paragraph), span);
    }

    /// The column alignments of the innermost open table.
    fn alignments(&self) -> Vec<Alignment> {
        self.open
            .iter()
            .rev()
            .find_map(|open| match &open.kind {
                Kind::Table { alignments, .. } => Some(alignments.clone()),
                _ => None,
            })
            .unwrap_or_default()
    }

    fn next_row(&mut self) {
        for open in self.open.iter_mut().rev() {
            if let Kind::Table { cell, .. } = &mut open.kind {
                *cell = 0;
                return;
            }
        }
    }

    /// The alignment of the cell being entered, and on to the next.
    fn next_cell(&mut self) -> Alignment {
        for open in self.open.iter_mut().rev() {
            if let Kind::Table { alignments, cell } = &mut open.kind {
                let alignment = alignments.get(*cell).copied().unwrap_or_default();
                *cell += 1;
                return alignment;
            }
        }
        Alignment::None
    }

    /// A pulldown-cmark range as a span confined to the block it is in.
    fn spanned(&self, range: Range<usize>) -> Span {
        let span = self.source.span(range);
        self.source.clamped(span, self.blocks.last())
    }

    fn push(&mut self, event: Event<'a>, span: Span) {
        self.events.push(SpannedEvent { event, span });
    }
}

/// Whether a tag is an inline one, and so neither a block nor something that ends the
/// paragraph a tight item's prose is in.
fn is_inline(tag: &Opened<'_>) -> bool {
    matches!(
        tag,
        Opened::Emphasis
            | Opened::Strong
            | Opened::Strikethrough
            | Opened::Superscript
            | Opened::Subscript
            | Opened::Link { .. }
            | Opened::Image { .. }
    )
}

/// The event that closes a tag the boundary models as a container.
///
/// The single source of truth for what `enter` pushes and `leave` pops, so the two
/// cannot drift apart.
fn closing(tag: &Opened<'_>) -> Option<TagEnd> {
    Some(match tag {
        Opened::Paragraph => TagEnd::Paragraph,
        Opened::Heading { level, .. } => TagEnd::Heading(heading_level(*level)),
        Opened::BlockQuote(_) => TagEnd::BlockQuote,
        Opened::CodeBlock(_) => TagEnd::CodeBlock,
        Opened::FootnoteDefinition(_) => TagEnd::FootnoteDefinition,
        Opened::TableHead => TagEnd::TableHead,
        Opened::TableRow => TagEnd::TableRow,
        Opened::TableCell => TagEnd::TableCell,
        Opened::Emphasis => TagEnd::Emphasis,
        Opened::Strong => TagEnd::Strong,
        Opened::Strikethrough => TagEnd::Strikethrough,
        Opened::Link {
            link_type: LinkType::WikiLink { .. },
            ..
        }
        | Opened::Image {
            link_type: LinkType::WikiLink { .. },
            ..
        } => TagEnd::WikiLink,
        Opened::Link { .. } => TagEnd::Link,
        Opened::Image { .. } => TagEnd::Image,
        _ => return None,
    })
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn alignment(alignment: PulldownAlignment) -> Alignment {
    match alignment {
        PulldownAlignment::None => Alignment::None,
        PulldownAlignment::Left => Alignment::Left,
        PulldownAlignment::Center => Alignment::Center,
        PulldownAlignment::Right => Alignment::Right,
    }
}
