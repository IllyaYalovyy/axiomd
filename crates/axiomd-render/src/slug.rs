//! Heading anchors, spelled the way GitHub spells them.
//!
//! `guide.md#getting-started` is written once and has to work everywhere the
//! document is read. That makes the slug algorithm a compatibility contract rather
//! than a naming choice, so this follows `github-slugger` — the implementation
//! behind GitHub's own heading anchors — rather than inventing anything:
//!
//! 1. lowercase the heading's text;
//! 2. delete everything that is not a letter, a digit, `-` or `_`;
//! 3. turn each remaining space into `-`;
//! 4. disambiguate a repeat with `-1`, `-2`, … counting per original slug.
//!
//! Step 2 is `github-slugger`'s generated character class, which is every ASCII
//! control and punctuation character except `-` and `_`, plus the Unicode
//! punctuation, separator and symbol ranges. It is expressed here as "keep what is
//! alphanumeric", which agrees with it for letters, digits, punctuation, symbols and
//! emoji alike. The one place the two part company is a *combining mark* written
//! separately from its letter: `github-slugger` keeps it and this drops it, so
//! decomposed `e` + U+0301 slugs as `e` where composed `é` slugs as `é`. Text read
//! from a file is composed in practice, and closing the gap would mean carrying a
//! Unicode category table for it.

use std::collections::HashMap;

use axiomd_engine::{Event, Parsed, Tag, TagEnd};

/// One id per heading in `parsed`, in document order, already disambiguated.
///
/// An empty string means the heading had nothing sluggable in it — punctuation
/// alone, or nothing at all — and gets no anchor rather than one no link could name.
pub(crate) fn heading_ids(parsed: &Parsed<'_>) -> Vec<String> {
    let mut ids = Vec::new();
    let mut occurrences: HashMap<String, usize> = HashMap::new();
    let mut heading: Option<String> = None;
    // An image's label is alt text: GitHub slugs the heading's words, and the alt
    // text of an image inside it is not one of them.
    let mut in_image = 0usize;

    for spanned in parsed.events() {
        match &spanned.event {
            Event::Start(Tag::Heading { .. }) => heading = Some(String::new()),
            Event::End(TagEnd::Heading(_)) => {
                if let Some(text) = heading.take() {
                    ids.push(unique(slug(&text), &mut occurrences));
                }
            }
            Event::Start(Tag::Image { .. }) => in_image += 1,
            Event::End(TagEnd::Image) => in_image = in_image.saturating_sub(1),
            event => {
                if let Some(text) = heading.as_mut()
                    && in_image == 0
                {
                    match event {
                        Event::Text(run) | Event::Code(run) => text.push_str(run),
                        Event::Math { latex, .. } => text.push_str(latex),
                        // A setext heading spans two lines; the break is a control
                        // character, which step 2 deletes.
                        Event::SoftBreak | Event::HardBreak => text.push('\n'),
                        _ => {}
                    }
                }
            }
        }
    }
    ids
}

/// Steps 1 to 3: the slug of one heading's text, before disambiguation.
fn slug(text: &str) -> String {
    let mut slug = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        if c == ' ' {
            slug.push('-');
        } else if keeps(c) {
            slug.push(c);
        }
    }
    slug
}

/// Step 2, as a predicate.
fn keeps(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_alphanumeric() || c == '-' || c == '_';
    }
    c.is_alphanumeric()
}

/// Step 4, counting exactly as `github-slugger` counts: the suffix comes from how
/// many times the *original* slug has been asked for, and the result is itself
/// reserved — so a heading whose own slug collides with a generated one is pushed
/// along rather than stealing the other section's link.
fn unique(base: String, occurrences: &mut HashMap<String, usize>) -> String {
    if base.is_empty() {
        return base;
    }
    let mut candidate = base.clone();
    while occurrences.contains_key(&candidate) {
        let taken = occurrences
            .get_mut(&base)
            .expect("the base is reserved before any suffix derived from it");
        *taken += 1;
        candidate = format!("{base}-{taken}");
    }
    occurrences.insert(candidate.clone(), 0);
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn punctuation_is_deleted_and_spaces_become_hyphens() {
        assert_eq!(slug("Hello, World!"), "hello-world");
        assert_eq!(slug("a.b.c"), "abc");
        assert_eq!(slug("snake_case and-dashes"), "snake_case-and-dashes");
        assert_eq!(slug("  double  space "), "--double--space-");
    }

    /// The counter is per original slug, and every result is reserved.
    #[test]
    fn a_repeat_is_numbered_from_the_slug_it_repeats() {
        let mut seen = HashMap::new();
        assert_eq!(unique(slug("Notes"), &mut seen), "notes");
        assert_eq!(unique(slug("Notes"), &mut seen), "notes-1");
        assert_eq!(unique(slug("Notes"), &mut seen), "notes-2");
        assert_eq!(unique(slug("Notes 1"), &mut seen), "notes-1-1");
        assert_eq!(unique(slug("Notes 1"), &mut seen), "notes-1-2");
    }

    /// An unsluggable heading is not reserved, so it cannot start a numbering run
    /// of empty ids.
    #[test]
    fn a_heading_with_nothing_sluggable_in_it_has_no_id() {
        let mut seen = HashMap::new();
        assert_eq!(unique(slug("!!!"), &mut seen), "");
        assert_eq!(unique(slug("???"), &mut seen), "");
        assert!(seen.is_empty());
    }
}
