//! Where the reader left off in each document they have read, and the one small file
//! that outlives the window that learned it (issue #51).
//!
//! A reader who closes a document half way through and opens it again tomorrow is put
//! back where they were. That is the whole of what this module is for, and everything
//! about how it is done is here: where the file is, what is in it, how it is written,
//! and what stops it growing for ever. Nothing outside asks any of those questions —
//! a window says "where did they leave off in this document" and "they are here now",
//! and this answers.
//!
//! # What is written down, and why it is not a scroll offset
//!
//! A source line: the line of the block that stood at the top of the page. That is the
//! same currency the mode switch and the live reload already trade in (invariant 5,
//! `place.js`) — a place in the source rather than a place on a page — so it survives
//! the document being edited between visits, the window being a different size, the
//! reader having changed the measure, and the document being read with another engine.
//! A pixel offset would survive none of them. Restoring it is not new code either: the
//! line is handed to the very call that puts the reader back after a mode switch.
//!
//! What is deliberately *not* written down is where inside that block they were. The
//! block is the granularity everything else in the application places the reader at,
//! and the only way to be finer would be a pixel offset into the block — the one thing
//! the design of this feature rules out.
//!
//! # Where the file is
//!
//! `g_get_user_state_dir()/axiomd/reading-positions` — `~/.local/state/axiomd/` on an
//! ordinary desktop, and the sandbox's own state directory under flatpak, which is
//! right: a package's memory of the reader's documents belongs to the package.
//!
//! One line per document, `<last seen, in seconds since the epoch> <line> <path>`,
//! most recently seen first. Bytes rather than text: a path on Linux is bytes, and a
//! document whose name is not UTF-8 is still a document. A line that does not parse is
//! skipped rather than mourned — a store somebody has damaged costs the reader the
//! places in it, never a word on screen (VISION principle 6).
//!
//! Written by writing a new file beside it and renaming: a reader whose machine loses
//! power mid-write comes back to the store as it was, never to half of one.
//!
//! # What stops it growing
//!
//! Every write tidies the whole store ([`tidied`]), so it is bounded by construction
//! rather than by a cleanup nobody remembers to run:
//!
//! * documents the reader has not opened for [`FORGOTTEN_AFTER`] are dropped;
//! * documents whose file the filesystem says is not there any more are dropped;
//! * what is left is capped at [`REMEMBERED`], oldest first.
//!
//! The prune is what a write can afford to be sure of and no more: a path is dropped
//! only when the filesystem says *that path does not exist*. One that cannot be looked
//! at — a permission, an unmounted drive, a sandbox that was never shown that folder —
//! is kept, because "I could not look" is not "it is gone".

use std::io::ErrorKind;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use axiomd_doc::Home;

use crate::settings::Settings;

/// How many documents the reader's places are kept for.
///
/// Five hundred: comfortably more documents than anyone reads between one release and
/// the next, and a file of about thirty kilobytes at its very largest — small enough
/// that reading all of it to answer one question costs nothing worth measuring.
const REMEMBERED: usize = 500;

/// How long a document goes unopened before the place in it is forgotten, in seconds.
///
/// A hundred and eighty days. Long enough that a document picked up once a term is
/// still where the reader left it; short enough that a store is a memory of what
/// somebody is reading rather than of everything they have ever read.
const FORGOTTEN_AFTER: u64 = 180 * 24 * 60 * 60;

/// The file itself, under the state directory.
const STORE: &str = "axiomd/reading-positions";

/// Where the reader left off, as a window asks it.
///
/// One per window, holding nothing but a path and the reader's settings: there is no
/// shared state between windows to disagree about, and two windows on one document
/// simply write in the order they close — the last one to close is the place the
/// reader comes back to (invariant 7).
pub(crate) struct Places {
    file: PathBuf,
    settings: Rc<Settings>,
}

/// One document, and where the reader last was in it.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Place {
    /// The path the reader keeps the document at ([`Home::kept`]) — never the portal
    /// path axiomd reached it by, which is minted afresh for every open.
    document: PathBuf,
    /// The source line of the block that was at the top of the page.
    line: u32,
    /// When the reader was last there, in seconds since the epoch.
    seen: u64,
}

impl Places {
    /// The reader's places, wherever this system keeps such things.
    pub(crate) fn new(settings: &Rc<Settings>) -> Rc<Places> {
        Rc::new(Places {
            file: glib::user_state_dir().join(STORE),
            settings: settings.clone(),
        })
    }

