//! Shared machinery for the conformance suites.
//!
//! Two things live here and nowhere else:
//!
//! * a loader for the vendored `spec.txt` files in `tests/spec/`, and
//! * a minimal HTML serialiser for the engine's event stream.
//!
//! The serialiser exists *only* so spec cases can be compared against the
//! specification's own expected HTML. It is deliberately not part of
//! `axiomd-engine`'s API: the real rendering pipeline (anchors, sanitisation,
//! highlighting, plugins) is `axiomd-render`'s job, issue #3.

#![allow(dead_code)]

use std::fmt::Write as _;
use std::path::Path;

use axiomd_engine::{Alignment, Event, Parsed, SpannedEvent, Tag, TagEnd};

// ---------------------------------------------------------------------------
// Spec file loading
// ---------------------------------------------------------------------------

/// One `example` block from a vendored spec file.
#[derive(Debug, Clone)]
pub struct Example {
    /// 1-based index among all examples in the file, matching the numbering the
    /// published specification uses.
    pub number: usize,
    /// The nearest preceding section heading, e.g. `Tables (extension)`.
    pub section: String,
    /// The Markdown input, with `→` restored to a tab.
    pub markdown: String,
    /// The expected HTML, with `→` restored to a tab.
    pub html: String,
}

/// The delimiter the CommonMark and GFM spec files use around examples.
const EXAMPLE_FENCE: &str = "````````````````````````````````";

/// Loads every example from a spec file under `tests/spec/`.
///
/// # Panics
///
/// Panics if the file is missing or an example block is unterminated — a corrupt
/// fixture must never look like an empty suite.
pub fn load_examples(file_name: &str) -> Vec<Example> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/spec")
        .join(file_name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading spec fixture {}: {e}", path.display()));

    let mut examples = Vec::new();
    let mut section = String::new();
    let mut lines = text.lines();

    while let Some(line) = lines.next() {
        if let Some(rest) = line.strip_prefix('#') {
            section = rest.trim_start_matches('#').trim().to_string();
            continue;
        }
        if !line.starts_with(EXAMPLE_FENCE) || !line.contains("example") {
            continue;
        }

        let mut markdown = String::new();
        let mut saw_separator = false;
        for line in lines.by_ref() {
            if line == "." {
                saw_separator = true;
                break;
            }
            markdown.push_str(line);
            markdown.push('\n');
        }
        assert!(
            saw_separator,
            "example {} in {file_name} has no `.` separator",
            examples.len() + 1
        );

        let mut html = String::new();
        let mut saw_end = false;
        for line in lines.by_ref() {
            if line.starts_with(EXAMPLE_FENCE) {
                saw_end = true;
                break;
            }
            html.push_str(line);
            html.push('\n');
        }
        assert!(
            saw_end,
            "example {} in {file_name} is unterminated",
            examples.len() + 1
        );

        examples.push(Example {
            number: examples.len() + 1,
            section: section.clone(),
            markdown: markdown.replace('\u{2192}', "\t"),
            html: html.replace('\u{2192}', "\t"),
        });
    }

    assert!(
        !examples.is_empty(),
        "no examples found in {file_name}; the fixture or the loader is broken"
    );
    examples
}

/// Reports every example whose serialised HTML differs from the specification's.
///
/// Returns `(example, actual_html)` pairs so callers can both count and print.
pub fn run_suite(
    examples: &[Example],
    parse: impl Fn(&str) -> Parsed<'_>,
    html: HtmlFlavor,
) -> Vec<(&Example, String)> {
    let mut failures = Vec::new();
    for example in examples {
        let parsed = parse(&example.markdown);
        let actual = to_html(parsed.events(), html);
        if actual != example.html {
            failures.push((example, actual));
        }
    }
    failures
}

/// Formats failures for an assertion message: the input, what the spec wants, and
/// what we produced.
pub fn describe(failures: &[(&Example, String)]) -> String {
    let mut out = String::new();
    for (example, actual) in failures {
        let _ = write!(
            out,
            "\n--- example {} [{}]\nmarkdown: {:?}\nexpected: {:?}\nactual:   {:?}\n",
            example.number, example.section, example.markdown, example.html, actual
        );
    }
    out
}

// ---------------------------------------------------------------------------
// Minimal HTML serialisation
// ---------------------------------------------------------------------------

/// Which specification's HTML output rules to follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HtmlFlavor {
    /// CommonMark: raw HTML passes through untouched.
    CommonMark,
    /// GFM: additionally applies the "Disallowed Raw HTML" tag filter, which the
    /// specification defines as an *output* filter rather than a parse rule.
    Gfm,
}

