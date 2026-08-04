//! Editable document model.
//!
//! While a window owns a file the text here is the source of truth: rendering,
//! outline, search and export consume this model, never the file on disk. The file is
//! where the text came from and where it goes back to, and nothing else reads it.
//!
//! ```
//! use axiomd_doc::Document;
//!
//! let mut document = Document::untitled();
//! assert!(document.needs_a_name(), "an untitled document has nowhere to be saved");
//! assert!(document.home().is_none(), "and so it is nowhere");
//!
//! document.edited();
//! document.holds("# Notes\n".to_owned());
//! assert!(document.is_modified());
//! ```
//!
//! # Two ways to say the buffer changed
//!
//! [`Document::edited`] costs nothing and says only *that* the reader changed
//! something; [`Document::holds`] says *what* it now is. They are separate because a
//! keystroke must not pay for a copy of a ten-megabyte document: the window marks the
//! document edited on every keystroke and hands over the text only when something
//! actually needs it — a render, a save, a reconciliation. Every method that reads the
//! text says so, so a caller that never calls [`Document::holds`] simply has an older
//! document rather than a corrupted one.
//!
//! # Saving is atomic
//!
//! A save writes a temporary file beside the document, flushes it to the disk, and
//! renames it over the original. A save that fails at any point — no space, no
//! permission, the process killed — leaves the previous version exactly as it was,
//! because the rename is the only thing that touches the reader's file and a rename
//! either happens or does not. Symbolic links are resolved first, so saving through a
//! link updates what it points at rather than replacing the link with a file.
//!
//! # Where a document is
//!
//! One answer, resolved once when a window is given a document, and read by everything
//! that needs it: [`Home`]. A document reached through the desktop's document portal
//! has a path axiomd opens it by and a folder the reader keeps it in, and they are not
//! the same path — see that module for why, and for what the portal was asked.
//!
//! # What a change under the document means
//!
//! [`Document::reconcile`] is the whole of the external-change matrix
//! (`ux_decisions.md`). It compares the file's identity on disk against the one this
//! document last read or wrote, so a document's *own* save — including an automatic
//! one — is recognised as its own and never becomes a reload, a re-render or a
//! question. Everything else is decided by whether the reader has unsaved work:
//! without it the document silently follows the file, with it the reader is offered
//! the choice and nothing is overwritten either way.

#![deny(missing_docs)]

mod home;

use std::fs::File;
use std::io::Write;
use std::path::Path;

use axiomd_i18n::{gettext, gettext_noop};

pub use home::Home;

/// The text a window owns, and everything true about it that is not the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    home: Option<Home>,
    text: String,
    modified: bool,
    /// The identity of the file as this document last read or wrote it. `None` for a
    /// document that has never been on disk.
    stamp: Option<Stamp>,
    /// What the file says now, kept only while it disagrees with a modified buffer —
    /// this is what [`Document::take_theirs`] hands over.
    theirs: Option<String>,
}

/// What a change to the file underneath a document turned out to mean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum External {
    /// The file is what this document already knows it to be — including a save this
    /// document just made. There is nothing for the reader to see.
    Nothing,
    /// A clean buffer took the file's new text. The reader is shown it without being
    /// asked, in the place they were reading (invariant 5).
    Followed,
    /// The file cannot be read any more. The buffer keeps every word it had; the
    /// window says so beside the document rather than taking it away.
    Gone,
    /// A modified buffer met a changed file. Neither is overwritten: the reader
    /// chooses, with [`Document::keep_mine`] or [`Document::take_theirs`].
    Conflict,
}

/// Why a document could not be read or written, in the words the reader is shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trouble {
    title: String,
    detail: String,
}

impl Trouble {
    /// The one-line summary — a window title, a banner, a status page's heading.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The sentence under it.
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

/// A file's identity as the model last saw it: enough to tell "nobody has touched
/// this since we did" from "somebody has".
///
/// The modification time is part of it, so a rewrite that happens to produce the same
/// size is still a change. Our own save is recognised by taking this again from the
/// file the rename produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    device: u64,
    inode: u64,
    size: u64,
    seconds: i64,
    nanoseconds: i64,
}