    /// The source line to put back at the top of the page when `home` is opened, or
    /// `None` when there is nothing to put back — the reader has never read this
    /// document, has asked axiomd not to remember, or has a store that is not there
    /// or cannot be read.
    ///
    /// Nothing is pruned here: opening a document is the one path that must not go
    /// looking at the filesystem for documents nobody asked about (invariant 12's
    /// spirit, and the open path's budget).
    pub(crate) fn left_off_at(&self, home: &Home) -> Option<u32> {
        if !self.settings.remember_position() {
            return None;
        }
        let document = named(home)?;
        read(&self.file)
            .into_iter()
            .find(|place| place.document == document)
            .map(|place| place.line)
    }

    /// Writes down that the reader is at source `line` of `home`, and tidies the store
    /// while it is open ([`tidied`]).
    ///
    /// Silent about everything except a write it could not make, which is a fault of
    /// this machine rather than of the reader's document and belongs in the log.
    pub(crate) fn remember(&self, home: &Home, line: u32) {
        if !self.settings.remember_position() {
            return;
        }
        let Some(document) = named(home) else {
            return;
        };
        let now = now();
        let kept = tidied(
            read(&self.file),
            Place {
                document,
                line,
                seen: now,
            },
            now,
            &gone,
        );
        write(&self.file, &kept);
    }
}

/// The name the store keeps a document under, or `None` for one it cannot keep.
///
/// The reader's own path, which is the same path next time where the portal's is not
/// ([`Home::kept`]). A path holding a newline is not one this file can spell, and a
/// document with such a name is left unremembered rather than written down as two
/// broken lines.
fn named(home: &Home) -> Option<PathBuf> {
    let kept = home.kept()?;
    match kept.as_os_str().as_bytes().contains(&b'\n') {
        true => None,
        false => Some(kept.to_path_buf()),
    }
}

/// The store as it is written back once the reader's place is `newest`.
///
/// The whole of the cleanup policy, with the clock and the filesystem handed in — which
/// is what lets a test state that a document is gone or that a year has passed instead
/// of arranging one (`home.rs` does the same with the desktop's answer).
fn tidied(kept: Vec<Place>, newest: Place, now: u64, gone: &dyn Fn(&Path) -> bool) -> Vec<Place> {
    let here = newest.document.clone();
    let mut tidy = vec![newest];
    tidy.extend(
        kept.into_iter()
            // The document the reader is in now is in the list once, at the front:
            // where they are supersedes where they were.
            .filter(|place| place.document != here)
            .filter(|place| now.saturating_sub(place.seen) <= FORGOTTEN_AFTER)
            // Never asked of the newest: it is the document a window has open, so a
            // filesystem that will not talk about it cannot make the reader lose the
            // place they are standing in.
            .filter(|place| !gone(&place.document)),
    );
    tidy.truncate(REMEMBERED);
    tidy
}

/// Whether the filesystem says there is no such path — as opposed to saying nothing at
/// all, which is not the same answer and must not cost the reader an entry.
fn gone(path: &Path) -> bool {
    matches!(
        std::fs::symlink_metadata(path),
        Err(error) if error.kind() == ErrorKind::NotFound
    )
}

/// The places in `file`, or none at all: a store that is not there yet, one this
/// machine will not read, and one whose bytes are not what was written all mean the
/// same thing to a reader — the document opens at the top and nothing is said.
fn read(file: &Path) -> Vec<Place> {
    let Ok(stored) = std::fs::read(file) else {
        return Vec::new();
    };
    stored
        .split(|byte| *byte == b'\n')
        .filter_map(parsed)
        .collect()
}

/// One line of the store, or `None` for one that is not a place.
fn parsed(line: &[u8]) -> Option<Place> {
    let (seen, rest) = split(line)?;
    let (at, document) = split(rest)?;
    match document.is_empty() {
        true => None,
        false => Some(Place {
            seen: std::str::from_utf8(seen).ok()?.parse().ok()?,
            line: std::str::from_utf8(at).ok()?.parse().ok()?,
            document: PathBuf::from(std::ffi::OsString::from_vec(document.to_vec())),
        }),
    }
}

