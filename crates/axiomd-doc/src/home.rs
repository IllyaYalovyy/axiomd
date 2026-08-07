//! Where a document is — the one answer everything that opens, saves, watches,
//! resolves against or *says* where it lives reads.
//!
//! Two paths can name the same document, and on a packaged desktop they usually do. A
//! document handed to a sandboxed application by the desktop's document portal arrives
//! as `/run/user/<uid>/doc/<document id>/<its own name>`: a filesystem of its own
//! holding that one file. It is the only path the application can open, and it is a
//! path the reader has never seen and would not recognise — what they know is
//! `~/SynologyDrive/AiBlog/article.md` (issue #24).
//!
//! So a document has a path axiomd *reaches* it by and a folder the reader *keeps* it
//! in, and this is the one place that knows both. It is resolved once, when a window is
//! given a document; every consumer reads the answer rather than asking the question
//! again, which is what keeps the window's subtitle, the Save As chooser, the watch and
//! the folder a picture beside the document resolves against from disagreeing.
//!
//! # How the reader's own path is found
//!
//! `org.freedesktop.portal.Documents.GetHostPaths` — the document portal's own answer
//! to "what is this document really", asked over the session bus.
//!
//! Probed on 2026-08-04, xdg-desktop-portal on this GNOME 49 machine, `Documents`
//! interface version 5:
//!
//! * `GetHostPaths(as doc_ids) → a{say} paths`; each value is the host path **with a
//!   trailing NUL byte**, and an id the portal does not know is simply absent from the
//!   answer rather than an error.
//! * from *inside* a flatpak sandbox the same call answers with the reader's own path
//!   (`{'396616cf': b'/tmp/…/notes.md'}`), while `Info` — the older call that also
//!   carries the path — answers `org.freedesktop.portal.Error.NotAllowed: Not allowed
//!   in sandbox`. `GetHostPaths` is therefore not merely the preferred mechanism, it is
//!   the only one a packaged axiomd has.
//! * `GetMountPoint()` answers `/run/user/1000/doc` both on the host and inside the
//!   sandbox, where `XDG_RUNTIME_DIR` is `/run/user/1000`. The mount is derived from
//!   `XDG_RUNTIME_DIR` here rather than asked for, because every ordinary document
//!   would otherwise pay a round trip to learn that it is an ordinary document.
//! * the round trip itself measured 0.3–2.1 ms over five calls.
//!
//! Nothing here is a guess about an extended attribute or a `/proc` entry: the portal
//! is asked, and when it will not answer the document is named rather than placed.

use std::path::{Component, Path, PathBuf};

use gio::glib;

/// How long the desktop is given to say where a document really is, in milliseconds.
///
/// Two hundred times the 2.1 ms the call was measured at at its slowest, so a busy
/// portal still answers — and bounded, because the answer is only a *subtitle*. A
/// desktop that has stopped answering must cost a window the folder under its title,
/// never the moment the reader gets their document (invariant 4).
const PATIENCE: i32 = 500;

/// Where a document is: the path axiomd reaches it by, and the folder the reader keeps
/// it in.
///
/// The two are the same path for a document opened from the reader's own filesystem,
/// and differ for one the desktop's document portal handed over. Everything that has to
/// *reach* the document — reading it, saving it, watching it — uses [`Home::path`];
/// everything the reader sees or navigates from uses [`Home::folder`], [`Home::shown`]
/// and [`Home::full`], which can never spell a portal path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Home {
    /// What axiomd opens. Under the portal this is the fuse path and nothing else will
    /// do: the folder the reader keeps the document in is not reachable from inside the
    /// sandbox.
    reach: PathBuf,
    /// Where the reader keeps it, or `None` when the desktop would not say — the one
    /// case where axiomd knows the document's name and nothing about its place.
    kept: Option<PathBuf>,
}