/// Serialises an event stream to the HTML the CommonMark/GFM specs expect.
pub fn to_html(events: &[SpannedEvent<'_>], flavor: HtmlFlavor) -> String {
    let mut w = Writer {
        out: String::new(),
        at_line_start: true,
        flavor,
        open: Vec::new(),
        list_tight: Vec::new(),
        table_body_open: Vec::new(),
        plain_depth: 0,
        image_title: None,
    };
    for spanned in events {
        w.event(&spanned.event);
    }
    w.out
}

struct Writer {
    out: String,
    at_line_start: bool,
    flavor: HtmlFlavor,
    /// Stack of currently open tags, innermost last.
    open: Vec<OpenTag>,
    /// `tight` of every currently open list, innermost last.
    list_tight: Vec<bool>,
    /// Whether a `<tbody>` has been opened for every currently open table.
    table_body_open: Vec<bool>,
    /// Depth of nesting inside an image's alt text, where markup is dropped.
    plain_depth: usize,
    /// Title of the outermost open image, written once its alt text is complete.
    image_title: Option<String>,
}

/// The subset of tag identity the serialiser needs while a tag is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenTag {
    Item,
    TableHead,
    TableRow,
    Image,
    Other,
}

impl Writer {
    fn push(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        self.out.push_str(s);
        self.at_line_start = s.ends_with('\n');
    }

    /// Newline unless already at the start of a line (cmark's `cr`).
    fn cr(&mut self) {
        if !self.at_line_start {
            self.push("\n");
        }
    }

    /// Unconditional newline (cmark's `lf`).
    fn lf(&mut self) {
        self.push("\n");
    }

    fn escaped(&mut self, s: &str) {
        let escaped = escape(s);
        self.push(&escaped);
    }

    /// A paragraph is "tight" — rendered without `<p>` — when it sits directly in an
    /// item of a tight list.
    fn in_tight_item(&self) -> bool {
        self.open.last() == Some(&OpenTag::Item) && self.list_tight.last() == Some(&true)
    }

