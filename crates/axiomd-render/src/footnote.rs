//! Footnotes as a reader meets them: numbered in the order they are referred to, and
//! linked in both directions.
//!
//! A label is what the *author* calls a footnote; a number is what the *reader* sees.
//! The two are not the same thing and the mapping between them is a property of the
//! whole document, not of any one event — `[^method]` is footnote 1 because it is
//! referred to first, whatever it is called and wherever its definition is written. So
//! the document is read once, ahead of the walk, exactly as heading anchors are
//! ([`crate::slug`]).
//!
//! # Where the definitions stay
//!
//! Where the author wrote them. GitHub and Obsidian both collect footnotes into a
//! section at the foot of the page; axiomd cannot, because the anchor map is in
//! document order with strictly increasing lines and every feature that navigates a
//! document rides on it (invariant 3). Moving a definition would move its anchor out
//! of order and break outline tracking, scroll sync, search and live reload at once.
//! A definition written at the end of the document — which is where all but a
//! vanishing few are — therefore renders exactly where Obsidian puts it anyway.

use std::collections::HashMap;

use axiomd_engine::{Event, Parsed};

/// The document's footnotes: what each label is called in front of the reader, and how
/// many places refer to it.
#[derive(Debug, Default)]
pub(crate) struct Footnotes {
    /// Label to `(number, references)`.
    numbered: HashMap<String, (usize, usize)>,
    /// How many references to each label the walk has written so far, which is what
    /// gives every one of them an id of its own to be sent back to.
    written: HashMap<String, usize>,
}

/// One reference, as the renderer needs to write it.
pub(crate) struct Reference {
    /// The number the reader sees.
    pub(crate) number: usize,
    /// Which reference to this label it is, counting from one — what tells two
    /// references to the same footnote apart so each can be jumped back to.
    pub(crate) nth: usize,
}

impl Footnotes {
    /// Reads the whole document's footnotes: the order references appear in decides
    /// the numbering, and repeated references to one label share its number.
    pub(crate) fn of(parsed: &Parsed<'_>) -> Footnotes {
        let mut numbered: HashMap<String, (usize, usize)> = HashMap::new();
        let mut next = 0;
        for spanned in parsed.events() {
            let Event::FootnoteReference(label) = &spanned.event else {
                continue;
            };
            let entry = numbered.entry(label.to_string()).or_insert_with(|| {
                next += 1;
                (next, 0)
            });
            entry.1 += 1;
        }
        Footnotes {
            numbered,
            written: HashMap::new(),
        }
    }

    /// The next reference to `label`, or `None` for a reference to a footnote nothing
    /// defines — which the parser does not produce, and which would have no number.
    pub(crate) fn referenced(&mut self, label: &str) -> Option<Reference> {
        let (number, _) = *self.numbered.get(label)?;
        let written = self.written.entry(label.to_owned()).or_insert(0);
        *written += 1;
        Some(Reference {
            number,
            nth: *written,
        })
    }

    /// How the definition of `label` is labelled, and how many references point at
    /// it: `None` for a definition nothing refers to, which is shown as the author
    /// wrote it and has nowhere to send anybody back to.
    pub(crate) fn defined(&self, label: &str) -> Option<(usize, usize)> {
        self.numbered.get(label).copied()
    }
}
