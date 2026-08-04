//! The engine boundary: the vocabulary every axiomd markdown engine speaks.
//!
//! Nothing in this module knows about any particular parser. Everything a view,
//! renderer, outline or search feature is allowed to see about a document lives
//! here.

use std::borrow::Cow;
use std::fmt;
use std::ops::{BitOr, Range};

/// Stable identifier of a markdown engine.
///
/// Used by the engine registry and by per-document engine selection; it is what a
/// preference or a document override stores, and what a menu item carries as its
/// action target. It is never what a reader is shown — that is
/// [`MarkdownEngine::display_name`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EngineId(&'static str);

impl EngineId {
    /// Names an engine. The name is persisted and carried in action targets, so treat
    /// it as stable once an engine ships.
    pub const fn new(name: &'static str) -> Self {
        Self(name)
    }

    /// The engine's name.
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for EngineId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// One syntax extension beyond strict CommonMark.
///
/// An engine advertises the ones it can parse through [`MarkdownEngine::capabilities`];
/// a caller requests a subset per parse. Anything the engine cannot parse is simply
/// left as ordinary markdown — never an error, per the best-effort rendering rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Extension {
    /// GFM pipe tables, with per-column alignment.
    Tables,
    /// GFM task list items (`- [x] done`).
    TaskLists,
    /// GFM strikethrough (`~~gone~~`).
    Strikethrough,
    /// GFM extended autolinks: bare `www.`, `http://` and email text becomes a link.
    Autolinks,
    /// Footnote references and definitions.
    Footnotes,
    /// Inline (`$x$`) and display (`$$x$$`) math spans.
    Math,
    /// `[[Wikilink]]` targets.
    WikiLinks,
    /// GitHub/Obsidian callouts: `> [!NOTE]` on a block quote.
    Callouts,
    /// Leading `---` front matter, exposed as metadata rather than rendered.
    FrontMatter,
}

impl Extension {
    /// Every extension the event vocabulary can express, in declaration order.
    pub const ALL: [Extension; 9] = [
        Extension::Tables,
        Extension::TaskLists,
        Extension::Strikethrough,
        Extension::Autolinks,
        Extension::Footnotes,
        Extension::Math,
        Extension::WikiLinks,
        Extension::Callouts,
        Extension::FrontMatter,
    ];

    const fn bit(self) -> u16 {
        1 << (self as u16)
    }
}

/// A set of [`Extension`]s.
///
/// This is axiomd's own parse-options type (RFC-001 calls it `ParseOptions`). It is
/// the set itself rather than a struct wrapping one, and it does double duty as an
/// engine's capability report: an engine parses exactly the extensions it was asked
/// for *and* advertises.
#[derive(Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Extensions(u16);

impl Extensions {
    /// Strict CommonMark: no extensions at all.
    pub const COMMONMARK: Self = Self(0);

    /// The GitHub Flavored Markdown extension set.
    pub const GFM: Self = Self(
        Extension::Tables.bit()
            | Extension::TaskLists.bit()
            | Extension::Strikethrough.bit()
            | Extension::Autolinks.bit(),
    );

    /// Everything the event vocabulary can express: GFM plus the Obsidian-facing
    /// surface (footnotes, math, wikilinks, callouts, front matter).
    pub const FULL: Self = Self(
        Self::GFM.0
            | Extension::Footnotes.bit()
            | Extension::Math.bit()
            | Extension::WikiLinks.bit()
            | Extension::Callouts.bit()
            | Extension::FrontMatter.bit(),
    );

    /// Whether `ext` is in the set.
    pub const fn contains(self, ext: Extension) -> bool {
        self.0 & ext.bit() != 0
    }

    /// The extensions present in both sets.
    pub const fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    /// The members of the set, in [`Extension::ALL`] order.
    pub fn iter(self) -> impl Iterator<Item = Extension> {
        Extension::ALL
            .into_iter()
            .filter(move |e| self.contains(*e))
    }
}

impl BitOr for Extensions {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl BitOr<Extension> for Extensions {
    type Output = Self;
    fn bitor(self, rhs: Extension) -> Self {
        Self(self.0 | rhs.bit())
    }
}

impl From<Extension> for Extensions {
    fn from(ext: Extension) -> Self {
        Self(ext.bit())
    }
}

impl fmt::Debug for Extensions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

/// Where an event came from in the source document.
///
/// Spans are load-bearing: outline navigation, scroll sync, live-reload anchor
/// preservation and search highlighting all map through them. `range` always slices
/// the source the parse was handed.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Span {
    /// Byte range into the parsed source.
    pub range: Range<usize>,
    /// 1-based line number of `range.start`.
    pub line: u32,
}