    fn event(&mut self, event: &Event<'_>) {
        if self.plain_depth > 0 {
            self.plain_event(event);
            return;
        }
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(*tag),
            Event::Text(text) => self.escaped(text),
            Event::Code(code) => {
                self.push("<code>");
                self.escaped(code);
                self.push("</code>");
            }
            Event::Math { display, latex } => {
                let style = if *display { "display" } else { "inline" };
                self.push(&format!("<span data-math-style=\"{style}\">"));
                self.escaped(latex);
                self.push("</span>");
            }
            Event::HtmlBlock(html) => {
                self.cr();
                let rendered = match self.flavor {
                    HtmlFlavor::CommonMark => html.to_string(),
                    HtmlFlavor::Gfm => tagfilter_block(html),
                };
                self.push(&rendered);
                self.cr();
            }
            Event::InlineHtml(html) => {
                let rendered = match self.flavor {
                    HtmlFlavor::CommonMark => html.to_string(),
                    HtmlFlavor::Gfm if is_disallowed_raw_html(html) => {
                        format!("&lt;{}", &html[1..])
                    }
                    HtmlFlavor::Gfm => html.to_string(),
                };
                self.push(&rendered);
            }
            Event::FootnoteReference(label) => {
                let label = escape(label);
                self.push(&format!(
                    "<sup class=\"footnote-ref\"><a href=\"#fn-{label}\">{label}</a></sup>"
                ));
            }
            Event::SoftBreak => self.push("\n"),
            Event::HardBreak => {
                self.push("<br />");
                self.lf();
            }
            Event::ThematicBreak => {
                self.cr();
                self.push("<hr />");
                self.lf();
            }
        }
    }

    /// Inside an image's alt text only literal characters survive.
    fn plain_event(&mut self, event: &Event<'_>) {
        match event {
            Event::Start(Tag::Image { .. }) => self.plain_depth += 1,
            Event::End(TagEnd::Image) => {
                self.plain_depth -= 1;
                if self.plain_depth == 0 {
                    self.close_image();
                }
            }
            Event::Text(text) | Event::Code(text) | Event::InlineHtml(text) => self.escaped(text),
            Event::Math { latex, .. } => self.escaped(latex),
            Event::SoftBreak | Event::HardBreak => self.push(" "),
            _ => {}
        }
    }

    fn close_image(&mut self) {
        let title = match self.open.pop() {
            Some(OpenTag::Image) => self.image_title.take().unwrap_or_default(),
            other => panic!("unbalanced image end, top of stack was {other:?}"),
        };
        if !title.is_empty() {
            self.push("\" title=\"");
            self.escaped(&title);
        }
        self.push("\" />");
    }

    fn start(&mut self, tag: &Tag<'_>) {
        match tag {
            Tag::Paragraph => {
                if !self.in_tight_item() {
                    self.cr();
                    self.push("<p>");
                }
                self.open.push(OpenTag::Other);
            }
            Tag::Heading { level } => {
                self.cr();
                self.push(&format!("<h{level}>"));
                self.open.push(OpenTag::Other);
            }
            Tag::BlockQuote { .. } => {
                self.cr();
                self.push("<blockquote>");
                self.lf();
                self.open.push(OpenTag::Other);
            }
            Tag::CodeBlock { language, .. } => {
                self.cr();
                self.push("<pre><code");
                if let Some(language) = language {
                    self.push(&format!(" class=\"language-{}\"", escape(language)));
                }
                self.push(">");
                self.open.push(OpenTag::Other);
            }
            Tag::List { start, tight } => {
                self.cr();
                match start {
                    None => self.push("<ul>"),
                    Some(1) => self.push("<ol>"),
                    Some(n) => self.push(&format!("<ol start=\"{n}\">")),
                }
                self.lf();
                self.list_tight.push(*tight);
                self.open.push(OpenTag::Other);
            }
            Tag::Item { task } => {
                self.cr();
                self.push("<li>");
                // Attribute order and the lack of a self-closing slash are what the
                // GFM spec's own expected output uses; see spec examples 279-280.
                match task {
                    Some(task) if task.checked => {
                        self.push("<input checked=\"\" disabled=\"\" type=\"checkbox\"> ")
                    }
                    Some(_) => self.push("<input disabled=\"\" type=\"checkbox\"> "),
                    None => {}
                }
                self.open.push(OpenTag::Item);
            }
            Tag::FootnoteDefinition { label } => {
                self.cr();
                self.push(&format!(
                    "<section class=\"footnote-definition\" id=\"fn-{}\">",
                    escape(label)
                ));
                self.lf();
                self.open.push(OpenTag::Other);
            }
            Tag::Table { .. } => {
                self.cr();
                self.push("<table>");
                self.lf();
                self.table_body_open.push(false);
                self.open.push(OpenTag::Other);
            }
            Tag::TableHead => {
                self.cr();
                self.push("<thead>");
                self.lf();
                self.push("<tr>");
                self.open.push(OpenTag::TableHead);
            }
            Tag::TableRow => {
                self.cr();
                if let Some(body_open) = self.table_body_open.last_mut()
                    && !*body_open
                {
                    *body_open = true;
                    self.push("<tbody>");
                    self.lf();
                }
                self.push("<tr>");
                self.open.push(OpenTag::TableRow);
            }
            Tag::TableCell { alignment } => {
                let head = self
                    .open
                    .iter()
                    .rev()
                    .find(|t| matches!(t, OpenTag::TableHead | OpenTag::TableRow))
                    == Some(&OpenTag::TableHead);
                self.cr();
                self.push(if head { "<th" } else { "<td" });
                match alignment {
                    Alignment::None => {}
                    Alignment::Left => self.push(" align=\"left\""),
                    Alignment::Center => self.push(" align=\"center\""),
                    Alignment::Right => self.push(" align=\"right\""),
                }
                self.push(">");
                self.open.push(OpenTag::Other);
            }
            Tag::Emphasis => {
                self.push("<em>");
                self.open.push(OpenTag::Other);
            }
            Tag::Strong => {
                self.push("<strong>");
                self.open.push(OpenTag::Other);
            }
            Tag::Strikethrough => {
                self.push("<del>");
                self.open.push(OpenTag::Other);
            }
            Tag::Link { url, title } => {
                self.push("<a href=\"");
                let href = escape_href(url);
                self.push(&href);
                if !title.is_empty() {
                    self.push("\" title=\"");
                    self.escaped(title);
                }
                self.push("\">");
                self.open.push(OpenTag::Other);
            }
            Tag::Image { url, title } => {
                self.push("<img src=\"");
                let href = escape_href(url);
                self.push(&href);
                self.push("\" alt=\"");
                self.image_title = Some(title.to_string());
                self.open.push(OpenTag::Image);
                self.plain_depth = 1;
            }
            Tag::WikiLink { target, .. } => {
                self.push("<a href=\"");
                let href = escape_href(target);
                self.push(&href);
                self.push("\" data-wikilink=\"true\">");
                self.open.push(OpenTag::Other);
            }
        }
    }

    fn end(&mut self, tag: TagEnd) {
        if !matches!(tag, TagEnd::Image) {
            self.open.pop();
        }
        match tag {
            TagEnd::Paragraph => {
                if !self.in_tight_item() {
                    self.push("</p>");
                    self.lf();
                }
            }
            TagEnd::Heading(level) => {
                self.push(&format!("</h{level}>"));
                self.lf();
            }
            TagEnd::BlockQuote => {
                self.cr();
                self.push("</blockquote>");
                self.lf();
            }
            TagEnd::CodeBlock => {
                self.push("</code></pre>");
                self.lf();
            }
            TagEnd::List { ordered } => {
                self.list_tight.pop();
                self.push(if ordered { "</ol>" } else { "</ul>" });
                self.lf();
            }
            TagEnd::Item => {
                self.push("</li>");
                self.lf();
            }
            TagEnd::FootnoteDefinition => {
                self.cr();
                self.push("</section>");
                self.lf();
            }
            TagEnd::Table => {
                if self.table_body_open.pop() == Some(true) {
                    self.cr();
                    self.push("</tbody>");
                    self.lf();
                }
                self.cr();
                self.push("</table>");
                self.lf();
            }
            TagEnd::TableHead => {
                self.cr();
                self.push("</tr>");
                self.cr();
                self.push("</thead>");
            }
            TagEnd::TableRow => {
                self.cr();
                self.push("</tr>");
            }
            TagEnd::TableCell => {
                let head = self
                    .open
                    .iter()
                    .rev()
                    .find(|t| matches!(t, OpenTag::TableHead | OpenTag::TableRow))
                    == Some(&OpenTag::TableHead);
                self.push(if head { "</th>" } else { "</td>" });
            }
            TagEnd::Emphasis => self.push("</em>"),
            TagEnd::Strong => self.push("</strong>"),
            TagEnd::Strikethrough => self.push("</del>"),
            TagEnd::Link | TagEnd::WikiLink => self.push("</a>"),
            TagEnd::Image => self.close_image(),
        }
    }
}

