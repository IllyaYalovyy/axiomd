//! What a document says about itself.
//!
//! One question so far, and it is asked in three places at once: the `<title>` of the
//! page on screen, the name a print job appears under, and the title recorded in an
//! exported PDF's metadata are all the same string, because they are all the same
//! document. WebKit takes all three from the page's own title element, so getting it
//! right here is the whole of getting it right there.
//!
//! Frontmatter is metadata and is never rendered (`ux_decisions.md`); this is the one
//! thing the document's frontmatter is read for.

use axiomd_engine::{Event, Parsed, Tag, TagEnd};

/// What to call this document: what its frontmatter says, else its first heading,
/// else `name` — which is what the reader calls the file.
pub(crate) fn title(parsed: &Parsed<'_>, name: &str) -> String {
    front_matter_title(parsed.front_matter().unwrap_or_default())
        .or_else(|| first_heading(parsed))
        .unwrap_or_else(|| name.to_owned())
}

/// The `title:` of a YAML frontmatter block, as the one field axiomd reads.
///
/// Deliberately a scan of top-level `key: value` lines rather than a YAML parser: a
/// document title is a scalar, a wrong guess about anything more elaborate would put
/// somebody's stray text in a window title, and a YAML dependency to read one field
/// is not a trade this crate makes.
fn front_matter_title(front_matter: &str) -> Option<String> {
    for line in front_matter.lines() {
        // The delimiters, and anything nested under another key.
        if line.starts_with("---") || line.starts_with(char::is_whitespace) {
            continue;
        }
        if let Some(value) = line.strip_prefix("title:") {
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .or_else(|| {
                    value
                        .strip_prefix('\'')
                        .and_then(|value| value.strip_suffix('\''))
                })
                .unwrap_or(value);
            if !value.is_empty() {
                return Some(value.to_owned());
            }
        }
    }
    None
}

/// The text of the document's first heading that has any, however deeply the markup
/// inside it is nested — a title is words, not markup, and a heading made of nothing
/// but an image is not a title at all.
fn first_heading(parsed: &Parsed<'_>) -> Option<String> {
    let mut text = String::new();
    let mut inside = false;
    for spanned in parsed.events() {
        match (&spanned.event, inside) {
            (Event::Start(Tag::Heading { .. }), _) => inside = true,
            (Event::End(TagEnd::Heading(_)), true) => {
                if !text.trim().is_empty() {
                    return Some(text.trim().to_owned());
                }
                inside = false;
                text.clear();
            }
            (Event::Text(words), true) | (Event::Code(words), true) => text.push_str(words),
            (Event::SoftBreak, true) => text.push(' '),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use axiomd_engine::{ComrakEngine, Extensions, MarkdownEngine};

    use super::*;

    fn title_of(source: &str) -> String {
        let parsed = ComrakEngine::new().parse(source, Extensions::FULL);
        title(&parsed, "the file name")
    }

    #[test]
    fn a_document_titles_itself_before_its_file_does() {
        assert_eq!(
            title_of("---\ntitle: Written Down\n---\n\n# A Heading\n"),
            "Written Down",
        );
        assert_eq!(title_of("# A Heading\n\nText.\n"), "A Heading");
        assert_eq!(title_of("Just text.\n"), "the file name");
        assert_eq!(title_of(""), "the file name");
    }

    /// A title is the words of the heading, not the markup that decorates them.
    #[test]
    fn a_heading_title_is_its_words() {
        assert_eq!(
            title_of("# A *decorated* `heading`\n"),
            "A decorated heading"
        );
        assert_eq!(
            title_of("## Only a second-level heading\n"),
            "Only a second-level heading"
        );
        assert_eq!(
            title_of("# \n\n# Second\n"),
            "Second",
            "an empty heading is no title"
        );
    }

    /// Frontmatter is YAML, and the quoting it allows is not decoration to strip
    /// blindly — nor is a nested `title:` belonging to some other key the title.
    #[test]
    fn a_frontmatter_title_is_read_the_way_it_was_written() {
        assert_eq!(title_of("---\ntitle: \"Quoted\"\n---\n\ntext\n"), "Quoted");
        assert_eq!(title_of("---\ntitle: 'Quoted'\n---\n\ntext\n"), "Quoted");
        assert_eq!(
            title_of("---\nauthor:\n  title: Not This One\n---\n\n# This One\n"),
            "This One",
        );
        assert_eq!(
            title_of("---\ntitle:\n---\n\n# The Heading\n"),
            "The Heading",
            "an empty frontmatter title is not a title",
        );
    }
}