impl Home {
    /// Where the document at `file` is, asking the desktop when `file` is a path only
    /// the desktop can explain.
    ///
    /// Blocking, and bounded: an ordinary path costs no system call at all, and a
    /// portal one costs a single session-bus round trip with [`PATIENCE`] to answer in.
    pub fn of(file: &Path) -> Home {
        located(file, portal_mount().as_deref(), &ask_the_desktop)
    }

    /// The path axiomd reaches this document by — what it reads, writes and watches.
    pub fn path(&self) -> &Path {
        &self.reach
    }

    /// The folder the reader keeps this document in: where Save As starts, and what a
    /// picture or a `[[wikilink]]` beside the document resolves against.
    ///
    /// `None` when the desktop would not say where the document really is. Nothing is
    /// offered in that case rather than the portal's own folder, which holds this one
    /// document and nothing else and is not a place the reader has ever been.
    pub fn folder(&self) -> Option<&Path> {
        Some(self.kept()?.parent().unwrap_or(Path::new("")))
    }

    /// What a window puts under the document's name: the folder it is in, with the
    /// reader's home shortened to `~`.
    ///
    /// The document's own name alone when the desktop would not say where it is — never
    /// nothing, and never a path the reader has not seen.
    pub fn shown(&self) -> String {
        match self.folder() {
            Some(folder) => with_home_shortened(folder),
            None => self.name(),
        }
    }

    /// The same, in full and unshortened — what the reader gets on hovering the
    /// subtitle, which is where a path too long for a header bar goes.
    pub fn full(&self) -> String {
        match self.kept() {
            Some(kept) => kept.display().to_string(),
            None => self.name(),
        }
    }

    /// The path the reader keeps this document at — the one name anything that writes
    /// something down *about* a document has to write it down under (issue #51).
    ///
    /// [`Home::path`] will not do for that: under the document portal it is a fuse path
    /// carrying a document id the desktop mints afresh, so the same document is a
    /// different path the next time it is opened and what was remembered about it
    /// would never be found again. This is the path that does not move.
    ///
    /// `None` when the desktop would not say where the document really is, which is the
    /// one case where axiomd knows a document's name and nothing about its place: a
    /// name alone is not an identity — two folders can each hold a `notes.md` — so
    /// there is nothing to write anything down under.
    pub fn kept(&self) -> Option<&Path> {
        self.kept.as_deref()
    }

    /// The document's own name. The portal keeps it (probed: forwarding
    /// `article.medium.md` arrives as `/run/user/1000/doc/<id>/article.medium.md`), so
    /// this is the same either way round.
    fn name(&self) -> String {
        self.reach
            .file_name()
            .unwrap_or(self.reach.as_os_str())
            .to_string_lossy()
            .into_owned()
    }
}

/// The whole of the policy, with the desktop's answer handed in — which is what lets a
/// test state the portal's answer instead of arranging one.
fn located(file: &Path, mount: Option<&Path>, ask: &dyn Fn(&str) -> Option<PathBuf>) -> Home {
    let kept = match mount.and_then(|mount| document_id(file, mount)) {
        // Not a portal path at all: it is already the path the reader knows.
        None => Some(file.to_path_buf()),
        Some(id) => ask(id),
    };
    Home {
        reach: file.to_path_buf(),
        kept,
    }
}

/// The portal's id for `file`, or `None` when `file` is an ordinary path.
///
/// The shape under the mount is `<document id>/<the file's own name>` and nothing else.
/// A deeper path is not something the portal handed over, and treating it as one would
/// send a made-up id to the desktop.
fn document_id<'a>(file: &'a Path, mount: &Path) -> Option<&'a str> {
    let mut under = file.strip_prefix(mount).ok()?.components();
    let id = match under.next()? {
        Component::Normal(id) => id.to_str()?,
        _ => return None,
    };
    under.next()?;
    match under.next() {
        None => Some(id),
        Some(_) => None,
    }
}

/// Where the document portal is mounted for this session: `$XDG_RUNTIME_DIR/doc`,
/// which is what `GetMountPoint` answers on both sides of a sandbox (probed above).
fn portal_mount() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR")?).join("doc"))
}

