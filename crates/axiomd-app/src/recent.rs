//! The reader's recent documents: told to the desktop (issue #52), and offered back to
//! them inside axiomd (issue #54).
//!
//! Every document application on this desktop feeds one list: the reader opens a file,
//! and it turns up in Files → Recent, in the file chooser's Recent view and in Shell
//! search, whichever application it was opened in. axiomd fed nothing into it, so a
//! document read here was a document the desktop had never heard of. [`opened`] is the
//! whole of the fix, and it is one call wide: a window says a document was opened, and
//! this writes it down where the desktop keeps such things.
//!
//! [`offer`] is the way back. The primary menu's *Open Recent* is a view of that same
//! list, filtered to the documents axiomd registered — the convenient one, two presses
//! from any window, for the reader who does not want to go out to Files to come back to
//! the note they were reading this morning.
//!
//! # There is one list, and it is the desktop's
//!
//! `GtkRecentManager` *is* the store — `recently-used.xbel` under the data directory,
//! shared by every application on the desktop, aged and capped by the desktop itself.
//! axiomd writes entries into it and reads them back out; it keeps no list of its own,
//! prunes nothing, and has nothing to hold in step with anything (owner ruling, issue
//! #54: one store, not two — a private list would start disagreeing with the desktop's
//! the first time another application touched a document).
//!
//! What is read back is what [`opened`] wrote and nothing else: entries this
//! application registered (`has_application`), most recently used first, of documents
//! that are still there. It is the same shape GNOME's own document viewer reads that
//! list in — `gtk_recent_manager_get_items`, `gtk_recent_info_has_application`,
//! `gtk_recent_info_get_modified`, `gtk_recent_info_is_local`, `gtk_recent_info_exists`
//! are all in `/usr/bin/evince` on this machine, which is the reference implementation
//! for every choice in [`latest`].
//!
//! # Privacy is the desktop's, and there is no knob here
//!
//! GNOME's *File History* switch (`org.gnome.desktop.privacy remember-recent-files`)
//! reaches GTK as `Gtk/RecentFilesEnabled` — `gsd-xsettings` maps one to the other
//! (probed: both names are in `/usr/libexec/gsd-xsettings`) — and GTK's own
//! `gtk-recent-files-enabled` decides whether anything is written at all: "if set to
//! FALSE, the list will always be empty" (`Gtk-4.0.gir`, `gtk/gtksettings.c`). Probed
//! on GTK 4.20.4: with it off, [`gtk::RecentManager::add_full`] still answers `true`
//! and the file is never so much as created.
//!
//! So a reader who has turned their File History off is already obeyed, by the toolkit,
//! for every application at once. An axiomd switch beside it could only disagree with
//! the desktop, which is why there is none and why invariant 14 does not apply here
//! (recorded in `ux_decisions.md`).
//!
//! # Never on the reader's path
//!
//! Registration is metadata about a document, not part of showing it: nothing here can
//! interrupt an open (invariant 12), and nothing about it happens while the reader is
//! waiting. The desktop's recent list is a file that has to be read before it can be
//! added to — every entry of it, of every application on the machine — so the whole of
//! this is left for the loop to do when it has nothing better on (invariant 4). What
//! the reader gets is their document; what the desktop gets, it gets a moment later.
//!
//! Reading it back is the other way round: it happens because the reader has just asked
//! for the menu, and what it costs is the list the manager already holds in memory plus
//! one `stat` per document until [`OFFERED`] of them are found. Nothing is read from
//! disk that the toolkit has not read already, and no document is looked at that the
//! reader is not about to be shown.

use std::path::{Path, PathBuf};

use axiomd_doc::Home;
use axiomd_i18n::gettext;
use gtk::gio;
use gtk::prelude::*;

use crate::shell::{APP_ID, OPEN_RECENT};

/// What axiomd tells the desktop one of its documents is.
///
/// `text/markdown` — the type the desktop entry says axiomd handles, so a document
/// registered under it is offered back to axiomd when the reader opens it from the
/// recent list.
const MARKDOWN: &str = "text/markdown";