impl Stamp {
    fn of(file: &Path) -> Option<Stamp> {
        use std::os::unix::fs::MetadataExt;

        let metadata = std::fs::metadata(file).ok()?;
        Some(Stamp {
            device: metadata.dev(),
            inode: metadata.ino(),
            size: metadata.size(),
            seconds: metadata.mtime(),
            nanoseconds: metadata.mtime_nsec(),
        })
    }
}

/// What an untitled document is called, in the window title and in the Save As dialog.
///
/// A name the reader reads and then types over, so it is theirs to read: a German
/// reader's first save offers `Unbenannt.md`, not `Untitled.md`.
const UNTITLED: &str = gettext_noop("Untitled");

impl Document {
    /// A document with nothing in it and nowhere to be saved — a bare launch, or
    /// `Ctrl+N`.
    pub fn untitled() -> Document {
        Document {
            home: None,
            text: String::new(),
            modified: false,
            stamp: None,
            theirs: None,
        }
    }

    /// The document at `home` holds, read now.
    ///
    /// Blocking: meant for a worker, never for the main loop (invariant 4).
    ///
    /// It takes a resolved [`Home`] rather than a path so that where a document is is
    /// settled once, by whoever opened it, instead of once per reload.
    pub fn read(home: &Home) -> Result<Document, Trouble> {
        let file = home.path();
        let name = name_of(file);
        // Taken before the read rather than after it, so a write that lands between
        // the two is a change this document has not seen rather than one it thinks it
        // already has.
        let stamp = Stamp::of(file);
        let bytes = std::fs::read(file).map_err(|error| Trouble {
            title: gettext("Could not open {document}").replace("{document}", &name),
            detail: format!("{error}."),
        })?;
        let text = String::from_utf8(bytes).map_err(|_| Trouble {
            title: gettext("Could not read {document}").replace("{document}", &name),
            detail: gettext("This file is not UTF-8 text, so it is not a Markdown document."),
        })?;
        Ok(Document {
            home: Some(home.clone()),
            text,
            modified: false,
            stamp,
            theirs: None,
        })
    }

    /// Where this document is — the path it is reached by and the place the reader
    /// keeps it — or `None` while it has never been anywhere.
    pub fn home(&self) -> Option<&Home> {
        self.home.as_ref()
    }

    /// What the reader calls this document: its file name, or `Untitled`.
    pub fn name(&self) -> String {
        match &self.home {
            Some(home) => name_of(home.path()),
            None => gettext(UNTITLED),
        }
    }

    /// The text, as this document last heard it (see [`Document::holds`]).
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether the buffer holds work that is not on disk.
    pub fn is_modified(&self) -> bool {
        self.modified
    }

    /// Whether saving has to ask for a name first — the reader's first `Ctrl+S` on a
    /// document that has never been anywhere.
    pub fn needs_a_name(&self) -> bool {
        self.home.is_none()
    }

    /// The reader changed the buffer.
    ///
    /// Constant cost, whatever the document's size: this is what a keystroke pays.
    pub fn edited(&mut self) {
        self.modified = true;
    }

    /// The buffer now says `text`.
    ///
    /// Called before anything that reads the document — a render, a save, a
    /// reconciliation — rather than on every keystroke.
    pub fn holds(&mut self, text: String) {
        self.text = text;
    }

    /// Writes the buffer back to its file, atomically.
    ///
    /// The document must have a name; [`Document::needs_a_name`] is how a caller knows
    /// to ask for one first.
    pub fn save(&mut self) -> Result<(), Trouble> {
        let Some(file) = self.home.as_ref().map(|home| home.path().to_path_buf()) else {
            return Err(Trouble {
                title: gettext("Could not save {document}")
                    .replace("{document}", &gettext(UNTITLED)),
                detail: gettext("This document has never been saved, so it has no file yet."),
            });
        };
        self.write(&file)
    }