/// Escapes text for HTML content and attribute values, exactly as cmark does.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '"' => out.push_str("&quot;"),
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\0' => out.push('\u{fffd}'),
            c => out.push(c),
        }
    }
    out
}

/// Percent-escapes a URL for an `href`/`src` attribute, following cmark's
/// `houdini_escape_href`: alphanumerics and `-_.+!*(),#@?=;:/$~` pass through, `&`
/// and `'` become entities, an existing `%XX` escape is preserved, and everything
/// else is percent-encoded byte by byte.
pub fn escape_href(url: &str) -> String {
    const SAFE: &[u8] = b"-_.+!*(),#@?=;:/$~";
    let bytes = url.as_bytes();
    let mut out = String::with_capacity(url.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_alphanumeric() || SAFE.contains(&b) {
            out.push(b as char);
        } else {
            match b {
                b'&' => out.push_str("&amp;"),
                b'\'' => out.push_str("&#x27;"),
                b'%' => {
                    let hex = bytes.get(i + 1).is_some_and(u8::is_ascii_hexdigit)
                        && bytes.get(i + 2).is_some_and(u8::is_ascii_hexdigit);
                    if hex {
                        out.push_str(&url[i..=i + 2]);
                        i += 2;
                    } else {
                        out.push_str("%25");
                    }
                }
                0 => out.push_str("%EF%BF%BD"),
                b => {
                    let _ = write!(out, "%{b:02X}");
                }
            }
        }
        i += 1;
    }
    out
}

/// The GFM "Disallowed Raw HTML" tag list.
const DISALLOWED_TAGS: [&str; 9] = [
    "title",
    "textarea",
    "style",
    "xmp",
    "iframe",
    "noembed",
    "noframes",
    "script",
    "plaintext",
];

/// Whether a raw HTML string opens (or closes) one of the disallowed tags.
fn is_disallowed_raw_html(literal: &str) -> bool {
    let rest = match literal.strip_prefix('<') {
        Some(rest) => rest.strip_prefix('/').unwrap_or(rest),
        None => return false,
    };
    DISALLOWED_TAGS.iter().any(|tag| {
        rest.len() > tag.len()
            && rest[..tag.len()].eq_ignore_ascii_case(tag)
            && matches!(
                rest.as_bytes()[tag.len()],
                b' ' | b'\t' | b'\n' | b'\r' | b'>' | b'/'
            )
    })
}

/// Applies the GFM tag filter across a raw HTML block.
fn tagfilter_block(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        if is_disallowed_raw_html(&rest[i..]) {
            out.push_str("&lt;");
        } else {
            out.push('<');
        }
        rest = &rest[i + 1..];
    }
    out.push_str(rest);
    out
}
