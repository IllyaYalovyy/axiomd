//! The last gate every byte of a rendered document passes through.
//!
//! The pipeline templates untrusted markdown — including verbatim raw HTML blocks
//! and inline tags — into one body string, and this module cleans that whole string
//! once, as a tree. Cleaning per event is not an option: CommonMark splits raw HTML
//! at blank lines, so `<div align="center">` and its `</div>` arrive as separate
//! events, and a fragment cleaner given half a tree would balance each half and
//! destroy the document.
//!
//! Two rules beyond ammonia's defaults matter here:
//!
//! * the pipeline's own vocabulary survives — `class` for the stylesheet and the
//!   syntect palettes, `data-line` for the source anchors, `id` for heading and
//!   footnote targets, `data-remote-src` for the load placeholders, the `axiomd:`
//!   scheme those placeholders link to, and the disabled task-list checkbox;
//! * no image may name a remote source. Markdown images are turned into load
//!   placeholders upstream; this is the backstop that catches an `<img src>` written
//!   as raw HTML, so that "zero implicit network" holds for any document, not just
//!   well-behaved ones.
//!
//! MathML is the third rule and the one that needed a vocabulary of its own: an
//! equation is markup, not a picture, so the elements the math plugin writes have to
//! survive this gate. What is admitted is the presentation vocabulary of MathML Core
//! and nothing beside it — see [`MATHML`].
//!
//! Letting `axiomd:` through as a link scheme is not a grant: it is a scheme only
//! the app answers for, every request on it is decided by the app's own navigation
//! policy, and a document forging one gets exactly what an ordinary link gets.

use std::borrow::Cow;

use ammonia::Builder;

/// Cleans one templated document body.
pub(crate) fn clean(html: &str) -> String {
    builder(false).clean(html).to_string()
}

/// The same, for a document being written to a file that leaves axiomd.
///
/// One difference, and it is the whole reason an exported document can be opened
/// anywhere: a picture travelling inside the file is a `data:` image, which the rule
/// above exists to strip. It is admitted here by exactly the shape the pipeline
/// itself writes — a base64 image and nothing else — so a `data:text/html` smuggled
/// into a document's raw HTML is still removed, and a remote source still is too.
pub(crate) fn clean_for_a_file(html: &str) -> String {
    builder(true).clean(html).to_string()
}

/// The elements an equation is built out of: the presentation vocabulary of MathML
/// Core (W3C Recommendation, §3), which is the whole of what WebKitGTK lays out.
///
/// Three of the specification's elements are deliberately absent. `<maction>` is
/// interactive and MathML Core dropped it; `<mglyph>` names a picture by URL, which is
/// a fetch this document may not make; and `<annotation-xml>` carries a foreign
/// document — HTML or SVG — inside the equation, which is a way into the page that
/// nothing here needs. An equation missing one of them degrades to markup that reads
/// as text, which is what a best-effort renderer owes a document it cannot fully draw.
const MATHML: [&str; 29] = [
    "math",
    "mi",
    "mn",
    "mo",
    "ms",
    "mtext",
    "mspace",
    "mrow",
    "mfrac",
    "msqrt",
    "mroot",
    "mstyle",
    "merror",
    "mpadded",
    "mphantom",
    "msub",
    "msup",
    "msubsup",
    "munder",
    "mover",
    "munderover",
    "mmultiscripts",
    "mprescripts",
    "none",
    "mtable",
    "mtr",
    "mtd",
    "semantics",
    "annotation",
];

/// What those elements may say about themselves: MathML Core's global attributes and
/// the per-element ones, as one set rather than a table.
///
/// One set because none of them is a way to do anything — they are sizes, alignments
/// and typographic switches — so which element may carry which is the layout engine's
/// business and not this gate's. `class` and `id` are already generic here, and
/// `style` is admitted separately and filtered down to the six properties the renderer
/// writes.
const MATHML_ATTRIBUTES: [&str; 29] = [
    "accent",
    "accentunder",
    "columnspan",
    "depth",
    "dir",
    "display",
    "displaystyle",
    "encoding",
    "fence",
    "form",
    "height",
    "largeop",
    "linethickness",
    "lspace",
    "mathbackground",
    "mathcolor",
    "mathsize",
    "mathvariant",
    "maxsize",
    "minsize",
    "movablelimits",
    "rowspan",
    "rspace",
    "scriptlevel",
    "separator",
    "stretchy",
    "symmetric",
    "voffset",
    "width",
];