/// An [`Event`] together with the source it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct SpannedEvent<'a> {
    /// What happened.
    pub event: Event<'a>,
    /// Where in the source it happened.
    pub span: Span,
}

/// Column alignment of a table cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Alignment {
    /// No alignment was requested for this column.
    #[default]
    None,
    /// `:---`
    Left,
    /// `:-:`
    Center,
    /// `---:`
    Right,
}

/// A callout marker found on a block quote.
///
/// The kind is carried as the author wrote it rather than as a closed set of
/// variants: Obsidian's vocabulary is open, an unknown kind is a callout too, and
/// deciding what a kind *looks like* is styling — which belongs to the renderer and
/// not to the parser.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Callout<'a> {
    /// The kind between the brackets, lowercased — `note`, `tldr`, `bug`, or
    /// whatever else the author wrote.
    pub kind: Cow<'a, str>,
    /// The author's replacement title, if they wrote one after the marker.
    pub title: Option<Cow<'a, str>>,
    /// Whether the author asked for the callout to fold, and how it starts:
    /// `Some(true)` for `+` (folds, starts open), `Some(false)` for `-` (folds,
    /// starts shut), `None` for a callout that does not fold at all.
    pub fold: Option<bool>,
}

/// A task list item's checkbox, and where its state lives in the source.
///
/// The offset is what makes a checkbox something a reader can press: toggling one
/// rewrites exactly that byte, so two identical items on two lines are told apart by
/// where they are rather than by what they say (invariant 3 — never a text search).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Task {
    /// Whether the box is ticked.
    pub checked: bool,
    /// Byte offset, in the parsed source, of the single character between the
    /// brackets — the `x` of `[x]`, or the space of `[ ]`.
    pub marker: usize,
}

/// A container that opens and later closes around other events.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Tag<'a> {
    /// A paragraph. Emitted inside tight list items too; consumers decide whether to
    /// wrap it, using the `tight` flag on the enclosing [`Tag::List`].
    Paragraph,
    /// An ATX or setext heading.
    Heading {
        /// 1-6.
        level: u8,
    },
    /// A block quote, possibly marked as a callout.
    BlockQuote {
        /// Present when the quote opened with a `[!KIND]` marker.
        callout: Option<Callout<'a>>,
    },
    /// A fenced or indented code block. Its content arrives as a single
    /// [`Event::Text`].
    CodeBlock {
        /// First word of the info string; `None` when there was no info string.
        language: Option<Cow<'a, str>>,
        /// The rest of the info string, trimmed; `None` when there was none.
        meta: Option<Cow<'a, str>>,
        /// Whether the block was fenced rather than indented.
        fenced: bool,
    },
    /// A bullet or ordered list.
    List {
        /// The first ordinal for an ordered list; `None` for a bullet list.
        start: Option<u64>,
        /// Whether the list is tight (its item paragraphs are not wrapped).
        tight: bool,
    },
    /// One item of a list.
    Item {
        /// `Some` when the item is a task list item.
        task: Option<Task>,
    },
    /// A footnote definition.
    FootnoteDefinition {
        /// The footnote's label, as written.
        label: Cow<'a, str>,
    },
    /// A table. Contains one [`Tag::TableHead`] followed by zero or more
    /// [`Tag::TableRow`]s.
    Table {
        /// One entry per column.
        alignments: Vec<Alignment>,
    },
    /// The header row of a table.
    TableHead,
    /// A body row of a table.
    TableRow,
    /// One cell of a table row.
    TableCell {
        /// The alignment of this cell's column.
        alignment: Alignment,
    },
    /// `*emphasis*`
    Emphasis,
    /// `**strong**`
    Strong,
    /// `~~strikethrough~~`
    Strikethrough,
    /// A link. Its label arrives as the contained events.
    Link {
        /// The destination, as resolved by the parser.
        url: Cow<'a, str>,
        /// The title, empty when there was none.
        title: Cow<'a, str>,
    },
    /// An image. Its alt text arrives as the contained events.
    Image {
        /// The source, as resolved by the parser.
        url: Cow<'a, str>,
        /// The title, empty when there was none.
        title: Cow<'a, str>,
    },
    /// A `[[wikilink]]`. Its label arrives as the contained events.
    WikiLink {
        /// The link target, as written.
        target: Cow<'a, str>,
        /// Whether the author wrote it as an embed (`![[target]]`). Transclusion is
        /// out of scope (issue #12), so an embed is a reference to something that is
        /// not here — the renderer shows it as such rather than dropping it.
        embed: bool,
    },
}