/// `line` up to its first space, and everything after it — the path being last is what
/// lets a document's own name hold spaces without any escaping at all.
fn split(line: &[u8]) -> Option<(&[u8], &[u8])> {
    let at = line.iter().position(|byte| *byte == b' ')?;
    Some((&line[..at], &line[at + 1..]))
}

/// Writes `places` to `file`, through a file beside it: a store is replaced whole or
/// not at all, so nothing that reads it can ever see half a write.
fn write(file: &Path, places: &[Place]) {
    let mut written = Vec::new();
    for place in places {
        written.extend_from_slice(format!("{} {} ", place.seen, place.line).as_bytes());
        written.extend_from_slice(place.document.as_os_str().as_bytes());
        written.push(b'\n');
    }

    let Some(directory) = file.parent() else {
        return;
    };
    let beside = file.with_extension("writing");
    if let Err(error) = std::fs::create_dir_all(directory)
        .and_then(|()| std::fs::write(&beside, &written))
        .and_then(|()| std::fs::rename(&beside, file))
    {
        eprintln!("axiomd: could not write down where you are in your documents: {error}",);
        let _ = std::fs::remove_file(&beside);
    }
}

/// Now, in seconds since the epoch. A clock somebody has set behind the epoch reads as
/// the epoch, which costs one entry its age and nothing else.
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::ScratchDir;

    /// A day, in the seconds the store counts in.
    const DAY: u64 = 24 * 60 * 60;

    /// The moment every test here calls now — a fixed one, so nothing in this module
    /// is ever asserted against the wall clock.
    const NOW: u64 = 1_800_000_000;

    fn place(document: &str, line: u32, seen: u64) -> Place {
        Place {
            document: PathBuf::from(document),
            line,
            seen,
        }
    }

    fn documents(places: &[Place]) -> Vec<String> {
        places
            .iter()
            .map(|place| place.document.display().to_string())
            .collect()
    }

    /// Nothing is gone — the filesystem this test hands in has every document in it.
    fn all_there(_: &Path) -> bool {
        false
    }

    /// Where the reader is now goes to the front, and the place they had in that same
    /// document is replaced rather than added beside it. A store that grew an entry
    /// per visit would pass the cap in a week of reading one file.
    #[test]
    fn the_place_a_reader_is_in_now_replaces_the_one_they_had_there() {
        let kept = vec![
            place("/home/reader/notes.md", 12, NOW - DAY),
            place("/home/reader/guide.md", 40, NOW - 2 * DAY),
        ];

        let tidy = tidied(
            kept,
            place("/home/reader/notes.md", 96, NOW),
            NOW,
            &all_there,
        );

        assert_eq!(
            documents(&tidy),
            ["/home/reader/notes.md", "/home/reader/guide.md"],
        );
        assert_eq!(
            tidy[0].line, 96,
            "the reader was left where they used to be"
        );
        assert_eq!(tidy[0].seen, NOW);
    }

    /// The cap: the store never holds more than [`REMEMBERED`] documents, and what
    /// falls off the end is what the reader read longest ago.
    #[test]
    fn the_store_keeps_five_hundred_documents_and_forgets_the_oldest_first() {
        // One more than fits, each seen a second before the one in front of it, so
        // exactly one of them is the oldest.
        let kept: Vec<Place> = (0..REMEMBERED)
            .map(|nth| place(&format!("/home/reader/{nth}.md"), 1, NOW - nth as u64))
            .collect();
        let oldest = kept[REMEMBERED - 1].document.clone();

        let tidy = tidied(kept, place("/home/reader/new.md", 7, NOW), NOW, &all_there);

        assert_eq!(tidy.len(), REMEMBERED, "the store grew past its cap");
        assert_eq!(tidy[0].document, PathBuf::from("/home/reader/new.md"));
        assert!(
            !tidy.iter().any(|place| place.document == oldest),
            "the store dropped something other than the document read longest ago",
        );
        assert!(
            tidy.iter()
                .any(|place| place.document == Path::new("/home/reader/498.md")),
            "the store dropped a document that fits in it",
        );
    }

    /// The age ceiling: a document nobody has opened for half a year is forgotten,
    /// and one opened a day before the ceiling is not.
    #[test]
    fn a_document_unopened_for_half_a_year_is_forgotten() {
        let kept = vec![
            place("/home/reader/last-week.md", 3, NOW - 7 * DAY),
            place("/home/reader/on-the-day.md", 4, NOW - FORGOTTEN_AFTER),
            place("/home/reader/a-day-past.md", 5, NOW - FORGOTTEN_AFTER - DAY),
        ];

        let tidy = tidied(kept, place("/home/reader/now.md", 1, NOW), NOW, &all_there);

        assert_eq!(
            documents(&tidy),
            [
                "/home/reader/now.md",
                "/home/reader/last-week.md",
                "/home/reader/on-the-day.md",
            ],
            "the ceiling forgot the wrong documents",
        );
    }

    /// A document the reader has deleted or moved away is dropped when the store is
    /// next written — and one the filesystem merely will not talk about is kept,
    /// because "I could not look" is not "it is gone".
    #[test]
    fn a_document_that_is_no_longer_there_is_dropped_and_an_unreachable_one_is_kept() {
        let kept = vec![
            place("/home/reader/deleted.md", 9, NOW - DAY),
            place("/mnt/unplugged/notes.md", 9, NOW - DAY),
            place("/home/reader/still-here.md", 9, NOW - DAY),
        ];

        let tidy = tidied(
            kept,
            place("/home/reader/now.md", 1, NOW),
            NOW,
            &|document| document == Path::new("/home/reader/deleted.md"),
        );

        assert_eq!(
            documents(&tidy),
            [
                "/home/reader/now.md",
                "/mnt/unplugged/notes.md",
                "/home/reader/still-here.md",
            ],
        );
    }

    /// The document a window has open now is never asked about: under the document
    /// portal it is kept somewhere the sandbox has never been shown, so a filesystem
    /// that answers "no such path" about it must not lose the reader the very place
    /// they are standing in.
    #[test]
    fn the_document_the_reader_is_in_is_written_down_whatever_the_filesystem_says() {
        let tidy = tidied(
            Vec::new(),
            place("/home/reader/now.md", 42, NOW),
            NOW,
            &|_| true,
        );

        assert_eq!(documents(&tidy), ["/home/reader/now.md"]);
        assert_eq!(tidy[0].line, 42);
    }

    /// A place written down is the place read back, name and all — including a
    /// document whose name holds the space this file separates its fields with.
    #[test]
    fn a_place_survives_the_trip_through_the_file() {
        let scratch = ScratchDir::new("places-roundtrip");
        let file = scratch.path().join("axiomd/reading-positions");
        let places = vec![
            place("/home/reader/Reading Notes/chapter 3.md", 128, NOW),
            place("/home/reader/guide.md", 1, NOW - DAY),
        ];

        write(&file, &places);

        assert_eq!(read(&file), places);
    }

    /// The non-happy path the reader must never hear a word about: a store that is not
    /// there, and one whose bytes are not what axiomd wrote. Both are "open at the
    /// top" and nothing else.
    #[test]
    fn a_missing_or_damaged_store_reads_as_no_places_at_all() {
        let scratch = ScratchDir::new("places-damaged");

        assert_eq!(
            read(&scratch.path().join("never-written")),
            Vec::<Place>::new(),
        );

        let damaged = scratch.write(
            "reading-positions",
            "not a place at all\n\n1800000000 nine /home/reader/notes.md\n\
             1800000000 12 /home/reader/kept.md\n1800000000 12 \n",
        );
        assert_eq!(
            read(&damaged),
            vec![place("/home/reader/kept.md", 12, NOW)],
            "a damaged store lost the lines that were still good, or kept broken ones",
        );
    }

    /// The write is atomic: what a reader who lost power mid-write comes back to is
    /// the store as it was, and nothing is left lying beside it either.
    #[test]
    fn writing_the_store_leaves_nothing_half_written_beside_it() {
        let scratch = ScratchDir::new("places-atomic");
        let file = scratch.path().join("axiomd/reading-positions");

        write(&file, &[place("/home/reader/notes.md", 5, NOW)]);
        write(&file, &[place("/home/reader/notes.md", 9, NOW)]);

        assert_eq!(read(&file), vec![place("/home/reader/notes.md", 9, NOW)]);
        let beside: Vec<String> = std::fs::read_dir(file.parent().expect("the store's folder"))
            .expect("read the store's folder back")
            .map(|entry| {
                entry
                    .expect("an entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .filter(|name| name != "reading-positions")
            .collect();
        assert!(beside.is_empty(), "the write left {beside:?} behind");
    }
}