/// The declarations a `style` attribute may hold, which is the whole of what the
/// MathML renderer puts in one: colour for `\color` and for an error's border, and the
/// three lengths it spaces a formula with. Nothing here can position an element or
/// cover the page, which is why `style` — admitted nowhere else in a document — is
/// admitted at all.
const MATHML_STYLE: [&str; 6] = [
    "color",
    "background-color",
    "border",
    "border-color",
    "margin-left",
    "height",
];

/// Whether `url` is a picture carried inside the document itself.
fn is_embedded_image(url: &str) -> bool {
    url.starts_with("data:image/") && url[..url.find(',').unwrap_or(0)].ends_with(";base64")
}

/// Whether `url` points outside the document's own directory.
///
/// Anything with a scheme (`https:`, `data:`, `file:`) or a protocol-relative
/// prefix is remote; only document-relative paths and fragments are local, and the
/// app resolves those against the document's `axiomd://` base.
pub(crate) fn is_remote(url: &str) -> bool {
    let url = url.trim();
    if url.starts_with("//") {
        return true;
    }
    let scheme_end = url.find(|c: char| !c.is_ascii_alphanumeric() && !"+-.".contains(c));
    matches!(scheme_end, Some(end) if end > 0
        && url[end..].starts_with(':')
        && url.starts_with(|c: char| c.is_ascii_alphabetic()))
}

/// The cleaner, in the one shape both destinations share.
///
/// `carrying_its_pictures` is the single difference between them: a document being
/// written to a file holds its images as `data:` bytes, and a document on screen may
/// never hold anything but a reference the app's own scheme answers for.
fn builder(carrying_its_pictures: bool) -> Builder<'static> {
    let mut builder = Builder::default();
    if carrying_its_pictures {
        builder.add_url_schemes(["data"]);
    }
    builder.add_tags(MATHML);
    for element in MATHML {
        builder.add_tag_attributes(element, MATHML_ATTRIBUTES);
        builder.add_tag_attributes(element, ["style"]);
    }
    builder
        .filter_style_properties(MATHML_STYLE.into())
        .add_generic_attributes(["class", "data-line", "id"])
        .add_tags(["input"])
        .add_tag_attributes("input", ["checked", "disabled"])
        // A foldable callout is a `<details>` that starts open or shut, which is how
        // folding happens in a document that cannot run a script. `open` is the whole
        // of what a document may say about it: a boolean attribute with no value, so
        // there is nothing here for a hostile document to smuggle anything through.
        .add_tag_attributes("details", ["open"])
        .add_tag_attributes("a", ["data-remote-src"])
        .add_url_schemes(["axiomd"])
        // `<div align="center">` is how a README centres its badges; the HTML
        // standard still maps the attribute to `text-align`, and it is presentation
        // only, so keeping it costs nothing and matches what GitHub shows.
        .add_tag_attributes("div", ["align"])
        .add_tag_attributes("p", ["align"])
        // An `<input>` written by the document itself is forced into the only shape
        // this pipeline has a meaning for: a task-list checkbox. Everything else an
        // input could be — a text field, a file picker, a submit button — loses its
        // type here rather than appearing in the middle of someone's prose.
        //
        // Exactly one forced value, deliberately: ammonia applies these from a hash
        // map, so a second one would order the two attributes differently from run
        // to run and the golden documents would not be reproducible.
        .set_tag_attribute_value("input", "type", "checkbox")
        .attribute_filter(
            move |element, attribute, value| match (element, attribute) {
                ("img", "src") if carrying_its_pictures && is_embedded_image(value) => {
                    Some(Cow::Borrowed(value))
                }
                ("img", "src") if is_remote(value) => None,
                _ => Some(Cow::Borrowed(value)),
            },
        );
    builder
}