    /// The same, to a place the reader has just chosen. The document lives there from
    /// now on — including when the chooser was the desktop's, and what came back is a
    /// portal path rather than the one the reader picked.
    pub fn save_as(&mut self, home: &Home) -> Result<(), Trouble> {
        self.write(home.path())?;
        self.home = Some(home.clone());
        Ok(())
    }

    fn write(&mut self, file: &Path) -> Result<(), Trouble> {
        // A document reached through a symbolic link is the file the link points at.
        // Renaming over the link itself would replace the reader's link with a copy of
        // their document and leave the original untouched.
        let target = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
        write_atomically(&target, &self.text).map_err(|error| Trouble {
            title: gettext("Could not save {document}").replace("{document}", &name_of(file)),
            detail: format!("{error}."),
        })?;
        self.stamp = Stamp::of(&target);
        self.modified = false;
        self.theirs = None;
        Ok(())
    }

    /// Folds a freshly read copy of this document's file into it, and answers what
    /// that meant.
    ///
    /// `file` is the result of [`Document::read`] on the same path, run on a worker:
    /// this call itself touches no disk, so a window can make it from a signal handler.
    pub fn reconcile(&mut self, file: Result<Document, Trouble>) -> External {
        let arrived = match file {
            Ok(arrived) => arrived,
            // Unreadable is not "empty": the reader keeps every word they had.
            Err(_) => {
                self.stamp = None;
                return External::Gone;
            }
        };
        // Nobody has touched the file since this document last read or wrote it — and
        // that includes the automatic save it made a moment ago, which is what keeps
        // autosave from looping back through the reload path.
        if arrived.stamp.is_some() && arrived.stamp == self.stamp {
            return External::Nothing;
        }
        self.stamp = arrived.stamp;
        if arrived.text == self.text {
            // Somebody wrote the file, and it now says exactly what the buffer says.
            // There is nothing to show and nothing left unsaved.
            self.modified = false;
            self.theirs = None;
            return External::Nothing;
        }
        if self.modified {
            self.theirs = Some(arrived.text);
            return External::Conflict;
        }
        self.text = arrived.text;
        self.theirs = None;
        External::Followed
    }

    /// Whether the reader has a choice to make about this document right now.
    pub fn is_conflicted(&self) -> bool {
        self.theirs.is_some()
    }

    /// The reader keeps their version. The file's is forgotten; the buffer stays
    /// unsaved until they save it.
    pub fn keep_mine(&mut self) {
        self.theirs = None;
    }

    /// The reader takes the file's version, losing their unsaved work by asking for it.
    pub fn take_theirs(&mut self) {
        if let Some(theirs) = self.theirs.take() {
            self.text = theirs;
            self.modified = false;
        }
    }
}

/// The document's file name as the reader thinks of it.
fn name_of(file: &Path) -> String {
    file.file_name()
        .unwrap_or(file.as_os_str())
        .to_string_lossy()
        .into_owned()
}

