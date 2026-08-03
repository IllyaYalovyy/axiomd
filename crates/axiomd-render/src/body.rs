//! The event stream turned into document markup.
//!
//! Two things are produced together and cannot drift apart: the HTML, and the
//! anchor map. Every top-level block carries `data-line`, and the same block
//! contributes one [`Anchor`] — that pairing is what outline navigation, scroll
//! sync, search highlighting and live-reload position preservation all ride on, so
//! the attribute is never written without recording the anchor.
//!
//! Constructs whose full treatment belongs to a later feature are rendered
//! structurally rather than dropped: math keeps its LaTeX inside a `math` span for
//! MathML to replace, a callout keeps its kind and title, a wikilink keeps its
//! target, and an unclaimed fence keeps its `language-*` class for a fence plugin to
//! recognise. Nothing here panics on an event it does not fully implement, and no
//! document ever loses content to a missing feature.

use axiomd_engine::{Alignment, CalloutKind, Event, Parsed, Span, SpannedEvent, Tag, TagEnd};

use crate::request::{Request, origin_of};
use crate::sanitize::is_remote;
use crate::{Anchor, highlight, slug};

/// Renders one parse into unsanitised body markup, its anchor map, and the remote
/// images standing behind placeholders in it.
pub(crate) fn render(parsed: &Parsed<'_>) -> (String, Vec<Anchor>, Vec<String>) {
    let mut writer = Writer {
        headings: slug::heading_ids(parsed),
        ..Writer::default()
    };
    for SpannedEvent { event, span } in parsed.events() {
        writer.event(event, span);
    }
    let mut out = writer.out;
    if !writer.remote_images.is_empty() {
        out.insert_str(0, &load_all_banner(writer.remote_images.len()));
    }
    (out, writer.anchors, writer.remote_images)
}

/// The one affordance a document with remote images carries: inline, above the
/// document, and never a dialog (`ux_decisions.md`). The app hides it once no
/// placeholder is left.
fn load_all_banner(images: usize) -> String {
    let images = if images == 1 {
        "1 image".to_owned()
    } else {
        format!("{images} images")
    };
    format!(
        "<div class=\"remote-banner\">\
         <span class=\"remote-banner-text\">This document links {images} from the internet. \
         Nothing has been loaded.</span>\
         <a class=\"remote-banner-action\" href=\"{}\">Load all</a>\
         </div>\n",
        Request::LoadAllImages.uri(),
    )
}

/// One open block container, carrying whatever its children need to know about it.
enum Frame {
    /// A paragraph that a tight list item left unwrapped emits no closing tag.
    Paragraph {
        wrapped: bool,
    },
    List {
        tight: bool,
    },
    Item {
        tight: bool,
    },
    /// A table opens `<tbody>` lazily, on its first body row.
    Table {
        body: bool,
    },
    /// The header row, whose cells are `<th>`.
    Head,
    /// One cell, which must close with the tag it opened with.
    Cell {
        head: bool,
    },
    Other,
}

/// An image whose label is still being collected.
struct Image {
    url: String,
    title: String,
    alt: String,
}

#[derive(Default)]
struct Writer {
    out: String,
    anchors: Vec<Anchor>,
    /// Open block containers, innermost last. Empty means "at top level", which is
    /// exactly the condition for emitting an anchor.
    stack: Vec<Frame>,
    /// One entry per open image. Non-empty means markup is suppressed and text is
    /// being collected into an alt attribute instead.
    alt: Vec<Image>,
    /// The open code block's language, once removed from its info string.
    code: Option<Option<String>>,
    /// The anchor id of each heading, in document order, and how many have been
    /// written. Computed ahead of the walk because a heading's id comes from text
    /// that only arrives after its opening tag has to be written.
    headings: Vec<String>,
    headings_written: usize,
    /// The source of every placeholder standing in for an image the reader has not
    /// asked for, in document order.
    remote_images: Vec<String>,
}

