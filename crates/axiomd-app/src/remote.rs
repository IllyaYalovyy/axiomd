//! The one network request axiomd makes, and the conditions it makes it under.
//!
//! Everything else in the app is offline by construction: the pipeline emits no
//! fetchable source, the rendered page's policy allows images from the app's own
//! scheme alone, and the view follows nothing outside the document it shows. This
//! module is the single deliberate exception ruled by the owner (D4): the reader
//! pressed a placeholder card, and axiomd goes and gets that one image.
//!
//! Nothing here can be reached except from that press. [`load`] takes a URL that
//! [`crate::links`] has already classified as a load request the reader activated,
//! and it is the only function in the app that starts a transfer.
//!
//! # Why WebKit's own downloader
//!
//! The bytes come back through `WebKitNetworkSession`, which is the HTTP stack
//! already in the process — the same TLS configuration, proxy settings and redirect
//! handling the platform gives every other GNOME application, with no second client
//! to keep correct and no dependency to add for it. What comes back is checked before
//! it is believed: a reply that is not an image is not one, however it is labelled,
//! and a reply that will not stop is cancelled rather than held.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;

/// The largest image axiomd will hold for a document. A reader pressing a button
/// asked for a picture, not for an unbounded transfer into their window's memory, and
/// a reply with no end to it would otherwise be one.
const LARGEST_IMAGE: u64 = 64 * 1024 * 1024;

/// An image the reader asked for, as it came back.
pub(crate) struct Fetched {
    pub(crate) body: Vec<u8>,
    pub(crate) content_type: String,
}

/// Fetches `url`, and calls `done` on the main loop with the image or with what to
/// tell the reader instead.
///
/// Returns at once: a slow server cannot make the window stop drawing (invariant 4).
/// `done` runs exactly once.
pub(crate) fn load(
    session: &webkit6::NetworkSession,
    url: &str,
    done: impl FnOnce(Result<Fetched, String>) + 'static,
) {
    if !is_loadable(url) {
        done(Err(
            "axiomd only loads images over http and https.".to_owned()
        ));
        return;
    }
    let Some(download) = session.download_uri(url) else {
        done(Err("This image could not be requested.".to_owned()));
        return;
    };

    // The bytes land in a file, which is what the downloader deals in; it is read
    // once and removed, so a window holds its images and the filesystem holds none.
    let scratch = match Scratch::new() {
        Some(scratch) => scratch,
        None => {
            download.cancel();
            done(Err("There is nowhere to put this image.".to_owned()));
            return;
        }
    };

    // Whoever finishes first — the reply, a failure, or the size limit — takes the
    // callback, so the reader is answered exactly once.
    let answer = Answer::new(done);
    let destination = scratch.destination();
    download.connect_decide_destination(move |download, _suggested| {
        download.set_allow_overwrite(true);
        download.set_destination(&destination);
        true
    });
    download.connect_received_data(move |download, _| {
        if download.received_data_length() > LARGEST_IMAGE {
            download.cancel();
        }
    });
    download.connect_failed({
        let answer = answer.clone();
        move |_, error| answer.give(Err(readable(error)))
    });
    download.connect_finished(move |download| {
        answer.give(finished(download, &scratch));
    });
}

/// Whether a placeholder's source is something this app will go and get.
///
/// Only what a remote image is: `https` first, `http` because plenty of documentation
/// still references it, and a protocol-relative source as the `https` it means. A
/// `file:` or `data:` source is not fetched — a document must not be able to turn a
/// button press into a read of the reader's disk.
fn is_loadable(url: &str) -> bool {
    let scheme = match url.split_once("//") {
        Some(("", _)) => return true,
        Some((scheme, _)) => scheme.trim_end_matches(':'),
        None => return false,
    };
    scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
}

/// The URL to actually request, which is the source with the one thing a document's
/// `axiomd://` base cannot resolve — a protocol-relative source — spelled out.
pub(crate) fn requestable(url: &str) -> String {
    match url.strip_prefix("//") {
        Some(rest) => format!("https://{rest}"),
        None => url.to_owned(),
    }
}

