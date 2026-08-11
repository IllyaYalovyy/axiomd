//! The desktop's list of recently used documents, read from the world one launch lives
//! in (issue #52).
//!
//! An application does not keep a recent list of its own: it tells the desktop that a
//! document was opened, and the desktop is what shows it — in Files → Recent, in the
//! file chooser's Recent view, in Shell search. On a freedesktop desktop that list is
//! one file, `recently-used.xbel` under `XDG_DATA_HOME`, written by GTK's recent
//! manager, and reading it is how a test sees what the reader would see there.
//!
//! Which world's file matters more than usual. Every launch this harness makes is
//! pinned to a data directory inside its own scratch, so a thousand test opens write a
//! thousand entries nobody will ever see and not one into the developer's real list
//! (issue #44). What is read here is that pinned file, and a launch that wrote
//! somewhere else fails `containment::confirm` before a test gets as far as looking.
//!
//! # Reading it
//!
//! The file is XBEL — bookmarks with freedesktop metadata — and the shape GLib's
//! `g_bookmark_file` writes it in is fixed: one `<bookmark href="…">` per document,
//! holding one `<mime:mime-type type="…"/>` and one `<bookmark:application name="…">`
//! per application that registered it (probed on GTK 4.20.4; the file this parser was
//! written against is in the issue). Reading it by hand rather than with an XML parser
//! keeps a dependency out of the harness for the sake of four attributes, and a file
//! that does not have the expected shape reads as no entries at all — which fails the
//! test that was waiting for one, saying so.

use std::path::{Path, PathBuf};

use gtk::gio;
use gtk::prelude::*;

/// The desktop's recent list, relative to the data directory it lives in.
const LIST: &str = "recently-used.xbel";

/// One document in the desktop's list of recently used documents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentDocument {
    /// The document itself — the path the desktop would open, decoded from the `file:`
    /// URI the entry is keyed on.
    pub document: PathBuf,
    /// What the desktop has been told this document is, which is what decides the icon
    /// beside it and the application that opens it.
    pub kind: String,
    /// The applications that registered it, by the name each gave for itself.
    pub applications: Vec<String>,
}

/// Every document in the recent list of the desktop whose data directory is `data`, in
/// the order the file holds them, and none at all when nothing has ever been registered
/// there.
pub(crate) fn documents(data: &Path) -> Vec<RecentDocument> {
    let Ok(stored) = std::fs::read_to_string(data.join(LIST)) else {
        return Vec::new();
    };
    stored
        .split("<bookmark href=\"")
        .skip(1)
        .filter_map(entry)
        .collect()
}

/// One bookmark, from just after its `href` to the end of the file.
fn entry(bookmark: &str) -> Option<RecentDocument> {
    let (uri, rest) = bookmark.split_once('"')?;
    let held = rest.split("</bookmark>").next().unwrap_or(rest);
    Some(RecentDocument {
        // The desktop's own answer to what a `file:` URI names, percent-encoding and
        // all — the same call the application spelled it with.
        document: gio::File::for_uri(&unescaped(uri)).path()?,
        kind: attributes(held, "<mime:mime-type type=\"")
            .into_iter()
            .next()
            .unwrap_or_default(),
        applications: attributes(held, "<bookmark:application name=\""),
    })
}

/// Every value of the attribute `opened` opens, in the order they appear.
fn attributes(held: &str, opened: &str) -> Vec<String> {
    held.split(opened)
        .skip(1)
        .filter_map(|rest| rest.split_once('"'))
        .map(|(value, _)| unescaped(value))
        .collect()
}

/// An attribute value as it was before XML got hold of it. The five entities XML
/// escapes are the five it can hold.
fn unescaped(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The file GTK 4.20.4 wrote in the probe on this module's header, with a second
    /// document added by hand — so this parser is held to the real format rather than
    /// to what it expects.
    const AS_GTK_WROTE_IT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<xbel version="1.0"
      xmlns:bookmark="http://www.freedesktop.org/standards/desktop-bookmarks"
      xmlns:mime="http://www.freedesktop.org/standards/shared-mime-info"
>
  <bookmark href="file:///home/reader/Reading%20Notes/chapter%203.md" added="2026-08-11T15:24:35.825090Z" modified="2026-08-11T15:24:35.825093Z" visited="2026-08-11T15:24:35.825090Z">
    <info>
      <metadata owner="http://freedesktop.org">
        <mime:mime-type type="text/markdown"/>
        <bookmark:applications>
          <bookmark:application name="io.github.etf.axiomd" exec="&apos;axiomd %u&apos;" modified="2026-08-11T15:24:35.825091Z" count="1"/>
        </bookmark:applications>
      </metadata>
    </info>
  </bookmark>
  <bookmark href="file:///home/reader/guide.md" added="2026-08-11T15:24:36.825090Z" modified="2026-08-11T15:24:36.825093Z" visited="2026-08-11T15:24:36.825090Z">
    <info>
      <metadata owner="http://freedesktop.org">
        <mime:mime-type type="text/plain"/>
        <bookmark:applications>
          <bookmark:application name="io.github.etf.axiomd" exec="&apos;axiomd %u&apos;" modified="2026-08-11T15:24:36.825091Z" count="2"/>
          <bookmark:application name="Text Editor" exec="&apos;gnome-text-editor %U&apos;" modified="2026-08-11T15:24:36.825091Z" count="1"/>
        </bookmark:applications>
      </metadata>
    </info>
  </bookmark>
</xbel>
"#;

    fn read(stored: &str) -> Vec<RecentDocument> {
        let scratch = crate::Scratch::new("recent-parse");
        std::fs::write(scratch.path().join(LIST), stored).expect("write a recent list");
        documents(scratch.path())
    }

    /// Every document in the file, with the name a reader would recognise it by — a
    /// document whose own name holds a space included, because that name is
    /// percent-encoded in the file and a test that read it back with the encoding
    /// still on it would pass while asserting a path nobody has.
    #[test]
    fn a_recent_list_reads_back_as_the_documents_the_desktop_would_show() {
        let listed = read(AS_GTK_WROTE_IT);

        assert_eq!(
            listed
                .iter()
                .map(|entry| entry.document.display().to_string())
                .collect::<Vec<_>>(),
            [
                "/home/reader/Reading Notes/chapter 3.md",
                "/home/reader/guide.md",
            ],
        );
        assert_eq!(listed[0].kind, "text/markdown");
        assert_eq!(listed[0].applications, ["io.github.etf.axiomd"]);
        assert_eq!(
            listed[1].applications,
            ["io.github.etf.axiomd", "Text Editor"],
            "a document several applications have opened lost one of them",
        );
    }

    /// A desktop nothing has ever been registered with — the state every launch here
    /// starts in — and one whose file is not what this parser expects. Both are "the
    /// list is empty", which is what makes a test waiting for an entry fail on the
    /// entry rather than on an unwrap.
    #[test]
    fn a_desktop_with_no_recent_documents_reads_as_none() {
        let scratch = crate::Scratch::new("recent-empty");

        assert_eq!(documents(scratch.path()), Vec::new());
        assert_eq!(read("not a bookmark file at all"), Vec::new());
    }
}