/// Writes down that the reader has just opened the document at `home`.
///
/// The path the *reader* keeps it at ([`Home::kept`]) and never the one axiomd reached
/// it by: under the document portal that is a fuse path carrying an id the desktop
/// mints afresh for each open, so a recent entry spelled with it would name nothing at
/// all by the time the reader pressed it. A document the desktop will not place has no
/// such path, and nothing is written down for it — an entry the desktop cannot reopen
/// is worse than no entry.
///
/// Returns at once: the desktop is told on a later turn of the main loop, so a document
/// arriving never waits for a list of every file the reader has opened this month to be
/// read off the disk.
pub(crate) fn opened(home: &Home) {
    let Some(kept) = home.kept() else {
        return;
    };
    let uri = gio::File::for_path(kept).uri();
    glib::idle_add_local_once(move || tell_the_desktop(&uri));
}

/// The registration itself, off the open path.
fn tell_the_desktop(uri: &str) {
    let entry = gtk::RecentData::new(
        // No display name and no description: the desktop names the entry after the
        // file itself, which is what the reader calls it and what every other
        // application leaves it as.
        None,
        None,
        MARKDOWN,
        APP_ID,
        // The command the entry says opened it, in GTK's own spelling for this field
        // (`gtk_recent_manager_add_item` builds `<prgname> %u`) — the name this copy of
        // axiomd was started as, which inside the flatpak is the command the package
        // exports.
        &format!("{} %u", glib::prgname().unwrap_or_else(|| "axiomd".into())),
        &[],
        false,
    );
    if !gtk::RecentManager::default().add_full(uri, &entry) {
        // Not something the reader can act on and not something they are told: the
        // document is open and on screen either way, and a banner about the desktop's
        // bookkeeping would be an interruption on the open path (invariant 12). It is a
        // fault of this machine, so it goes where the other ones go.
        eprintln!("axiomd: the desktop would not add {uri} to its recent documents");
    }
}

/// Says that axiomd has just *written* the file at `file`, which is never a document
/// the reader opened — and takes back the desktop's entry for it if one turns up.
///
/// An exported PDF is a copy the reader made for somebody else, not something they are
/// reading: it does not belong in the recent list (issue #52 rules exports out in as
/// many words), and a reader who exports a chapter a day would otherwise find their
/// recent documents full of files they never read.
///
/// # Why it is taken back rather than not made
///
/// axiomd does not make it. The toolkit's print machinery — which is what an export is
/// written by (`export.rs`) — registers the file it wrote, on its own account and after
/// the operation has reported itself finished. Measured on GTK 4.20.4 and WebKitGTK 6
/// through the e2e harness, exporting `notes.pdf`: the entry is `application/pdf` under
/// the application name `axiomd` — `g_get_application_name()`, with `g_get_prgname()`
/// as its exec line, which is exactly what `gtk_recent_manager_add_item` fills those
/// two fields with and neither of them what [`opened`] writes. At the moment the export
/// reports done the entry is not there yet (`RecentManager::lookup_item` answers
/// "Unable to find an item with URI …"), so there is no point at which axiomd could
/// decline it: what there is, is the desktop's own `changed`, and the entry is taken
/// back the moment it appears.
///
/// A job on its way to a printer is written by the same call and so passes through here
/// too, which is why this is asked about the file the operation wrote rather than about
/// an export. Nothing was ever observed in the list for one — measured with this
/// removal taken out, a print job leaves the desktop's list holding the document the
/// reader was reading and nothing else — so there is no test of that half, and this is
/// what would take it back if a toolkit ever registered it.
///
/// Only an entry that is axiomd's own is removed. A file some other application has in
/// the reader's list is *their* entry — a PDF read in Papers and exported over from
/// here is still a PDF the reader read — and taking it away would cost them something
/// they did do in order to stop saying something they did not.
pub(crate) fn written(file: &Path) {
    let manager = gtk::RecentManager::default();
    let uri = gio::File::for_path(file).uri().to_string();

    let start_watching = WRITTEN.with_borrow_mut(|written| {
        written.uris.retain(|kept| *kept != uri);
        written.uris.push(uri);
        // What the toolkit registers, it registers within moments of writing the file,
        // so a handful of the most recent is all that can still be waiting for. The
        // bound is what keeps a session of exports from being a list that only grows.
        while written.uris.len() > AWAITED {
            written.uris.remove(0);
        }
        written.watching.is_none()
    });
    if start_watching {
        let watching = manager.connect_changed(take_back);
        WRITTEN.with_borrow_mut(|written| written.watching = Some(watching));
    }
    // In case this desktop is quicker than the one above was measured on: an entry that
    // is already there needs no `changed` to be taken back.
    take_back(&manager);
}