/// Asks the desktop where the document the portal calls `id` really is.
///
/// Every way this can fail — no session bus, no portal, a revoked document, a portal
/// too old to answer — is the same answer: the desktop will not say. A document is
/// still read, still rendered and still saved without it; only its place is unknown.
fn ask_the_desktop(id: &str) -> Option<PathBuf> {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let bus = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).ok()?;
    let asked = glib::Variant::tuple_from_iter([glib::Variant::array_from_iter_with_type(
        glib::VariantTy::STRING,
        [glib::Variant::from(id)],
    )]);
    let answer = bus
        .call_sync(
            Some("org.freedesktop.portal.Documents"),
            "/org/freedesktop/portal/documents",
            "org.freedesktop.portal.Documents",
            "GetHostPaths",
            Some(&asked),
            None,
            gio::DBusCallFlags::NONE,
            PATIENCE,
            gio::Cancellable::NONE,
        )
        .ok()?;

    // `(a{say})`: one entry when the portal knows the id, and none when it does not.
    let found = answer.try_child_value(0)?.try_child_value(0)?;
    let named: String = found.try_child_value(0)?.get()?;
    if named != id {
        return None;
    }
    let mut path: Vec<u8> = found.try_child_value(1)?.get()?;
    // The portal's paths are NUL-terminated (probed); a path with the NUL left on it
    // names nothing at all.
    if path.last() == Some(&0) {
        path.pop();
    }
    match path.is_empty() {
        true => None,
        false => Some(PathBuf::from(OsString::from_vec(path))),
    }
}

