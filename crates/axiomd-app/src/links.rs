//! What happens when the reader clicks something.
//!
//! A rendered document cannot run a script, so every link in it arrives here as a
//! navigation the view is about to make, and this module is the whole of the policy
//! that decides it (UT-007). The rule the classes below encode: the view itself only
//! ever stays on the document it was given. Anything else — another file, the
//! browser, the desktop's own handler, a request the document is making of the app —
//! is refused as a navigation and done deliberately instead.
//!
//! Two properties are worth stating because the tests are built on them:
//!
//! * **Nothing leaves the app without a click.** Everything but staying put requires
//!   the navigation to be a link activation; a redirect, a script-less document's own
//!   first load, or anything else the engine initiates can only stay.
//! * **A document's reach is its own directory.** A relative link resolves under the
//!   document's folder and may not climb out of it, by path or by symlink — the same
//!   rule the `axiomd://` scheme applies to the bytes a document may read.

use std::path::{Path, PathBuf};

use axiomd_render::Request;

use crate::scheme::path_under;

/// What the app does about one navigation a document tried to make.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Follow {
    /// The view may go there itself: the document it is already showing, or a
    /// fragment inside it.
    Stay,
    /// Another Markdown document in the reader's own folder, shown in this window
    /// with back and forward.
    Document {
        file: PathBuf,
        fragment: Option<String>,
    },
    /// A file beside the document that axiomd does not render: the desktop opens it.
    Attachment { file: PathBuf },
    /// Somewhere outside this computer: the default browser opens it.
    External { uri: String },
    /// The document asking the app for something — the one-click image loads.
    Ask(Request),
    /// None of the above, and therefore nothing at all.
    Refuse,
}

/// Decides one navigation.
///
/// `here` is the URI the view is currently showing (empty before its first load),
/// `root` the directory the document lives in — `None` for an untitled document,
/// which lives nowhere and can therefore reach nothing — `target` where the
/// navigation would go, and `activated` whether the reader clicked a link to cause it.
pub(crate) fn follow(here: &str, root: Option<&Path>, target: &str, activated: bool) -> Follow {
    // The view's own URI carries the fragment it was sent to; the page it is on is
    // what a target has to match to be "this document".
    let page = before_fragment(here);
    if page.is_empty() {
        // The document's own first load, before the view has a URI of its own.
        return if target.starts_with("axiomd://") {
            Follow::Stay
        } else {
            Follow::Refuse
        };
    }

    let (destination, fragment) = split_fragment(target);
    if destination == page {
        return Follow::Stay;
    }
    if !activated {
        return Follow::Refuse;
    }

    if let Some(request) = Request::from_uri(target) {
        return Follow::Ask(request);
    }
    if let Some(relative) = destination.strip_prefix(page) {
        // Relative to what: an untitled document has no folder, so a link into one is
        // a link to nowhere rather than a link into whatever directory axiomd was
        // started in.
        let Some(root) = root else {
            return Follow::Refuse;
        };
        let Some(file) = decode(relative).and_then(|relative| path_under(root, &relative)) else {
            return Follow::Refuse;
        };
        return if is_markdown(&file) {
            Follow::Document {
                file,
                fragment: fragment.map(str::to_owned),
            }
        } else {
            Follow::Attachment { file }
        };
    }
    if opens_in_a_browser(target) {
        return Follow::External {
            uri: target.to_owned(),
        };
    }
    Follow::Refuse
}

/// The extensions axiomd claims (`ux_decisions.md`: Markdown files only). Anything
/// else beside the document belongs to whatever the desktop opens it with.
fn is_markdown(file: &Path) -> bool {
    file.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown")
        })
}

/// The schemes a link may hand to the desktop.
///
/// Deliberately three rather than "anything with a scheme": `file:` would turn a
/// document into a way to open arbitrary local paths, and `data:` a way to hand the
/// browser content the reader never saw.
fn opens_in_a_browser(target: &str) -> bool {
    let scheme = match target.split_once(':') {
        Some((scheme, _)) => scheme,
        None => return false,
    };
    scheme.eq_ignore_ascii_case("http")
        || scheme.eq_ignore_ascii_case("https")
        || scheme.eq_ignore_ascii_case("mailto")
}