impl Writer {
    fn event(&mut self, event: &Event<'_>, span: &Span) {
        match event {
            Event::Start(tag) => self.start(tag, span),
            Event::End(end) => self.end(end),
            Event::Text(text) => match &self.code {
                Some(language) => {
                    let code = highlight_or_escape(language.as_deref(), text);
                    self.out.push_str(&code);
                }
                None => self.text(text),
            },
            Event::Code(code) => {
                self.inline("<code>");
                self.text(code);
                self.inline("</code>");
            }
            Event::Math { display, latex } => {
                let kind = if *display { "display" } else { "inline" };
                self.inline(&format!("<span class=\"math math-{kind}\">"));
                self.text(latex);
                self.inline("</span>");
            }
            Event::HtmlBlock(html) => {
                // Raw HTML is written verbatim and cleaned with the rest of the
                // document; see `sanitize`. It cannot be wrapped in an anchored
                // element: CommonMark ends an HTML block at a blank line, so
                // `<div align="center">` and its `</div>` are separate blocks, and
                // wrapping either one would make the parser close it early and
                // rearrange the document around it. A top-level block instead gets a
                // zero-height anchor element in front of it, which carries the line
                // without enclosing anything.
                let anchor = self.anchor(span);
                if !anchor.is_empty() {
                    self.block(&format!("<span class=\"source-anchor\"{anchor}></span>"));
                    self.out.push('\n');
                }
                self.out.push_str(html);
            }
            Event::InlineHtml(html) => {
                if self.alt.is_empty() {
                    self.out.push_str(html);
                }
            }
            Event::FootnoteReference(label) => {
                let id = footnote_id(label);
                let label = escape_text(label);
                self.inline(&format!(
                    "<sup class=\"footnote-ref\"><a href=\"#{id}\">{label}</a></sup>"
                ));
            }
            Event::SoftBreak => self.text("\n"),
            Event::HardBreak => self.inline("<br>"),
            Event::ThematicBreak => {
                let anchor = self.anchor(span);
                self.block(&format!("<hr{anchor}>"));
                self.out.push('\n');
            }
        }
    }

    fn start(&mut self, tag: &Tag<'_>, span: &Span) {
        match tag {
            Tag::Paragraph => {
                let wrapped = !matches!(self.stack.last(), Some(Frame::Item { tight: true }));
                if wrapped {
                    let anchor = self.anchor(span);
                    self.block(&format!("<p{anchor}>"));
                }
                self.stack.push(Frame::Paragraph { wrapped });
            }
            Tag::Heading { level } => {
                let anchor = self.anchor(span);
                let id = self.heading_id();
                self.block(&format!("<h{level}{id}{anchor}>"));
                self.stack.push(Frame::Other);
            }
            Tag::BlockQuote { callout } => {
                let anchor = self.anchor(span);
                match callout {
                    None => self.block(&format!("<blockquote{anchor}>")),
                    Some(callout) => {
                        let kind = callout_kind(callout.kind);
                        self.block(&format!(
                            "<blockquote class=\"callout callout-{kind}\"{anchor}>"
                        ));
                        let title = match &callout.title {
                            Some(title) => escape_text(title),
                            None => callout_title(callout.kind).to_string(),
                        };
                        self.out.push('\n');
                        self.out
                            .push_str(&format!("<p class=\"callout-title\">{title}</p>"));
                    }
                }
                self.stack.push(Frame::Other);
            }
            Tag::CodeBlock { language, .. } => {
                let anchor = self.anchor(span);
                let class = match language {
                    Some(language) => {
                        format!(" class=\"language-{}\"", escape_attribute(language))
                    }
                    None => String::new(),
                };
                self.block(&format!("<pre class=\"sy-code\"{anchor}><code{class}>"));
                self.code = Some(language.as_deref().map(str::to_string));
                self.stack.push(Frame::Other);
            }
            Tag::List { start, tight } => {
                let anchor = self.anchor(span);
                match start {
                    None => self.block(&format!("<ul{anchor}>")),
                    Some(1) => self.block(&format!("<ol{anchor}>")),
                    Some(start) => self.block(&format!("<ol start=\"{start}\"{anchor}>")),
                }
                self.stack.push(Frame::List { tight: *tight });
            }
            Tag::Item { task } => {
                let tight = matches!(self.stack.last(), Some(Frame::List { tight: true }));
                match task {
                    None => self.block("<li>"),
                    Some(checked) => {
                        let checked = if *checked { " checked" } else { "" };
                        self.block(&format!(
                            "<li class=\"task-list-item\"><input type=\"checkbox\" disabled{checked}> "
                        ));
                    }
                }
                self.stack.push(Frame::Item { tight });
            }
            Tag::FootnoteDefinition { label } => {
                let anchor = self.anchor(span);
                let id = footnote_id(label);
                self.block(&format!(
                    "<div class=\"footnote-definition\" id=\"{id}\"{anchor}>"
                ));
                self.out.push('\n');
                self.out.push_str(&format!(
                    "<sup class=\"footnote-label\">{}</sup>",
                    escape_text(label)
                ));
                self.stack.push(Frame::Other);
            }
            Tag::Table { .. } => {
                let anchor = self.anchor(span);
                self.block(&format!("<table{anchor}>"));
                self.stack.push(Frame::Table { body: false });
            }
            Tag::TableHead => {
                self.block("<thead>");
                self.out.push('\n');
                self.out.push_str("<tr>");
                self.stack.push(Frame::Head);
            }
            Tag::TableRow => {
                if let Some(Frame::Table { body }) = self.stack.last_mut()
                    && !*body
                {
                    *body = true;
                    self.block("<tbody>");
                }
                self.block("<tr>");
                self.stack.push(Frame::Other);
            }
            Tag::TableCell { alignment } => {
                let head = matches!(self.stack.last(), Some(Frame::Head));
                let name = if head { "th" } else { "td" };
                let class = match alignment {
                    Alignment::None => String::new(),
                    alignment => format!(" class=\"align-{}\"", align_name(*alignment)),
                };
                // Cells stay on their row's line: the HTML parser treats whitespace
                // between cells as table text and relocates it into a cell, which
                // would put a stray line break inside every heading cell.
                self.inline(&format!("<{name}{class}>"));
                self.stack.push(Frame::Cell { head });
            }
            Tag::Emphasis => self.inline("<em>"),
            Tag::Strong => self.inline("<strong>"),
            Tag::Strikethrough => self.inline("<del>"),
            Tag::Link { url, title } => {
                self.inline(&format!(
                    "<a href=\"{}\"{}>",
                    escape_attribute(url),
                    title_attribute(title)
                ));
            }
            Tag::WikiLink { target } => {
                self.inline(&format!(
                    "<a class=\"wikilink\" href=\"{}\">",
                    escape_attribute(target)
                ));
            }
            // An image's label is alt text, which is a plain-text attribute: markup
            // inside it is collected as text until the image closes.
            Tag::Image { url, title } => self.alt.push(Image {
                url: url.to_string(),
                title: title.to_string(),
                alt: String::new(),
            }),
        }
    }