/// `folder` as the reader thinks of it, with their home shortened.
fn with_home_shortened(folder: &Path) -> String {
    let folder = folder.display().to_string();
    match glib::home_dir().to_str() {
        Some(home) if folder == home => "~".to_owned(),
        Some(home) => match folder.strip_prefix(&format!("{home}/")) {
            Some(rest) => format!("~/{rest}"),
            None => folder,
        },
        None => folder,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// The mount the probe found, spelled here rather than read from the environment:
    /// a test that set `XDG_RUNTIME_DIR` would be setting it for every other test in
    /// the binary at the same time.
    const MOUNT: &str = "/run/user/1000/doc";

    /// The answer the live portal gave, in the shape it gave it (see this module's
    /// header): the reader's own path, under their home.
    fn as_the_portal_answered(id: &str) -> Option<PathBuf> {
        match id {
            "d8ded700" => Some(PathBuf::from(format!(
                "{}/SynologyDrive/AiBlog/article.md",
                glib::home_dir().display()
            ))),
            _ => None,
        }
    }

    fn resolved(file: &str, ask: &dyn Fn(&str) -> Option<PathBuf>) -> Home {
        located(Path::new(file), Some(Path::new(MOUNT)), ask)
    }

    /// The defect issue #24 reports: the window said `/run/user/1000/doc/d8ded700`
    /// about a document the reader keeps in `~/SynologyDrive/AiBlog`.
    #[test]
    fn a_portal_document_is_placed_where_the_reader_keeps_it() {
        let home = resolved(
            "/run/user/1000/doc/d8ded700/article.md",
            &as_the_portal_answered,
        );

        assert_eq!(
            home.shown(),
            "~/SynologyDrive/AiBlog",
            "the subtitle is not where the reader keeps the document",
        );
        assert_eq!(
            home.full(),
            format!(
                "{}/SynologyDrive/AiBlog/article.md",
                glib::home_dir().display()
            ),
            "the tooltip is not the document's own path in full",
        );
        assert_eq!(
            home.folder(),
            Some(
                PathBuf::from(format!(
                    "{}/SynologyDrive/AiBlog",
                    glib::home_dir().display()
                ))
                .as_path()
            ),
            "a picture beside the document would resolve against the portal's folder",
        );
        assert_eq!(
            home.path(),
            Path::new("/run/user/1000/doc/d8ded700/article.md"),
            "the path axiomd has to open the document by was lost",
        );
    }

    /// The desktop not answering costs the document its place and nothing else — and
    /// above all it must not cost the reader a path they have never seen.
    #[test]
    fn a_document_the_desktop_will_not_place_is_named_and_never_placed() {
        let home = resolved("/run/user/1000/doc/6a6290b7/notes.md", &|_| None);

        assert_eq!(home.shown(), "notes.md");
        assert_eq!(home.full(), "notes.md");
        assert_eq!(
            home.folder(),
            None,
            "Save As and Ctrl+O would open on the portal's own folder",
        );
        assert_eq!(
            home.path(),
            Path::new("/run/user/1000/doc/6a6290b7/notes.md"),
            "an unplaced document must still be readable",
        );
    }

    /// Whatever happens, no part of a portal path reaches the reader.
    #[test]
    fn a_portal_path_is_never_what_a_window_says() {
        for placed in [true, false] {
            let ask: &dyn Fn(&str) -> Option<PathBuf> = match placed {
                true => &as_the_portal_answered,
                false => &|_| None,
            };
            let home = resolved("/run/user/1000/doc/d8ded700/article.md", ask);
            for said in [home.shown(), home.full()] {
                assert!(
                    !said.contains("/run/user") && !said.contains("d8ded700"),
                    "a window would show {said}",
                );
            }
        }
    }

    /// An ordinary document is already where the reader keeps it, and the desktop is
    /// not asked about it — a question per document opened would be a question per
    /// keystroke away from the reader's own files.
    #[test]
    fn an_ordinary_document_is_placed_without_asking_the_desktop() {
        let asked = Cell::new(0);
        let file = format!("{}/Documents/notes.md", glib::home_dir().display());
        let home = resolved(&file, &|_| {
            asked.set(asked.get() + 1);
            None
        });

        assert_eq!(
            asked.get(),
            0,
            "the desktop was asked about an ordinary path"
        );
        assert_eq!(home.shown(), "~/Documents");
        assert_eq!(home.full(), file);
        assert_eq!(
            home.folder(),
            Some(PathBuf::from(format!("{}/Documents", glib::home_dir().display())).as_path()),
        );
    }

    /// The home directory itself, and a document outside it: the two ends of the
    /// shortening the reader reads in the header bar.
    #[test]
    fn the_readers_home_is_shortened_and_nothing_else_is() {
        let inside = resolved(&format!("{}/notes.md", glib::home_dir().display()), &|_| {
            None
        });
        assert_eq!(inside.shown(), "~");

        let outside = resolved("/srv/shared/notes.md", &|_| None);
        assert_eq!(outside.shown(), "/srv/shared");

        // A name with no directory in it at all places the document nowhere rather
        // than at the root.
        let bare = resolved("notes.md", &|_| None);
        assert_eq!(bare.shown(), "");
    }

    /// Only the portal's own shape is a portal path. Anything else under the mount is
    /// an ordinary file as far as axiomd is concerned, and sending the desktop a
    /// made-up id would be asking a question about a document nobody exported.
    #[test]
    fn only_the_portals_own_shape_is_a_document_the_desktop_is_asked_about() {
        let mount = Path::new(MOUNT);
        assert_eq!(
            document_id(Path::new("/run/user/1000/doc/6a6290b7/notes.md"), mount),
            Some("6a6290b7"),
        );
        for ordinary in [
            "/run/user/1000/doc/6a6290b7",
            "/run/user/1000/doc/6a6290b7/nested/notes.md",
            "/run/user/1000/doc",
            "/run/user/1000/notes.md",
            "/home/reader/doc/6a6290b7/notes.md",
        ] {
            assert_eq!(
                document_id(Path::new(ordinary), mount),
                None,
                "{ordinary} was taken for a document the portal handed over",
            );
        }
    }
}