/// How many written files are watched for at once.
///
/// Two windows exporting at the same time is two, and everything else on the list is a
/// file the desktop was never going to say anything more about.
const AWAITED: usize = 16;

/// The files this copy of axiomd has written, and its subscription to the desktop
/// saying its list has changed.
///
/// One per process rather than per window because the desktop's recent list is one per
/// desktop: there is nothing here a window could own, and nothing one window's export
/// changes about another's (invariant 7).
struct Written {
    uris: Vec<String>,
    watching: Option<glib::SignalHandlerId>,
}

thread_local! {
    static WRITTEN: std::cell::RefCell<Written> = const {
        std::cell::RefCell::new(Written {
            uris: Vec::new(),
            watching: None,
        })
    };
}

/// Removes every file axiomd wrote from the desktop's list, of those that have turned
/// up in it and are axiomd's own.
///
/// Removing is itself a change, so this runs again on its own removal — and answers
/// nothing the second time, because what it removed is no longer there. The list is
/// copied out before anything is removed, so no borrow is held across a call that can
/// come straight back here.
fn take_back(manager: &gtk::RecentManager) {
    let uris = WRITTEN.with_borrow(|written| written.uris.clone());
    for uri in uris {
        let Ok(Some(entry)) = manager.lookup_item(&uri) else {
            continue;
        };
        if only_axiomd(&entry) {
            let _ = manager.remove_item(&uri);
        }
    }
}

/// Whether this application is the only one that has ever registered `entry` — under
/// the name this module writes, or the one the toolkit writes on its own account.
fn only_axiomd(entry: &gtk::RecentInfo) -> bool {
    let toolkit = glib::application_name().unwrap_or_default();
    entry
        .applications()
        .iter()
        .all(|name| name.as_str() == APP_ID || *name == toolkit)
}

/// How many documents the reader is offered a way back to (issue #54, owner's range of
/// five to ten).
///
/// Ten, the whole of it. The list is a popover of its own, so its length costs the menu
/// it hangs off nothing, and the reader opening it is looking for a document they had
/// open earlier — a session of reading is easily eight or nine documents, and the one
/// they want is the one a shorter list would have just dropped. Ten is still a glance
/// rather than a search, which is what keeps the search box the issue rules out from
/// being missed.
const OFFERED: usize = 10;

/// Offers the reader the documents they have been reading, as a submenu of `into` —
/// and offers nothing at all when there are none (issue #54).
///
/// `into` is the menu's documents section, so this lands directly under *Open…*: the
/// way back to a document the reader has read belongs beside the two ways to one they
/// have not.
///
/// # Nothing to offer is nothing shown
///
/// A reader with no recent documents — a first run, a desktop with *File History*
/// turned off, a list whose documents have all been moved away — gets a menu without an
/// *Open Recent* in it, never an error and never a dead end. An item that is there and
/// can never be pressed is an affordance that does nothing, which is the defect this
/// project treats as a defect; and the menu already has a section that is there only
/// when it has something in it, the history a narrow window takes out of the header bar
/// (issue #30). Recorded in `ux_decisions.md`.
pub(crate) fn offer(into: &gio::Menu) {
    let documents = latest(OFFERED);
    if documents.is_empty() {
        return;
    }
    let entries = gio::Menu::new();
    for (label, uri) in told_apart(&documents).into_iter().zip(documents) {
        let item = gio::MenuItem::new(Some(&label), None);
        // The `file:` URI the desktop keys the entry on, never the path spelled out as
        // text: an action's target is text and a path is bytes, so a document whose name
        // this machine's encoding cannot spell would arrive as something else and open
        // something else or nothing.
        item.set_action_and_target_value(Some(OPEN_RECENT), Some(&uri.to_variant()));
        entries.append_item(&item);
    }
    into.append_submenu(Some(&gettext("Open _Recent")), &entries);
}

