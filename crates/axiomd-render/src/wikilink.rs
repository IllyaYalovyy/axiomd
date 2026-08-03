//! Where a `[[wikilink]]` goes.
//!
//! There is no vault (`ux_decisions.md`): the document's own folder is the root, and a
//! wikilink reaches the Markdown files under it and nothing else. Resolution is the
//! rule issue #12 states — the exact relative path first, then a basename that names
//! exactly one document in the tree — and an ambiguous or missing target resolves to
//! nothing at all rather than to a guess.
//!
//! # Why the folder is data
//!
//! The pipeline is pure: it opens no file, so it cannot look at the reader's disk to
//! decide where a link goes. [`Folder`] is the answer to that — the app walks the
//! document's directory on the worker it renders on and hands the result in, and this
//! module resolves against a list rather than against a filesystem. The rule is
//! therefore unit-testable without a scratch directory, and "rendering touches no
//! disk" stays a property of the code rather than a promise about the caller.

use crate::slug;

/// What a resolved wikilink points at.
pub(crate) struct Destination {
    /// The document-relative path the link's `href` carries — the same shape a plain
    /// relative Markdown link has, so a click travels the path issue #6 already built.
    pub(crate) href: String,
}

/// The Markdown documents beside the one being rendered — the whole of what a
/// wikilink in it can reach.
///
/// Cheap to build and to hand over: it is one list of paths, relative to the
/// document's own directory, built fresh for each render.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Folder {
    documents: Vec<String>,
}

impl Folder {
    /// Nothing beside the document: an untitled document, a folder that could not be
    /// read, or a render that has no reader — every wikilink in it is unresolved.
    pub fn empty() -> Folder {
        Folder::default()
    }

    /// The documents under the rendered document's own directory, each named by its
    /// path relative to that directory (`guide.md`, `notes/setup.md`).
    ///
    /// Order does not matter and duplicates are harmless: what decides a link is
    /// whether a path is here and whether a basename is here once.
    pub fn holding(documents: impl IntoIterator<Item = String>) -> Folder {
        Folder {
            documents: documents.into_iter().collect(),
        }
    }

    /// Where `target` goes, or `None` when it goes nowhere: nothing in the folder has
    /// that name, or more than one thing does.
    ///
    /// `target` is the wikilink as written, `#heading` and all.
    pub(crate) fn resolve(&self, target: &str) -> Option<Destination> {
        let (path, heading) = match target.split_once('#') {
            Some((path, heading)) => (path.trim(), Some(heading.trim())),
            None => (target.trim(), None),
        };
        let fragment = match heading {
            Some(heading) if !heading.is_empty() => format!("#{}", slug::of(heading)),
            _ => String::new(),
        };
        // `[[#section]]` is a link inside this very document, which needs no file at
        // all — and must not be resolved against one, or a document with a sibling of
        // the same name would send the reader out of the page they are reading.
        if path.is_empty() {
            return (!fragment.is_empty()).then_some(Destination { href: fragment });
        }
        let path = path.strip_prefix("./").unwrap_or(path);
        let found = self.exactly(path).or_else(|| self.by_basename(path))?;
        Some(Destination {
            href: format!("{found}{fragment}"),
        })
    }

    /// The document `path` names outright, with the extension it may have left off.
    fn exactly(&self, path: &str) -> Option<&str> {
        [
            path.to_owned(),
            format!("{path}.md"),
            format!("{path}.markdown"),
        ]
        .into_iter()
        .find_map(|wanted| {
            self.documents
                .iter()
                .find(|document| *document == &wanted)
                .map(String::as_str)
        })
    }

    /// The one document in the tree whose own name is `path`'s — or `None` when
    /// several are, because a link that could mean two documents means neither.
    fn by_basename(&self, path: &str) -> Option<&str> {
        if path.contains('/') {
            return None;
        }
        let wanted = stem_of(path);
        let mut found: Option<&str> = None;
        for document in &self.documents {
            if stem_of(document) != wanted {
                continue;
            }
            if found.is_some_and(|already| already != document) {
                return None;
            }
            found = Some(document);
        }
        found
    }
}