/// What a completed download amounts to: an image, or a reason there is none.
fn finished(download: &webkit6::Download, scratch: &Scratch) -> Result<Fetched, String> {
    let response = download
        .response()
        .ok_or_else(|| "That address answered with nothing.".to_owned())?;
    let content_type = response.mime_type().unwrap_or_default().to_string();
    if !content_type.starts_with("image/") {
        return Err(format!(
            "That address answered with {}, which is not an image.",
            if content_type.is_empty() {
                "no content type"
            } else {
                &content_type
            }
        ));
    }
    let body = scratch
        .take()
        .map_err(|error| format!("This image could not be read back: {error}."))?;
    if body.is_empty() {
        return Err("That address answered with an empty image.".to_owned());
    }
    Ok(Fetched { body, content_type })
}

/// A downloader's error as something to show a reader, rather than a GLib domain and
/// a code.
fn readable(error: &glib::Error) -> String {
    let message = error.message().trim().to_owned();
    if message.is_empty() {
        "This image could not be loaded.".to_owned()
    } else {
        format!("{message}.")
    }
}

/// What the reader is told when the image arrives, or does not.
type Told = Box<dyn FnOnce(Result<Fetched, String>)>;

/// The one-shot callback, shared between the signals that could reach it.
struct Answer(Rc<RefCell<Option<Told>>>);

impl Answer {
    fn new(done: impl FnOnce(Result<Fetched, String>) + 'static) -> Self {
        Answer(Rc::new(RefCell::new(Some(Box::new(done)))))
    }

    fn clone(&self) -> Self {
        Answer(self.0.clone())
    }

    fn give(&self, outcome: Result<Fetched, String>) {
        let taken = self.0.borrow_mut().take();
        if let Some(done) = taken {
            done(outcome);
        }
    }
}

/// The file a download is written to, removed whatever becomes of it.
struct Scratch {
    path: std::path::PathBuf,
}

impl Scratch {
    fn new() -> Option<Self> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);

        let path = glib::tmp_dir().join(format!(
            "axiomd-image-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        ));
        // The downloader is given a path rather than a URI, and it must be absolute.
        // Probed on WebKitGTK 2.52.5: passing `file:///…` trips
        // `webkit_download_set_destination: assertion 'g_path_is_absolute(destination)'
        // failed` and the download lands in the user's Downloads folder instead.
        path.is_absolute().then_some(Scratch { path })
    }

    /// Where the downloader is told to put the bytes.
    fn destination(&self) -> String {
        self.path.display().to_string()
    }

    /// Reads the downloaded bytes and removes the file.
    fn take(&self) -> std::io::Result<Vec<u8>> {
        let body = std::fs::read(&self.path);
        let _ = std::fs::remove_file(&self.path);
        body
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The list is short on purpose: a button in a document must not be able to
    /// become a read of the reader's disk, or a way to hand the app content it never
    /// showed them.
    #[test]
    fn only_a_real_remote_image_is_something_the_app_will_go_and_get() {
        assert!(is_loadable("https://example.com/a.png"));
        assert!(is_loadable("http://example.com/a.png"));
        assert!(is_loadable("//example.com/a.png"));
        assert!(is_loadable("HTTPS://EXAMPLE.COM/A.PNG"));

        for refused in [
            "file:///etc/passwd",
            "data:image/svg+xml;base64,PHN2Zy8+",
            "javascript:alert(1)",
            "ftp://example.com/a.png",
            "/etc/passwd",
            "",
        ] {
            assert!(!is_loadable(refused), "{refused} would have been fetched");
        }
    }

    /// A protocol-relative source has no scheme to inherit from an `axiomd://` page,
    /// so it is requested as the secure one rather than not at all.
    #[test]
    fn a_protocol_relative_source_is_requested_over_https() {
        assert_eq!(
            requestable("//example.com/a.png"),
            "https://example.com/a.png"
        );
        assert_eq!(
            requestable("http://example.com/a.png"),
            "http://example.com/a.png"
        );
    }
}