    fn end(&mut self, end: &TagEnd) {
        match end {
            TagEnd::Paragraph => {
                if matches!(self.stack.pop(), Some(Frame::Paragraph { wrapped: true })) {
                    self.close("</p>");
                }
            }
            TagEnd::Heading(level) => {
                self.stack.pop();
                self.close(&format!("</h{level}>"));
            }
            TagEnd::BlockQuote => {
                self.stack.pop();
                self.close("</blockquote>");
            }
            TagEnd::CodeBlock => {
                self.stack.pop();
                self.code = None;
                self.out.push_str("</code></pre>\n");
            }
            TagEnd::List { ordered } => {
                self.stack.pop();
                self.close(if *ordered { "</ol>" } else { "</ul>" });
            }
            TagEnd::Item => {
                self.stack.pop();
                self.close("</li>");
            }
            TagEnd::FootnoteDefinition => {
                self.stack.pop();
                self.close("</div>");
            }
            TagEnd::Table => {
                if let Some(Frame::Table { body: true }) = self.stack.pop() {
                    self.close("</tbody>");
                }
                self.close("</table>");
            }
            TagEnd::TableHead => {
                self.stack.pop();
                self.close("</tr>");
                self.close("</thead>");
            }
            TagEnd::TableRow => {
                self.stack.pop();
                self.close("</tr>");
            }
            TagEnd::TableCell => {
                let head = matches!(self.stack.pop(), Some(Frame::Cell { head: true }));
                self.inline(if head { "</th>" } else { "</td>" });
            }
            TagEnd::Emphasis => self.inline("</em>"),
            TagEnd::Strong => self.inline("</strong>"),
            TagEnd::Strikethrough => self.inline("</del>"),
            TagEnd::Link | TagEnd::WikiLink => self.inline("</a>"),
            TagEnd::Image => self.image(),
        }
    }

    /// Closes an image, writing its tag — or, when the image was itself inside
    /// another image's label, contributing its alt text to that label.
    ///
    /// A remote source never becomes an `<img>`: displaying the document would fetch
    /// it, and nothing a document says may cause a request (`design_decisions.md`).
    /// It becomes the placeholder card instead — the D4 ruling, where the card *is*
    /// the one-click load button rather than something that opens a question.
    fn image(&mut self) {
        let Some(image) = self.alt.pop() else {
            return;
        };
        if let Some(outer) = self.alt.last_mut() {
            outer.alt.push_str(&image.alt);
            return;
        }
        if is_remote(&image.url) {
            self.remote_images.push(image.url.clone());
            self.out.push_str(&remote_placeholder(&image));
            return;
        }
        self.out.push_str(&format!(
            "<img src=\"{}\" alt=\"{}\"{}>",
            escape_attribute(&image.url),
            escape_attribute(&image.alt),
            title_attribute(&image.title)
        ));
    }

