//! Telling the desktop that the reader opened a document (issue #52).
//!
//! Every document application on this desktop feeds one list: the reader opens a file,
//! and it turns up in Files → Recent, in the file chooser's Recent view and in Shell
//! search, whichever application it was opened in. axiomd fed nothing into it, so a
//! document read here was a document the desktop had never heard of. This module is the
//! whole of the fix, and it is one call wide: a window says a document was opened, and
//! this writes it down where the desktop keeps such things.
//!
//! # There is no recent list of our own
//!
//! `GtkRecentManager` *is* the store — `recently-used.xbel` under the data directory,
//! shared by every application on the desktop, aged and capped by the desktop itself.
//! axiomd writes entries into it and nothing else: it does not read it back, does not
//! prune it, and has no list of its own to keep in step with it. There is no in-app
//! recent surface in this tier (owner decision, issue #52), which is what makes that
//! possible.
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

use std::path::Path;

use axiomd_doc::Home;
use gtk::gio;
use gtk::prelude::*;

use crate::shell::APP_ID;

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