/// A path's own name without its Markdown extension: what `[[setup]]` is compared
/// against for `notes/setup.md`.
fn stem_of(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    for extension in [".md", ".markdown"] {
        if let Some(stem) = name.strip_suffix(extension) {
            return stem;
        }
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder() -> Folder {
        Folder::holding(
            [
                "guide.md",
                "notes/setup.md",
                "notes/deep/setup.md",
                "notes/release notes.md",
                "archive/guide.md",
                "alone.markdown",
            ]
            .map(str::to_owned),
        )
    }

    fn href(target: &str) -> Option<String> {
        folder().resolve(target).map(|found| found.href)
    }

    /// The exact relative path wins, extension written or left off.
    #[test]
    fn an_exact_relative_path_resolves_to_that_document() {
        assert_eq!(href("guide.md").as_deref(), Some("guide.md"));
        assert_eq!(href("guide").as_deref(), Some("guide.md"));
        assert_eq!(href("./guide").as_deref(), Some("guide.md"));
        assert_eq!(href("notes/setup.md").as_deref(), Some("notes/setup.md"));
        assert_eq!(href("notes/setup").as_deref(), Some("notes/setup.md"));
        assert_eq!(href("alone").as_deref(), Some("alone.markdown"));
        assert_eq!(
            href("notes/release notes").as_deref(),
            Some("notes/release notes.md"),
        );
    }

    /// A bare name resolves when exactly one document in the tree has it — and the
    /// exact path is preferred over it, which is what `guide` means here even though
    /// two documents are called that.
    #[test]
    fn a_bare_name_resolves_when_one_document_in_the_tree_has_it() {
        assert_eq!(
            href("release notes").as_deref(),
            Some("notes/release notes.md")
        );
        // `guide` is both a path in the root and a basename twice over: the exact
        // path wins, so the reader lands where they wrote.
        assert_eq!(href("guide").as_deref(), Some("guide.md"));
    }

    /// An ambiguous basename resolves to nothing: two documents called `setup` mean
    /// the link means neither, deterministically and whatever order the folder was
    /// listed in.
    #[test]
    fn an_ambiguous_basename_resolves_to_nothing() {
        assert_eq!(href("setup"), None);
        assert_eq!(href("setup.md"), None);

        let reversed =
            Folder::holding(["notes/deep/setup.md", "notes/setup.md"].map(str::to_owned));
        assert_eq!(reversed.resolve("setup").map(|found| found.href), None);
    }

    /// A path that names nothing is unresolved, and a path is never guessed at from a
    /// folder it does not name.
    #[test]
    fn a_target_nothing_answers_to_resolves_to_nothing() {
        assert_eq!(href("missing"), None);
        assert_eq!(href("notes/missing.md"), None);
        // A basename is only a basename: a path with a folder in it must match a path.
        assert_eq!(href("deep/setup"), None);
        assert_eq!(Folder::empty().resolve("guide").map(|f| f.href), None);
    }

    /// A heading travels as the anchor the rendered document gives it, so
    /// `[[guide#Getting Started]]` lands on the same section `guide.md#getting-started`
    /// does.
    #[test]
    fn a_heading_becomes_the_anchor_the_rendered_document_gives_it() {
        assert_eq!(
            href("guide#Getting Started").as_deref(),
            Some("guide.md#getting-started"),
        );
        assert_eq!(href("guide.md#Setup").as_deref(), Some("guide.md#setup"));
        assert_eq!(href("guide#").as_deref(), Some("guide.md"));
        // A heading with no document is a section of this one.
        assert_eq!(
            href("#Getting Started").as_deref(),
            Some("#getting-started")
        );
        assert_eq!(href("#"), None);
        assert_eq!(href(""), None);
    }
}