fn before_fragment(uri: &str) -> &str {
    split_fragment(uri).0
}

fn split_fragment(uri: &str) -> (&str, Option<&str>) {
    match uri.split_once('#') {
        Some((before, after)) => (before, Some(after)),
        None => (uri, None),
    }
}

/// Percent-decoding, because a link to `release notes.md` arrives spelled
/// `release%20notes.md`. Anything that is not valid UTF-8 once decoded names no file.
fn decode(request: &str) -> Option<String> {
    let bytes = request.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let digits = request.get(at + 1..at + 3)?;
            decoded.push(u8::from_str_radix(digits, 16).ok()?);
            at += 3;
        } else {
            decoded.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::ScratchDir;

    const HERE: &str = "axiomd://doc-3/";

    /// A folder holding the four things a document can link to.
    fn folder() -> ScratchDir {
        let scratch = ScratchDir::new("links");
        scratch.write("notes.md", "# Notes\n");
        scratch.write("guide.markdown", "# Guide\n");
        scratch.write("release notes.md", "# Release\n");
        scratch.write("report.pdf", "%PDF-1.4\n");
        scratch.write("images/logo.png", b"\x89PNG\r\n\x1a\n");
        scratch
    }

    fn following(target: &str, scratch: &ScratchDir) -> Follow {
        follow(HERE, Some(scratch.path()), target, true)
    }

    /// UT-007, class by class.
    #[test]
    fn a_relative_markdown_link_opens_that_document() {
        let scratch = folder();

        assert_eq!(
            following("axiomd://doc-3/notes.md", &scratch),
            Follow::Document {
                file: scratch.path().join("notes.md"),
                fragment: None,
            },
        );
        assert_eq!(
            following("axiomd://doc-3/guide.markdown#setup", &scratch),
            Follow::Document {
                file: scratch.path().join("guide.markdown"),
                fragment: Some("setup".to_owned()),
            },
        );
        // The spelling a browser gives a link whose target has a space in its name.
        assert_eq!(
            following("axiomd://doc-3/release%20notes.md", &scratch),
            Follow::Document {
                file: scratch.path().join("release notes.md"),
                fragment: None,
            },
        );
    }

    #[test]
    fn an_anchor_in_this_document_is_left_to_the_view() {
        let scratch = folder();

        assert_eq!(
            following("axiomd://doc-3/#getting-started", &scratch),
            Follow::Stay
        );
        assert_eq!(following("axiomd://doc-3/", &scratch), Follow::Stay);
        // The view is already on a fragment and the reader clicks another one.
        assert_eq!(
            follow(
                "axiomd://doc-3/#first",
                Some(scratch.path()),
                "axiomd://doc-3/#second",
                true
            ),
            Follow::Stay,
        );
    }

    /// An untitled document lives nowhere, so a relative link in one reaches nothing
    /// — not the directory axiomd happens to have been started in.
    #[test]
    fn a_link_in_an_untitled_document_reaches_nothing_on_disk() {
        assert_eq!(
            follow(HERE, None, "axiomd://doc-3/notes.md", true),
            Follow::Refuse,
        );
        assert_eq!(
            follow(HERE, None, "axiomd://doc-3/report.pdf", true),
            Follow::Refuse,
        );
        // What an untitled document can still do: leave the app on a click, and stay
        // where it is on its own fragments.
        assert_eq!(
            follow(HERE, None, "https://example.com/", true),
            Follow::External {
                uri: "https://example.com/".to_owned()
            },
        );
        assert_eq!(
            follow(HERE, None, "axiomd://doc-3/#top", true),
            Follow::Stay
        );
    }

    #[test]
    fn an_external_link_goes_to_the_browser_and_nowhere_else() {
        let scratch = folder();

        for uri in [
            "https://example.com/page",
            "http://example.com/",
            "mailto:someone@example.com",
        ] {
            assert_eq!(
                following(uri, &scratch),
                Follow::External {
                    uri: uri.to_owned()
                },
            );
        }
    }

    #[test]
    fn a_file_beside_the_document_that_is_not_markdown_goes_to_the_desktop() {
        let scratch = folder();

        assert_eq!(
            following("axiomd://doc-3/report.pdf", &scratch),
            Follow::Attachment {
                file: scratch.path().join("report.pdf")
            },
        );
        assert_eq!(
            following("axiomd://doc-3/images/logo.png", &scratch),
            Follow::Attachment {
                file: scratch.path().join("images/logo.png")
            },
        );
    }

    #[test]
    fn a_load_request_is_recognised_as_one() {
        let scratch = folder();
        let request = Request::LoadImage("https://example.com/a.png".to_owned());

        assert_eq!(following(&request.uri(), &scratch), Follow::Ask(request));
        assert_eq!(
            following(&Request::LoadAllImages.uri(), &scratch),
            Follow::Ask(Request::LoadAllImages),
        );
    }

    /// Pressing a task list item's box is a navigation, because a document that cannot
    /// run a script has no other way to say it was pressed. It must reach the app as
    /// the request it is rather than as a link to a file called `task`.
    #[test]
    fn pressing_a_task_box_is_recognised_as_a_request() {
        let scratch = folder();

        assert_eq!(
            following(&Request::ToggleTask(349).uri(), &scratch),
            Follow::Ask(Request::ToggleTask(349)),
        );
    }

    /// The privacy rule, stated as the thing that must not happen: nothing but
    /// staying on the page happens without the reader having clicked.
    #[test]
    fn nothing_leaves_the_document_unless_the_reader_clicked() {
        let scratch = folder();

        for target in [
            "axiomd://doc-3/notes.md",
            "https://example.com/",
            "axiomd://doc-3/report.pdf",
            &Request::LoadAllImages.uri(),
        ] {
            assert_eq!(
                follow(HERE, Some(scratch.path()), target, false),
                Follow::Refuse,
                "{target} was followed without a click",
            );
        }
        // Except staying where it is, which is what a fragment does.
        assert_eq!(
            follow(HERE, Some(scratch.path()), "axiomd://doc-3/#x", false),
            Follow::Stay,
        );
    }

    /// A document's reach is its own folder, and its own host. Everything here is a
    /// way of asking for something outside one or the other.
    #[test]
    fn a_document_cannot_reach_outside_its_own_folder_or_host() {
        let scratch = folder();
        std::fs::write(
            scratch.path().join("../axiomd-links-secret.md"),
            "# Secret\n",
        )
        .ok();
        std::os::unix::fs::symlink("/etc/passwd", scratch.path().join("leak.md")).ok();

        for target in [
            "axiomd://doc-3/../secret.md",
            "axiomd://doc-3/images/../../secret.md",
            "axiomd://doc-3/%2e%2e/secret.md",
            "axiomd://doc-3/leak.md",
            "axiomd://doc-4/other.md",
            "axiomd://assets/axiomd.css",
            "file:///etc/passwd",
            "data:text/html,<h1>hi</h1>",
            "javascript:alert(1)",
            "ftp://example.com/x",
        ] {
            assert_eq!(
                following(target, &scratch),
                Follow::Refuse,
                "{target} was followed",
            );
        }
    }

    /// The very first load has no current URI to compare against and must still be
    /// allowed, or no document would ever appear.
    #[test]
    fn the_documents_own_first_load_is_allowed() {
        let scratch = folder();

        assert_eq!(
            follow("", Some(scratch.path()), "axiomd://doc-0/", false),
            Follow::Stay
        );
        assert_eq!(
            follow("", Some(scratch.path()), "https://example.com/", false),
            Follow::Refuse,
        );
    }
}