/// Closes the matching [`Tag`], carrying only what a consumer needs to close it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TagEnd {
    /// Closes [`Tag::Paragraph`].
    Paragraph,
    /// Closes [`Tag::Heading`].
    Heading(
        /// The level being closed.
        u8,
    ),
    /// Closes [`Tag::BlockQuote`].
    BlockQuote,
    /// Closes [`Tag::CodeBlock`].
    CodeBlock,
    /// Closes [`Tag::List`].
    List {
        /// Whether the list being closed was ordered.
        ordered: bool,
    },
    /// Closes [`Tag::Item`].
    Item,
    /// Closes [`Tag::FootnoteDefinition`].
    FootnoteDefinition,
    /// Closes [`Tag::Table`].
    Table,
    /// Closes [`Tag::TableHead`].
    TableHead,
    /// Closes [`Tag::TableRow`].
    TableRow,
    /// Closes [`Tag::TableCell`].
    TableCell,
    /// Closes [`Tag::Emphasis`].
    Emphasis,
    /// Closes [`Tag::Strong`].
    Strong,
    /// Closes [`Tag::Strikethrough`].
    Strikethrough,
    /// Closes [`Tag::Link`].
    Link,
    /// Closes [`Tag::Image`].
    Image,
    /// Closes [`Tag::WikiLink`].
    WikiLink,
}

/// One step of a parsed document.
#[derive(Debug, Clone, PartialEq)]
pub enum Event<'a> {
    /// A container opens.
    Start(Tag<'a>),
    /// The matching container closes.
    End(TagEnd),
    /// Literal text, with entity references and backslash escapes already resolved.
    Text(Cow<'a, str>),
    /// The content of an inline code span.
    Code(Cow<'a, str>),
    /// A math span.
    Math {
        /// Display math (`$$…$$`) rather than inline (`$…$`).
        display: bool,
        /// The LaTeX source between the delimiters.
        latex: Cow<'a, str>,
    },
    /// A raw HTML block, verbatim. Sanitisation is the renderer's job, not the
    /// engine's.
    HtmlBlock(Cow<'a, str>),
    /// A raw inline HTML tag, verbatim.
    InlineHtml(Cow<'a, str>),
    /// A reference to a footnote definition, by label.
    FootnoteReference(Cow<'a, str>),
    /// A line break that does not force a new line.
    SoftBreak,
    /// A line break that does.
    HardBreak,
    /// A thematic break (`---`).
    ThematicBreak,
}

/// The result of parsing one document.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed<'a> {
    events: Vec<SpannedEvent<'a>>,
    front_matter: Option<&'a str>,
}

impl<'a> Parsed<'a> {
    /// Assembles a parse result. Engines call this; consumers read it.
    pub fn new(events: Vec<SpannedEvent<'a>>, front_matter: Option<&'a str>) -> Self {
        Self {
            events,
            front_matter,
        }
    }

    /// The document's events, in source order.
    pub fn events(&self) -> &[SpannedEvent<'a>] {
        &self.events
    }

    /// The raw front matter blob including its delimiters, when the document opened
    /// with one and [`Extension::FrontMatter`] was requested. Front matter is
    /// metadata: it is never part of the event stream and never rendered.
    pub fn front_matter(&self) -> Option<&'a str> {
        self.front_matter
    }
}

/// A markdown parser behind the axiomd boundary.
///
/// Implementations must be usable from a worker thread (the GTK main thread never
/// blocks on a parse) and must never expose their parser's types.
pub trait MarkdownEngine: Send + Sync {
    /// This engine's stable identifier.
    fn id(&self) -> EngineId;

    /// What a reader calls this engine, in a menu or a preferences row.
    ///
    /// An engine that is offered has to be named, and naming it belongs here rather
    /// than in whichever surface happens to offer it: a build that gains an engine
    /// gains its name everywhere at once, and no chooser can fall back to showing an
    /// identifier the reader never chose. Header capitalised, because every surface
    /// that shows it is one the HIG capitalises that way.
    fn display_name(&self) -> &'static str;

    /// Every extension this engine can parse. Requesting more than this is not an
    /// error; the surplus is simply parsed as ordinary markdown.
    fn capabilities(&self) -> Extensions;

    /// Parses `source`, recognising `extensions ∩ capabilities()`.
    fn parse<'a>(&self, source: &'a str, extensions: Extensions) -> Parsed<'a>;
}
