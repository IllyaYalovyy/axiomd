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

use axiomd_engine::{Alignment, Event, Parsed, Span, SpannedEvent, Tag, TagEnd, Task};

use crate::footnote::Footnotes;
use crate::plugin::{Asset, Plugins, Used};
use crate::request::{Request, origin_of};
use crate::sanitize::is_remote;
use crate::wikilink::Folder;
use crate::{Anchor, Heading, Picture, callout, highlight, slug};

/// Where the markup being written is going to be read.
///
/// The difference is not decoration. A document on screen is backed by a running
/// application: its pictures arrive through the app's own scheme, and a remote image
/// is a button the app answers for. A document in a file has none of that — it is
/// read in a browser that has never heard of axiomd — so its pictures travel inside
/// it and nothing in it is offered as a button.
pub(crate) enum Destination<'a> {
    /// The window the reader is in.
    Screen,
    /// A file that leaves axiomd behind, carrying whatever `embed` answers with for
    /// a reference the document makes — or `None`, for one that cannot be carried.
    File(&'a dyn Fn(&str) -> Option<Picture>),
}

/// One document as this module produces it: the markup, and everything about that
/// markup which is not markup.
pub(crate) struct Body {
    /// Unsanitised body markup.
    pub(crate) markup: String,
    pub(crate) anchors: Vec<Anchor>,
    pub(crate) outline: Vec<Heading>,
    /// The remote images standing behind placeholders in it.
    pub(crate) remote_images: Vec<String>,
    /// The styling this document needs beyond the bundled one: one entry per
    /// stylesheet of a plugin that contributed to it, as `(plugin id, asset)`.
    pub(crate) stylesheets: Vec<(&'static str, Asset)>,
    /// The code that draws what this document holds, in the order it is to be run,
    /// as `(plugin id, asset)`. Empty for every document no drawing plugin
    /// contributed to, which is what makes such a plugin free rather than optional.
    pub(crate) scripts: Vec<(&'static str, Asset)>,
}

/// Renders one parse, with the plugins the reader is reading under and the documents
/// beside the one being rendered — which is the whole of what a wikilink in it can
/// reach (`wikilink.rs`).
pub(crate) fn render(parsed: &Parsed<'_>, into: &Page<'_>) -> Body {
    let plugins = into.plugins;
    let mut writer = Writer {
        headings: slug::heading_ids(parsed),
        footnotes: Footnotes::of(parsed),
        used: plugins.nothing_used(),
        ..Writer::default()
    };
    for SpannedEvent { event, span } in parsed.events() {
        // A plugin sees prose, never code: what is inside a code block belongs to
        // whichever plugin claimed the fence, and to no transform at all.
        let rewritten = match writer.code.is_some() || plugins.is_empty() {
            true => None,
            false => plugins.rewrite(event, &mut writer.used),
        };
        match &rewritten {
            // The replacement is written at the span of what it replaced, so no
            // rewriting a plugin can do moves a block or breaks the source map.
            Some(events) => {
                for event in events {
                    writer.event(event, span, into);
                }
            }
            None => writer.event(event, span, into),
        }
    }
    let mut markup = writer.out;
    // Only the app can answer it, so only the app's own window carries it.
    if !writer.remote_images.is_empty() && matches!(into.to, Destination::Screen) {
        markup.insert_str(0, &load_all_banner(writer.remote_images.len()));
    }
    let mut used = writer.used;
    let markup = plugins.decorate(markup, &writer.anchors, &mut used);
    Body {
        markup,
        stylesheets: plugins.stylesheets(&used),
        scripts: plugins.scripts(&used),
        anchors: writer.anchors,
        outline: writer.outline,
        remote_images: writer.remote_images,
    }
}

/// Everything about a render that is not the document: where the markup is going, the
/// capabilities it is written with, and what lies beside it on disk.
///
/// One value rather than three parameters threaded through every method of the walk:
/// they are always all three of them together, and a walk that took them apart would
/// be a walk with three chances to pass the wrong one.
pub(crate) struct Page<'a> {
    pub(crate) to: Destination<'a>,
    pub(crate) plugins: &'a Plugins,
    /// The Markdown documents under the rendered document's own directory — the whole
    /// of what a `[[wikilink]]` in it can resolve to.
    pub(crate) beside: &'a Folder,
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
    /// A callout, which closes with whichever element it opened as — a foldable one
    /// is a `<details>` and every other is a `<blockquote>`.
    Callout {
        element: &'static str,
    },
    /// A footnote definition, carrying the way back to every reference that sent a
    /// reader here — written when the definition closes, because that is when its
    /// last paragraph is known.
    Definition {
        backrefs: String,
    },
    Other,
}

/// An image whose label is still being collected.
struct Image {
    url: String,
    title: String,
    alt: String,
}

/// The code block being written, once its info string has been read.
enum Code {
    /// The pipeline's own: highlighted in this language, or in none.
    Core(Option<String>),
    /// Claimed by a plugin, which is handed the whole block when the fence closes —
    /// so the source is collected here rather than written out.
    Claimed {
        /// Which plugin in the registry claimed it.
        at: usize,
        language: String,
        source: String,
        /// The anchor attribute the block was given when it opened. The block is
        /// written when it closes, and it must carry the line it started on.
        anchor: String,
    },
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
    /// The open code block, once its info string has been read.
    code: Option<Code>,
    /// Which plugins have contributed to this document — the set whose styling it
    /// needs.
    used: Used,
    /// The anchor id of each heading, in document order, and how many have been
    /// written. Computed ahead of the walk because a heading's id comes from text
    /// that only arrives after its opening tag has to be written.
    headings: Vec<String>,
    headings_written: usize,
    /// The headings a reader can be sent to, in document order.
    outline: Vec<Heading>,
    /// The one being written, while it is being written. `Some` only for a heading
    /// that anchored itself, because an entry naming a block with no anchor would be
    /// a row in the sidebar that goes nowhere.
    heading: Option<Heading>,
    /// The source of every placeholder standing in for an image the reader has not
    /// asked for, in document order.
    remote_images: Vec<String>,
    /// What each footnote label is called in front of the reader, and how many places
    /// refer to it (`footnote.rs`).
    footnotes: Footnotes,
    /// One entry per open wikilink: whether it resolved, and so which element has to
    /// be closed when it ends.
    wikilinks: Vec<bool>,
    /// Where in the markup the footnote definition being written began, so its
    /// back-references can be put at the end of it.
    definition: Option<usize>,
}

impl Writer {
    fn event(&mut self, event: &Event<'_>, span: &Span, into: &Page<'_>) {
        match event {
            Event::Start(tag) => self.start(tag, span, into),
            Event::End(end) => self.end(end, into),
            Event::Text(text) => match &mut self.code {
                Some(Code::Core(language)) => {
                    let code = highlight_or_escape(language.as_deref(), text);
                    self.out.push_str(&code);
                }
                Some(Code::Claimed { source, .. }) => source.push_str(text),
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
            // What the reader sees is a number in the order the document refers to
            // its footnotes, not the label the author filed it under — and every
            // reference is named so that the definition can send them back to the one
            // they came from.
            Event::FootnoteReference(label) => {
                let id = footnote_id(label);
                let shown = match self.footnotes.referenced(label) {
                    Some(reference) => format!(
                        "<sup class=\"footnote-ref\">\
                         <a id=\"fnref-{id}-{nth}\" href=\"#{id}\">{number}</a></sup>",
                        nth = reference.nth,
                        number = reference.number,
                    ),
                    None => format!(
                        "<sup class=\"footnote-ref\"><a href=\"#{id}\">{}</a></sup>",
                        escape_text(label),
                    ),
                };
                self.inline(&shown);
            }
            Event::SoftBreak => self.text("\n"),
            Event::HardBreak => {
                // Nothing reaches `text` for a hard break, so the words either side of
                // it would run together in the outline without this.
                self.spoken(" ");
                self.inline("<br>");
            }
            Event::ThematicBreak => {
                let anchor = self.anchor(span);
                self.block(&format!("<hr{anchor}>"));
                self.out.push('\n');
            }
        }
    }

    fn start(&mut self, tag: &Tag<'_>, span: &Span, into: &Page<'_>) {
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
                if !anchor.is_empty() {
                    self.heading = Some(Heading {
                        level: *level,
                        text: String::new(),
                        line: span.line,
                    });
                }
                let id = self.heading_id();
                self.block(&format!("<h{level}{id}{anchor}>"));
                self.stack.push(Frame::Other);
            }
            Tag::BlockQuote { callout } => {
                let anchor = self.anchor(span);
                match callout {
                    None => self.block(&format!("<blockquote{anchor}>")),
                    // A foldable callout is a `<details>` and its title the `<summary>`
                    // that opens it: folding is then the browser's own, which is what
                    // "no JavaScript" means for a document that cannot run any.
                    Some(callout) => {
                        let kind = callout::class_of(&callout.kind);
                        let (element, title_element, open) = match callout.fold {
                            None => ("blockquote", "p", ""),
                            Some(true) => ("details", "summary", " open"),
                            Some(false) => ("details", "summary", ""),
                        };
                        self.block(&format!(
                            "<{element} class=\"callout callout-{kind}\"{open}{anchor}>"
                        ));
                        let title = match &callout.title {
                            Some(title) => escape_text(title),
                            None => escape_text(&callout::title_of(&callout.kind)),
                        };
                        self.out.push('\n');
                        self.out.push_str(&format!(
                            "<{title_element} class=\"callout-title\">{title}</{title_element}>"
                        ));
                        self.stack.push(Frame::Callout { element });
                        return;
                    }
                }
                self.stack.push(Frame::Other);
            }
            Tag::CodeBlock { language, .. } => {
                let anchor = self.anchor(span);
                // A claimed fence is not written as it opens: the plugin is handed the
                // whole block at once, and what it answers with — its own markup, or
                // the source with a badge — is what stands here.
                if let Some(language) = language
                    && let Some(at) = into.plugins.claiming(language)
                {
                    self.code = Some(Code::Claimed {
                        at,
                        language: language.to_string(),
                        source: String::new(),
                        anchor,
                    });
                    self.stack.push(Frame::Other);
                    return;
                }
                let class = match language {
                    Some(language) => {
                        format!(" class=\"language-{}\"", escape_attribute(language))
                    }
                    None => String::new(),
                };
                self.block(&format!("<pre class=\"sy-code\"{anchor}><code{class}>"));
                self.code = Some(Code::Core(language.as_deref().map(str::to_string)));
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
                    Some(task) => self.block(&task_checkbox(task, &into.to)),
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
                let shown = match self.footnotes.defined(label) {
                    Some((number, _)) => number.to_string(),
                    None => escape_text(label),
                };
                self.out
                    .push_str(&format!("<sup class=\"footnote-label\">{shown}</sup>"));
                // Where the definition's own markup starts, so its back-references can
                // be put at the end of it when it closes.
                self.definition = Some(self.out.len());
                self.stack.push(Frame::Definition {
                    backrefs: backrefs(&id, self.footnotes.defined(label)),
                });
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
            // A wikilink is a link only when it goes somewhere. One that resolves to
            // nothing — a target no document answers to, a name two of them answer to,
            // or an embed, which axiomd shows rather than transcludes — is written as a
            // span: styled as a link that leads nowhere, and inert by construction
            // rather than by a policy that has to refuse it later.
            Tag::WikiLink { target, embed } => {
                let found = (!embed).then(|| into.beside.resolve(target)).flatten();
                self.wikilinks.push(found.is_some());
                match found {
                    Some(found) => self.inline(&format!(
                        "<a class=\"wikilink\" href=\"{}\">",
                        escape_attribute(&found.href),
                    )),
                    None => self.inline("<span class=\"wikilink wikilink-unresolved\">"),
                }
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

    fn end(&mut self, end: &TagEnd, into: &Page<'_>) {
        match end {
            TagEnd::Paragraph => {
                if matches!(self.stack.pop(), Some(Frame::Paragraph { wrapped: true })) {
                    self.close("</p>");
                }
            }
            TagEnd::Heading(level) => {
                if let Some(mut heading) = self.heading.take() {
                    heading.text = one_line(&heading.text);
                    self.outline.push(heading);
                }
                self.stack.pop();
                self.close(&format!("</h{level}>"));
            }
            TagEnd::BlockQuote => {
                let element = match self.stack.pop() {
                    Some(Frame::Callout { element }) => element,
                    _ => "blockquote",
                };
                self.close(&format!("</{element}>"));
            }
            TagEnd::CodeBlock => {
                self.stack.pop();
                match self.code.take() {
                    Some(Code::Claimed {
                        at,
                        language,
                        source,
                        anchor,
                    }) => self.claimed_fence(into.plugins, at, &language, &source, &anchor),
                    _ => self.out.push_str("</code></pre>\n"),
                }
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
                let backrefs = match self.stack.pop() {
                    Some(Frame::Definition { backrefs }) => backrefs,
                    _ => String::new(),
                };
                self.write_backrefs(&backrefs);
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
            TagEnd::Link => self.inline("</a>"),
            TagEnd::WikiLink => {
                let resolved = self.wikilinks.pop().unwrap_or(false);
                self.inline(if resolved { "</a>" } else { "</span>" });
            }
            TagEnd::Image => self.image(&into.to),
        }
    }

    /// Closes an image, writing its tag — or, when the image was itself inside
    /// another image's label, contributing its alt text to that label.
    ///
    /// A remote source never becomes an `<img>`: displaying the document would fetch
    /// it, and nothing a document says may cause a request (`design_decisions.md`).
    /// It becomes the placeholder card instead — the D4 ruling, where the card *is*
    /// the one-click load button rather than something that opens a question.
    ///
    /// A local one is the document's own picture. On screen it is served from the
    /// document's origin; in a file it has to travel inside the file, and a picture
    /// that cannot be carried says so where it would have been rather than leaving a
    /// reference nobody can resolve.
    fn image(&mut self, to: &Destination<'_>) {
        let Some(image) = self.alt.pop() else {
            return;
        };
        if let Some(outer) = self.alt.last_mut() {
            outer.alt.push_str(&image.alt);
            return;
        }
        if is_remote(&image.url) {
            self.remote_images.push(image.url.clone());
            self.out.push_str(&remote_placeholder(&image, to));
            return;
        }
        let source = match to {
            Destination::Screen => Some(image.url.clone()),
            Destination::File(embed) => embed(&image.url).map(|picture| data_uri(&picture)),
        };
        match source {
            Some(source) => self.out.push_str(&format!(
                "<img src=\"{}\" alt=\"{}\"{}>",
                escape_attribute(&source),
                escape_attribute(&image.alt),
                title_attribute(&image.title)
            )),
            None => self.out.push_str(&missing_picture(&image)),
        }
    }

    /// Writes a fence a plugin claimed: what the plugin drew, or — when it could not —
    /// the block as the author wrote it, with a badge saying who could not draw it.
    ///
    /// The degraded block is the source, highlighted exactly as it would have been
    /// with the plugin switched off, so a reader never loses a line of their document
    /// to a plugin (invariant 13). The badge is inline beside it and never a dialog
    /// (`ux_decisions.md`), and the wrapper carries the block's anchor either way.
    fn claimed_fence(
        &mut self,
        plugins: &Plugins,
        at: usize,
        language: &str,
        source: &str,
        anchor: &str,
    ) {
        match plugins.fence(at, language, source, &mut self.used) {
            Ok(markup) => {
                self.block(&format!(
                    "<div class=\"plugin plugin-{id}\"{anchor}>",
                    id = escape_attribute(plugins.id_of(at)),
                ));
                self.out.push('\n');
                self.out.push_str(&markup);
                self.close("</div>");
            }
            Err(reason) => {
                self.block(&format!("<div class=\"plugin-failure\"{anchor}>"));
                self.out.push('\n');
                self.out.push_str(&format!(
                    "<pre class=\"sy-code\"><code class=\"language-{language}\">{code}</code></pre>\n",
                    language = escape_attribute(language),
                    code = highlight_or_escape(Some(language), source),
                ));
                self.out.push_str(&format!(
                    "<p class=\"plugin-badge\">{name} could not draw this block: {reason}</p>\n",
                    name = escape_text(plugins.name_of(at)),
                    reason = escape_text(&reason),
                ));
                self.close("</div>");
            }
        }
    }

    /// Puts the way back to a footnote's references at the end of its definition.
    ///
    /// Inside the definition's last paragraph when it has one, which is where GitHub
    /// and Obsidian both put it and what keeps the arrow on the same line as the
    /// sentence it belongs to. A definition that ends in something else — a list, a
    /// code block — gets a line of its own instead, because putting an arrow inside
    /// one of those would change what it is.
    fn write_backrefs(&mut self, backrefs: &str) {
        let start = self.definition.take().unwrap_or(0);
        if backrefs.is_empty() {
            return;
        }
        let paragraph = self.out[start..]
            .ends_with("</p>\n")
            .then(|| self.out.len() - "</p>\n".len());
        match paragraph {
            Some(at) => self.out.insert_str(at, backrefs),
            None => self.close(&format!("<p class=\"footnote-backrefs\">{backrefs}</p>")),
        }
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
        self.spoken(text);
        match self.alt.last_mut() {
            Some(image) => image.alt.push_str(text),
            None => escape_into(&mut self.out, text),
        }
    }

    /// Adds `text` to the heading being written, if one is.
    ///
    /// A picture's label is not part of it: an outline entry says what the section is
    /// called, and the alt text of an image inside the heading is not one of its
    /// words — the same rule the heading's own anchor id follows (`slug`).
    fn spoken(&mut self, text: &str) {
        if let Some(heading) = self.heading.as_mut()
            && self.alt.is_empty()
        {
            heading.text.push_str(text);
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
    ///
    /// Neither does a block the engine moved. The anchor map is in document order with
    /// strictly increasing lines, and everything that navigates a document rides on
    /// that (invariant 3): `place.js` walks the blocks in the order they are on screen
    /// and stops at the first one past the line it wants. A block whose source is
    /// *above* the block before it would end that walk early and land the reader
    /// somewhere they never were — so it is written without a line rather than with a
    /// misleading one. GFM footnote definitions are the case that makes this real:
    /// they are collected to the foot of the document in the order they are first
    /// referred to, which is not the order they were written in.
    fn anchor(&mut self, span: &Span) -> String {
        if !self.stack.is_empty() || !self.alt.is_empty() {
            return String::new();
        }
        if self
            .anchors
            .last()
            .is_some_and(|last| last.line >= span.line)
        {
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
/// would come from, and — on screen, because the card is itself the link — one click
/// to load it.
///
/// It keeps `data-remote-src` so that the app can find this exact placeholder again
/// when the image arrives, and after a live reload has rebuilt the block around it.
///
/// In a file there is nothing behind the click: the app that would answer it is not
/// there, and a button that cannot do anything is worse than no button. So the card
/// keeps everything it says and stops being one.
fn remote_placeholder(image: &Image, to: &Destination<'_>) -> String {
    let label = if image.alt.trim().is_empty() {
        "Remote image"
    } else {
        &image.alt
    };
    let (open, close, action) = match to {
        Destination::Screen => (
            format!(
                "<a class=\"remote-image\" href=\"{href}\" data-remote-src=\"{source}\"{title}>",
                href = escape_attribute(&Request::LoadImage(image.url.clone()).uri()),
                source = escape_attribute(&image.url),
                title = title_attribute(&image.title),
            ),
            "</a>",
            "<span class=\"remote-image-action\">Load image</span>".to_owned(),
        ),
        Destination::File(_) => (
            format!(
                "<span class=\"remote-image\"{title}>",
                title = title_attribute(&image.title)
            ),
            "</span>",
            String::new(),
        ),
    };
    format!(
        "{open}\
         <span class=\"remote-image-label\">{label}</span>\
         <span class=\"remote-image-origin\">{origin}</span>\
         {action}{close}",
        label = escape_text(label),
        origin = escape_text(origin_of(&image.url)),
    )
}

/// What stands where a picture would have been in a file that could not carry it:
/// the same card, saying what is missing and where it lived.
fn missing_picture(image: &Image) -> String {
    let label = if image.alt.trim().is_empty() {
        "Image"
    } else {
        &image.alt
    };
    format!(
        "<span class=\"remote-image\">\
         <span class=\"remote-image-label\">{label}</span>\
         <span class=\"remote-image-origin\">{source}</span>\
         </span>",
        label = escape_text(label),
        source = escape_text(&image.url),
    )
}

/// A picture as the bytes of the document that carries it.
fn data_uri(picture: &Picture) -> String {
    format!(
        "data:{};base64,{}",
        picture.content_type,
        base64(&picture.bytes)
    )
}

/// Base64 as RFC 4648 §4 defines it: standard alphabet, padded, no line breaks —
/// which is the only spelling a `data:` URI accepts.
pub(crate) fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for group in bytes.chunks(3) {
        let bits = group.iter().enumerate().fold(0u32, |bits, (at, byte)| {
            bits | (u32::from(*byte) << (16 - 8 * at))
        });
        for at in 0..=group.len() {
            encoded.push(ALPHABET[(bits >> (18 - 6 * at) & 0b11_1111) as usize] as char);
        }
        for _ in group.len()..3 {
            encoded.push('=');
        }
    }
    encoded
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

/// A task list item's opening markup: the box, and — on screen — the click that
/// toggles it.
///
/// The box is a link because a rendered document cannot run a script: pressing it is a
/// navigation, the app's own policy reads it as the one request it is
/// ([`Request::ToggleTask`]), and the offset it carries is where the parser said the
/// marker was. Two identical items are therefore two different links, and nothing
/// anywhere searches the document's text for the line to change (invariant 3).
///
/// In a file there is no app to answer, so the box is what it has always been: a
/// disabled checkbox saying what the author wrote.
fn task_checkbox(task: &Task, to: &Destination<'_>) -> String {
    let checked = if task.checked { " checked" } else { "" };
    match to {
        Destination::Screen => format!(
            "<li class=\"task-list-item\">\
             <a class=\"task-toggle\" href=\"{}\"><input type=\"checkbox\"{checked}></a> ",
            escape_attribute(&Request::ToggleTask(task.marker).uri()),
        ),
        Destination::File(_) => {
            format!("<li class=\"task-list-item\"><input type=\"checkbox\" disabled{checked}> ")
        }
    }
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

/// A heading's words as one line of them: a setext heading spans two source lines and
/// a long one is often wrapped, and an outline entry is a single row either way.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<&str>>().join(" ")
}

/// The way back from a definition to every reference that points at it.
///
/// One arrow per reference, numbered from the second onwards: a footnote cited three
/// times has three ways back, and a reader who followed the second one is taken to
/// where they were rather than to where the first reader was.
fn backrefs(id: &str, defined: Option<(usize, usize)>) -> String {
    let Some((_, references)) = defined else {
        return String::new();
    };
    (1..=references)
        .map(|nth| {
            let counted = if nth == 1 {
                String::new()
            } else {
                format!("<sup>{nth}</sup>")
            };
            format!(
                " <a class=\"footnote-backref\" href=\"#fnref-{id}-{nth}\" \
                 title=\"Back to the text\">\u{21a9}{counted}</a>"
            )
        })
        .collect()
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

pub(crate) fn escape_text(text: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The test vectors of RFC 4648 §10, which is the specification a `data:` URI
    /// points at. A picture encoded a byte wrong is a picture that does not open,
    /// and only in the exported file — where nobody would see it until later.
    #[test]
    fn pictures_are_encoded_the_way_rfc_4648_says() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        // Every bit pattern, so a wrong shift or a short alphabet cannot pass: the
        // three bytes below cover all 64 code points across their four groups.
        assert_eq!(base64(&[0x00, 0x10, 0x83]), "ABCD");
        assert_eq!(base64(&[0xfb, 0xff, 0xbf]), "+/+/");
        assert_eq!(base64(&(0u8..=255).collect::<Vec<u8>>()).len(), 344);
    }

    /// A picture in a file is the bytes plus what they are, and nothing else: a data
    /// URI with a line break or a space in it is a broken picture.
    #[test]
    fn a_carried_picture_is_one_unbroken_uri() {
        let uri = data_uri(&Picture {
            bytes: b"foobar".to_vec(),
            content_type: "image/png".to_owned(),
        });

        assert_eq!(uri, "data:image/png;base64,Zm9vYmFy");
        assert!(!uri.contains(char::is_whitespace), "{uri}");
    }
}