/// The `most` documents this reader last read in axiomd, newest first, as the `file:`
/// URIs the desktop knows them by.
///
/// Four rules, and every one of them is the one GNOME's own document viewer reads this
/// list by (see the module header):
///
/// * **axiomd's own.** `has_application` — the reader's list holds every document every
///   application on the machine has opened, and a Markdown viewer offering to open the
///   spreadsheet they had in another application would be offering to fail.
/// * **Most recently used first.** `modified` and not `visited` or `added`: probed on
///   GTK 4.20.4, re-registering a document already in the list moves `modified` and
///   leaves the other two at the moment it was first added — so `modified` is the only
///   one of the three that answers "when did the reader last read this".
/// * **Still there.** `exists` — an entry that cannot be opened is worse than no entry,
///   and it is a `stat` of documents the reader is about to be shown rather than of the
///   list. (Non-local entries answer `false` here as well, which is the same rule: a
///   document on a mount this session cannot reach is not one to offer.)
/// * **Bounded.** The walk stops at `most`, so a healthy list costs `most` calls.
fn latest(most: usize) -> Vec<String> {
    let mut listed: Vec<gtk::RecentInfo> = gtk::RecentManager::default()
        .items()
        .into_iter()
        .filter(|entry| entry.has_application(APP_ID))
        .collect();
    listed.sort_by_key(|entry| std::cmp::Reverse(entry.modified()));
    listed
        .into_iter()
        .filter(gtk::RecentInfo::exists)
        .map(|entry| entry.uri().to_string())
        .take(most)
        .collect()
}

/// What each of `documents` is called in the menu: the name the reader gave it, and as
/// much of the folder it is in as it takes to tell it from the others on the list.
///
/// A reader with `notes.md` in three projects would otherwise be reading one word three
/// times and choosing at random. The folder is added only where it settles something —
/// naming every entry twice over would cost every reader something to spare that one —
/// and it grows a component at a time until the list has no two entries that read the
/// same, which is a thing that terminates because a path has an end.
fn told_apart(documents: &[String]) -> Vec<String> {
    let paths: Vec<PathBuf> = documents
        .iter()
        .map(|uri| gio::File::for_uri(uri).path().unwrap_or_else(|| uri.into()))
        .collect();
    let mut depth = vec![0usize; paths.len()];
    loop {
        let shown: Vec<String> = paths
            .iter()
            .zip(&depth)
            .map(|(path, folders)| named(path, *folders))
            .collect();
        let mut deepened = false;
        for (at, path) in paths.iter().enumerate() {
            let shares_a_name = shown
                .iter()
                .enumerate()
                .any(|(other, name)| other != at && *name == shown[at]);
            if shares_a_name && depth[at] < folders_of(path) {
                depth[at] += 1;
                deepened = true;
            }
        }
        if !deepened {
            // Mnemonics last, over the finished words: GTK reads an underscore in a menu
            // label as "underline the next letter", so a document called `release_notes`
            // would be drawn as `releasenotes` with an N underlined (probed on GTK
            // 4.20.4). Doubling it is how a label says the character itself.
            return shown.iter().map(|name| name.replace('_', "__")).collect();
        }
    }
}