    /// The id of the next heading, as an attribute. Empty when the heading had
    /// nothing to make an anchor out of.
    fn heading_id(&mut self) -> String {
        let id = self
            .headings
            .get(self.headings_written)
            .map(String::as_str)
            .unwrap_or_default();
        self.headings_written += 1;
        if id.is_empty() {
            return String::new();
        }
        format!(" id=\"{}\"", escape_attribute(id))
    }

    /// Writes text, or collects it when an image label is open.
    fn text(&mut self, text: &str) {
        match self.alt.last_mut() {
            Some(image) => image.alt.push_str(text),
            None => escape_into(&mut self.out, text),
        }
    }

    /// Writes inline markup, unless an image label is open — alt text is plain.
    fn inline(&mut self, markup: &str) {
        if self.alt.is_empty() {
            self.out.push_str(markup);
        }
    }

    /// Opens a block-level element on a line of its own.
    fn block(&mut self, markup: &str) {
        if !self.out.is_empty() && !self.out.ends_with('\n') {
            self.out.push('\n');
        }
        self.out.push_str(markup);
    }

    /// Closes a block-level element and ends its line.
    fn close(&mut self, markup: &str) {
        self.out.push_str(markup);
        self.out.push('\n');
    }

    /// Records the anchor of a top-level block and returns its attribute. Nested
    /// blocks get neither.
    fn anchor(&mut self, span: &Span) -> String {
        if !self.stack.is_empty() || !self.alt.is_empty() {
            return String::new();
        }
        self.anchors.push(Anchor {
            line: span.line,
            source: span.range.clone(),
        });
        format!(" data-line=\"{}\"", span.line)
    }
}

/// The card a remote image renders as: what the reader would be loading, where it
/// would come from, and — because the card is itself the link — one click to load it.
///
/// It keeps `data-remote-src` so that the app can find this exact placeholder again
/// when the image arrives, and after a live reload has rebuilt the block around it.
fn remote_placeholder(image: &Image) -> String {
    let label = if image.alt.trim().is_empty() {
        "Remote image"
    } else {
        &image.alt
    };
    format!(
        "<a class=\"remote-image\" href=\"{href}\" data-remote-src=\"{source}\"{title}>\
         <span class=\"remote-image-label\">{label}</span>\
         <span class=\"remote-image-origin\">{origin}</span>\
         <span class=\"remote-image-action\">Load image</span>\
         </a>",
        href = escape_attribute(&Request::LoadImage(image.url.clone()).uri()),
        source = escape_attribute(&image.url),
        title = title_attribute(&image.title),
        label = escape_text(label),
        origin = escape_text(origin_of(&image.url)),
    )
}

fn highlight_or_escape(language: Option<&str>, code: &str) -> String {
    if let Some(language) = language
        && let Some(highlighted) = highlight::highlight(language, code)
    {
        return highlighted;
    }
    let mut escaped = String::with_capacity(code.len());
    escape_into(&mut escaped, code);
    escaped
}

fn title_attribute(title: &str) -> String {
    if title.is_empty() {
        return String::new();
    }
    format!(" title=\"{}\"", escape_attribute(title))
}

fn align_name(alignment: Alignment) -> &'static str {
    match alignment {
        Alignment::None => "",
        Alignment::Left => "left",
        Alignment::Center => "center",
        Alignment::Right => "right",
    }
}

fn callout_kind(kind: CalloutKind) -> &'static str {
    match kind {
        CalloutKind::Note => "note",
        CalloutKind::Tip => "tip",
        CalloutKind::Important => "important",
        CalloutKind::Warning => "warning",
        CalloutKind::Caution => "caution",
    }
}

fn callout_title(kind: CalloutKind) -> &'static str {
    match kind {
        CalloutKind::Note => "Note",
        CalloutKind::Tip => "Tip",
        CalloutKind::Important => "Important",
        CalloutKind::Warning => "Warning",
        CalloutKind::Caution => "Caution",
    }
}

/// The document-unique id a footnote reference links to.
fn footnote_id(label: &str) -> String {
    let mut id = String::from("fn-");
    for c in label.chars() {
        match c {
            'a'..='z' | '0'..='9' | '-' | '_' => id.push(c),
            'A'..='Z' => id.push(c.to_ascii_lowercase()),
            _ => id.push('-'),
        }
    }
    id
}

fn escape_text(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    escape_into(&mut escaped, text);
    escaped
}

fn escape_attribute(value: &str) -> String {
    escape_text(value).replace('\'', "&#39;")
}

fn escape_into(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
}
