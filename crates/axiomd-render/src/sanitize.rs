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
//!   syntect palettes, `data-line` for the source anchors, `id` for footnote
//!   targets, and the disabled task-list checkbox;
//! * no image may name a remote source. Markdown images are turned into load
//!   placeholders upstream; this is the backstop that catches an `<img src>` written
//!   as raw HTML, so that "zero implicit network" holds for any document, not just
//!   well-behaved ones.

use std::borrow::Cow;

use ammonia::Builder;

/// Cleans one templated document body.
pub(crate) fn clean(html: &str) -> String {
    builder().clean(html).to_string()
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

fn builder() -> Builder<'static> {
    let mut builder = Builder::default();
    builder
        .add_generic_attributes(["class", "data-line", "id"])
        .add_tags(["input"])
        .add_tag_attributes("input", ["checked", "disabled"])
        .add_tag_attributes("img", ["data-remote-src"])
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
        .attribute_filter(|element, attribute, value| match (element, attribute) {
            ("img", "src") if is_remote(value) => None,
            _ => Some(Cow::Borrowed(value)),
        });
    builder
}