/// One document's name, with the innermost `folders` of the folder it is in beside it.
fn named(path: &Path, folders: usize) -> String {
    let name = path
        .file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned();
    if folders == 0 {
        return name;
    }
    let parent = path.parent().unwrap_or(Path::new(""));
    let whole = folders >= folders_of(path);
    let folder = match whole {
        // Everything short of the whole path is a folder or two of it; the last step is
        // the path itself, which is the one form that cannot be shared by two documents.
        true => parent.to_string_lossy().into_owned(),
        false => parent
            .iter()
            .rev()
            .take(folders)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<PathBuf>()
            .to_string_lossy()
            .into_owned(),
    };
    // Two names on one line, the second of them a place. Written out rather than
    // formatted in so that a language that puts them the other way round can.
    filled(&gettext("{document} ({folder})"), &name, &folder)
}

/// How many folders `path` has above it — how far [`named`] can go.
fn folders_of(path: &Path) -> usize {
    path.parent().map_or(0, |parent| parent.iter().count())
}

/// `template` with its two placeholders filled in, and nothing else read as one — a
/// document called `{folder}.md` is a document, not a placeholder.
fn filled(template: &str, document: &str, folder: &str) -> String {
    template
        .split("{document}")
        .map(|part| part.replace("{folder}", folder))
        .collect::<Vec<_>>()
        .join(document)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(documents: &[&str]) -> Vec<String> {
        told_apart(
            &documents
                .iter()
                .map(|path| gio::File::for_path(path).uri().to_string())
                .collect::<Vec<_>>(),
        )
    }

    /// What the reader reads: the document's own name, and nothing else to read past.
    #[test]
    fn a_document_is_offered_under_the_name_the_reader_gave_it() {
        assert_eq!(
            names(&["/home/reader/Notes/chapter 3.md", "/home/reader/guide.md"]),
            ["chapter 3.md", "guide.md"],
        );
    }

    /// The defect this exists for: a reader who keeps a `notes.md` in every project
    /// would be offered the same word twice and have to guess which was which.
    #[test]
    fn documents_sharing_a_name_are_told_apart_by_the_folder_they_are_in() {
        assert_eq!(
            names(&[
                "/home/reader/wayfinder/notes.md",
                "/home/reader/atlas/notes.md",
                "/home/reader/atlas/guide.md",
            ]),
            [
                "notes.md (wayfinder)",
                "notes.md (atlas)",
                // Untouched: it shares its name with nothing, so nothing is added to it.
                "guide.md",
            ],
        );
    }

    /// And when the folders are the same word too, which is what a reader with a
    /// `docs/notes.md` in two projects has. The list keeps growing the place until no
    /// two entries read the same — the whole path, if that is what it takes.
    #[test]
    fn documents_whose_folders_share_a_name_are_told_apart_too() {
        assert_eq!(
            names(&[
                "/home/reader/wayfinder/docs/notes.md",
                "/home/reader/atlas/docs/notes.md",
            ]),
            ["notes.md (wayfinder/docs)", "notes.md (atlas/docs)"],
        );

        // And a folder with nothing left above it: the whole path is the last thing a
        // name can grow into, and two documents cannot share one.
        assert_eq!(
            names(&["/srv/docs/notes.md", "/docs/notes.md"]),
            ["notes.md (srv/docs)", "notes.md (/docs)"],
        );
    }

    /// A menu label is read for mnemonics, so a name with an underscore in it has to
    /// say so or the reader is shown a different name from the one on their disk.
    #[test]
    fn an_underscore_in_a_documents_name_reaches_the_reader() {
        assert_eq!(
            names(&["/home/reader/release_notes.md"]),
            ["release__notes.md"],
            "the label GTK draws as `release_notes.md` is spelled with the underscore \
             doubled",
        );
    }

    /// The words the folder is put beside the name with are a sentence to be
    /// translated, not a format to be trusted with the reader's own file names.
    #[test]
    fn a_document_named_after_the_placeholder_is_still_named_after_itself() {
        assert_eq!(
            filled("{document} ({folder})", "{folder}.md", "notes"),
            "{folder}.md (notes)",
        );
    }
}