/// Writes `text` to `file` so that `file` is either the old document or the new one,
/// never half of either.
///
/// The temporary file is in the document's own directory, because a rename is only
/// atomic within one filesystem — a temporary directory elsewhere would silently
/// become a copy-then-delete, which is exactly the non-atomic write this avoids.
fn write_atomically(file: &Path, text: &str) -> std::io::Result<()> {
    let directory = file.parent().unwrap_or(Path::new("."));
    let temporary = directory.join(format!(".{}.axiomd-{}", name_of(file), std::process::id()));

    let written = (|| -> std::io::Result<()> {
        let mut handle = File::create(&temporary)?;
        handle.write_all(text.as_bytes())?;
        // Before the rename, or a crash could leave the reader with a file that exists,
        // has the new name, and holds nothing.
        handle.sync_all()?;
        drop(handle);
        // A new file is created with the process umask; the reader's document keeps
        // whatever they set on it.
        if let Ok(existing) = std::fs::metadata(file) {
            std::fs::set_permissions(&temporary, existing.permissions())?;
        }
        std::fs::rename(&temporary, file)
    })();

    if written.is_err() {
        // Nothing half-written is left lying beside the reader's document.
        let _ = std::fs::remove_file(&temporary);
        return written;
    }
    // The rename itself is durable only once the directory entry is on the disk.
    if let Ok(handle) = File::open(directory) {
        let _ = handle.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// A directory that exists for one test and goes away with it.
    struct Scratch {
        path: PathBuf,
    }

    impl Scratch {
        fn new(label: &str) -> Scratch {
            use std::sync::atomic::{AtomicU64, Ordering};
            static COUNTER: AtomicU64 = AtomicU64::new(0);

            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "axiomd-doc-{label}-{}-{unique}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).expect("create scratch directory");
            Scratch { path }
        }

        fn write(&self, name: &str, contents: &str) -> PathBuf {
            let path = self.path.join(name);
            std::fs::write(&path, contents).expect("write scratch file");
            path
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o755));
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    use std::os::unix::fs::PermissionsExt;

    fn read(file: &Path) -> String {
        std::fs::read_to_string(file).expect("read the file back")
    }

    /// Where a scratch file is. An ordinary path is already the answer, so nothing here
    /// asks the desktop anything — the portal's own half is tested in `home.rs`.
    fn at(file: &Path) -> Home {
        Home::of(file)
    }

    /// Everything left beside the document after a save. A temporary file that
    /// survives is a file the reader has to explain to themselves.
    fn leftovers(directory: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(directory)
            .expect("list the directory")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_document_read_from_a_file_holds_what_the_file_holds() {
        let scratch = Scratch::new("read");
        let file = scratch.write("notes.md", "# Notes\n\nBody.\n");

        let document = Document::read(&at(&file)).expect("read the document");

        assert_eq!(document.text(), "# Notes\n\nBody.\n");
        assert_eq!(document.name(), "notes.md");
        assert_eq!(document.home().map(Home::path), Some(file.as_path()));
        assert!(!document.is_modified());
        assert!(!document.needs_a_name());
    }

    #[test]
    fn a_file_that_is_not_text_is_refused_in_words_the_reader_can_read() {
        let scratch = Scratch::new("read-binary");
        let file = scratch.path().join("image.md");
        std::fs::write(&file, [0xffu8, 0xfe, 0x00, 0x9f]).expect("write bytes");

        let trouble = Document::read(&at(&file)).expect_err("bytes are not a document");

        assert!(trouble.title().contains("image.md"), "{}", trouble.title());
        assert!(trouble.detail().contains("UTF-8"), "{}", trouble.detail());
    }

    #[test]
    fn a_file_that_is_not_there_is_refused_by_name() {
        let scratch = Scratch::new("read-missing");
        let trouble = Document::read(&at(&scratch.path().join("gone.md")))
            .expect_err("there is no such file");

        assert!(trouble.title().contains("gone.md"), "{}", trouble.title());
        assert!(!trouble.detail().is_empty(), "the reason was left blank");
    }

    /// An untitled document is the bare launch and `Ctrl+N`: empty, clean, and with
    /// nowhere to be saved until the reader says where.
    #[test]
    fn an_untitled_document_has_nowhere_to_be_saved_until_it_is_given_a_name() {
        let scratch = Scratch::new("untitled");
        let mut document = Document::untitled();

        assert_eq!(document.name(), "Untitled");
        assert!(document.needs_a_name());
        assert!(document.save().is_err(), "an untitled document was saved");

        document.edited();
        document.holds("# First\n".to_owned());
        let file = scratch.path().join("first.md");
        document.save_as(&at(&file)).expect("save the document");

        assert_eq!(read(&file), "# First\n");
        assert_eq!(document.name(), "first.md");
        assert!(!document.needs_a_name());
        assert!(!document.is_modified());
    }

    #[test]
    fn a_document_is_modified_from_the_first_keystroke_until_it_is_saved() {
        let scratch = Scratch::new("dirty");
        let file = scratch.write("notes.md", "one\n");
        let mut document = Document::read(&at(&file)).expect("read the document");
        assert!(!document.is_modified());

        document.edited();
        assert!(
            document.is_modified(),
            "a keystroke left the document clean"
        );

        document.holds("one\ntwo\n".to_owned());
        document.save().expect("save the document");

        assert!(
            !document.is_modified(),
            "a saved document is still modified"
        );
        assert_eq!(read(&file), "one\ntwo\n");
    }

    /// The reader's file is replaced by a rename, so it is either the old document or
    /// the new one at every instant — and nothing half-written is left beside it.
    #[test]
    fn a_save_leaves_the_document_whole_and_nothing_else_behind() {
        let scratch = Scratch::new("atomic");
        let file = scratch.write("notes.md", "old\n");
        let mut document = Document::read(&at(&file)).expect("read the document");

        document.holds("new\n".to_owned());
        document.save().expect("save the document");

        assert_eq!(read(&file), "new\n");
        assert_eq!(
            leftovers(scratch.path()),
            ["notes.md"],
            "a save left a temporary file beside the reader's document",
        );
    }

    /// The crash-mid-write case, made to happen: the save cannot complete, and the
    /// reader still has every word of the version they had.
    #[test]
    fn a_save_that_cannot_complete_leaves_the_previous_version_intact() {
        let scratch = Scratch::new("atomic-fail");
        let file = scratch.write("notes.md", "the version on disk\n");
        let mut document = Document::read(&at(&file)).expect("read the document");
        document.edited();
        document.holds("the version that will not land\n".to_owned());

        // Nothing new can be created in the directory, so the temporary file — and
        // therefore the save — fails before anything touches the reader's document.
        std::fs::set_permissions(scratch.path(), std::fs::Permissions::from_mode(0o500))
            .expect("make the directory read-only");
        let trouble = document
            .save()
            .expect_err("the save could not have succeeded");
        std::fs::set_permissions(scratch.path(), std::fs::Permissions::from_mode(0o755))
            .expect("make the directory writable again");

        assert!(trouble.title().contains("notes.md"), "{}", trouble.title());
        assert_eq!(
            read(&file),
            "the version on disk\n",
            "a failed save damaged the reader's document",
        );
        assert!(
            document.is_modified(),
            "a document that was not written was marked saved",
        );
        assert_eq!(
            leftovers(scratch.path()),
            ["notes.md"],
            "a failed save left a temporary file behind",
        );
    }

    /// A document reached through a symbolic link is the file the link points at.
    /// Renaming over the link would give the reader a copy and leave the original.
    #[test]
    fn saving_through_a_symbolic_link_writes_what_the_link_points_at() {
        let scratch = Scratch::new("symlink");
        let target = scratch.write("real.md", "old\n");
        let link = scratch.path().join("link.md");
        std::os::unix::fs::symlink(&target, &link).expect("create a symlink");

        let mut document = Document::read(&at(&link)).expect("read through the link");
        document.holds("new\n".to_owned());
        document.save().expect("save the document");

        assert_eq!(read(&target), "new\n");
        assert!(
            std::fs::symlink_metadata(&link)
                .expect("the link is still there")
                .file_type()
                .is_symlink(),
            "saving replaced the reader's symbolic link with a copy of the document",
        );
    }

    /// A document the reader made executable, or private, stays that way: a save is a
    /// new version of their file, not a new file.
    #[test]
    fn a_save_keeps_the_permissions_the_reader_gave_the_file() {
        let scratch = Scratch::new("permissions");
        let file = scratch.write("notes.md", "old\n");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600))
            .expect("make the document private");

        let mut document = Document::read(&at(&file)).expect("read the document");
        document.holds("new\n".to_owned());
        document.save().expect("save the document");

        let mode = std::fs::metadata(&file)
            .expect("the document is still there")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "the saved document is readable by everyone");
    }

    /// The document's own save comes back through the file monitor a moment later.
    /// Recognising it is what keeps autosave from re-reading, re-rendering and
    /// re-reporting every write it makes.
    #[test]
    fn a_documents_own_save_is_not_a_change_under_it() {
        let scratch = Scratch::new("self-write");
        let file = scratch.write("notes.md", "one\n");
        let mut document = Document::read(&at(&file)).expect("read the document");

        document.edited();
        document.holds("one\ntwo\n".to_owned());
        document.save().expect("save the document");

        assert_eq!(
            document.reconcile(Document::read(&at(&file))),
            External::Nothing
        );
        assert_eq!(document.text(), "one\ntwo\n");
        assert!(!document.is_modified());

        // And the edit that follows it, still unsaved when the monitor reports the
        // earlier save, is not mistaken for somebody else's work.
        document.edited();
        document.holds("one\ntwo\nthree\n".to_owned());
        assert_eq!(
            document.reconcile(Document::read(&at(&file))),
            External::Nothing
        );
        assert!(
            document.is_modified(),
            "the unsaved edit was thrown away by the document's own earlier save",
        );
        assert_eq!(document.text(), "one\ntwo\nthree\n");
    }

    /// The clean half of the external-change matrix: the document follows the file
    /// without asking anybody anything (`ux_decisions.md`).
    #[test]
    fn a_clean_document_follows_the_file_silently() {
        let scratch = Scratch::new("follow");
        let file = scratch.write("notes.md", "one\n");
        let mut document = Document::read(&at(&file)).expect("read the document");

        std::fs::write(&file, "two\n").expect("save over the document");

        assert_eq!(
            document.reconcile(Document::read(&at(&file))),
            External::Followed
        );
        assert_eq!(document.text(), "two\n");
        assert!(!document.is_modified());
        assert!(!document.is_conflicted());
    }

    /// The same, for the way most editors save: a new file renamed over the old one.
    /// The file the document was read from no longer exists.
    #[test]
    fn a_clean_document_follows_a_file_that_was_replaced_by_a_rename() {
        let scratch = Scratch::new("follow-rename");
        let file = scratch.write("notes.md", "one\n");
        let mut document = Document::read(&at(&file)).expect("read the document");

        let replacement = scratch.write("notes.md.new", "two\n");
        std::fs::rename(&replacement, &file).expect("rename over the document");

        assert_eq!(
            document.reconcile(Document::read(&at(&file))),
            External::Followed
        );
        assert_eq!(document.text(), "two\n");
    }

    /// The dirty half: nothing is overwritten in either direction, and the reader is
    /// the one who decides.
    #[test]
    fn a_modified_document_meeting_a_changed_file_asks_the_reader_and_loses_nothing() {
        let scratch = Scratch::new("conflict");
        let file = scratch.write("notes.md", "one\n");
        let mut document = Document::read(&at(&file)).expect("read the document");

        document.edited();
        document.holds("mine\n".to_owned());
        std::fs::write(&file, "theirs\n").expect("save over the document");

        assert_eq!(
            document.reconcile(Document::read(&at(&file))),
            External::Conflict
        );
        assert_eq!(
            document.text(),
            "mine\n",
            "the reader's work was thrown away"
        );
        assert!(document.is_modified());
        assert!(document.is_conflicted());

        document.keep_mine();
        assert_eq!(document.text(), "mine\n");
        assert!(document.is_modified(), "keeping my version saved it for me");
        assert!(!document.is_conflicted());
        assert_eq!(
            read(&file),
            "theirs\n",
            "keeping my version overwrote the file without being asked",
        );
    }

    #[test]
    fn a_reader_who_takes_the_file_version_gets_it_and_a_clean_document() {
        let scratch = Scratch::new("conflict-theirs");
        let file = scratch.write("notes.md", "one\n");
        let mut document = Document::read(&at(&file)).expect("read the document");

        document.edited();
        document.holds("mine\n".to_owned());
        std::fs::write(&file, "theirs\n").expect("save over the document");
        assert_eq!(
            document.reconcile(Document::read(&at(&file))),
            External::Conflict
        );

        document.take_theirs();

        assert_eq!(document.text(), "theirs\n");
        assert!(!document.is_modified());
        assert!(!document.is_conflicted());
        // And the document is on that version now: the same file is no longer a change.
        assert_eq!(
            document.reconcile(Document::read(&at(&file))),
            External::Nothing
        );
    }

    /// A modified buffer that the file catches up with is not a conflict: there is
    /// nothing to choose between.
    #[test]
    fn a_file_that_comes_to_agree_with_the_buffer_settles_the_document() {
        let scratch = Scratch::new("agree");
        let file = scratch.write("notes.md", "one\n");
        let mut document = Document::read(&at(&file)).expect("read the document");

        document.edited();
        document.holds("agreed\n".to_owned());
        std::fs::write(&file, "agreed\n").expect("save the same text over the document");

        assert_eq!(
            document.reconcile(Document::read(&at(&file))),
            External::Nothing
        );
        assert!(!document.is_modified());
        assert!(!document.is_conflicted());
    }

    /// A file that goes away does not take the reader's document with it.
    #[test]
    fn a_deleted_file_leaves_the_reader_every_word_they_had() {
        let scratch = Scratch::new("deleted");
        let file = scratch.write("notes.md", "one\n");
        let mut document = Document::read(&at(&file)).expect("read the document");

        std::fs::remove_file(&file).expect("delete the document");

        assert_eq!(
            document.reconcile(Document::read(&at(&file))),
            External::Gone
        );
        assert_eq!(document.text(), "one\n");
        assert_eq!(document.home().map(Home::path), Some(file.as_path()));

        // And when it comes back, the document follows it again.
        std::fs::write(&file, "two\n").expect("bring the document back");
        assert_eq!(
            document.reconcile(Document::read(&at(&file))),
            External::Followed
        );
        assert_eq!(document.text(), "two\n");
    }

    /// The other way a file goes away: renamed out from under the document. The path
    /// is what the reader means, so this is a deletion like any other.
    #[test]
    fn a_file_renamed_away_leaves_the_reader_every_word_they_had() {
        let scratch = Scratch::new("renamed-away");
        let file = scratch.write("notes.md", "one\n");
        let mut document = Document::read(&at(&file)).expect("read the document");

        std::fs::rename(&file, scratch.path().join("elsewhere.md")).expect("rename it away");

        assert_eq!(
            document.reconcile(Document::read(&at(&file))),
            External::Gone
        );
        assert_eq!(document.text(), "one\n");
    }

    /// A deletion under a modified buffer is still not a question: there is no file
    /// version to choose, so the reader keeps theirs and saving puts it back.
    #[test]
    fn a_modified_document_whose_file_is_deleted_can_be_saved_back() {
        let scratch = Scratch::new("deleted-dirty");
        let file = scratch.write("notes.md", "one\n");
        let mut document = Document::read(&at(&file)).expect("read the document");

        document.edited();
        document.holds("mine\n".to_owned());
        std::fs::remove_file(&file).expect("delete the document");

        assert_eq!(
            document.reconcile(Document::read(&at(&file))),
            External::Gone
        );
        assert!(!document.is_conflicted());

        document.save().expect("save the document back");
        assert_eq!(read(&file), "mine\n");
        assert!(!document.is_modified());
    }

    /// Save As is what the reader's first `Ctrl+S` runs, and what "save a copy"
    /// amounts to: from then on the document is the new file's and follows it.
    #[test]
    fn save_as_moves_the_document_to_the_file_the_reader_chose() {
        let scratch = Scratch::new("save-as");
        let original = scratch.write("notes.md", "one\n");
        let mut document = Document::read(&at(&original)).expect("read the document");

        document.edited();
        document.holds("two\n".to_owned());
        let chosen = scratch.path().join("copy.md");
        document.save_as(&at(&chosen)).expect("save the document");

        assert_eq!(read(&chosen), "two\n");
        assert_eq!(
            read(&original),
            "one\n",
            "Save As rewrote the original file"
        );
        assert_eq!(document.home().map(Home::path), Some(chosen.as_path()));
        assert!(!document.is_modified());
        // The document follows its new file, and no longer the one it came from.
        assert_eq!(
            document.reconcile(Document::read(&at(&chosen))),
            External::Nothing
        );
    }
}
