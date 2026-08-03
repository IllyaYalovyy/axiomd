//! The `axiomd://` origin — the only thing a document webview can reach.
//!
//! A window publishes its document here and gets back a URI to load. Everything the
//! rendered page then asks for comes back through this one handler: the page itself,
//! the bundled stylesheet, and the images the document names relative to its own
//! directory. There is no `file://` grant anywhere in the app, so this module is the
//! complete list of bytes a document can obtain.
//!
//! # The vocabulary
//!
//! * `axiomd://assets/<name>` — a bundled asset, matched by exact name against a
//!   compiled-in table. Nothing here touches the filesystem.
//! * `axiomd://doc-<n>/` — the rendered HTML of published document `n`.
//! * `axiomd://doc-<n>/<path>` — a file under that document's own directory.
//!
//! Putting each document on its own host is what makes relative references work
//! without a base-URI trick: the browser resolves `images/logo.png` against the
//! page's own URI, so it arrives here as `axiomd://doc-<n>/images/logo.png` and is
//! looked up under that document's directory. It also caps a document's reach at its
//! own host — the host names the root, so a request can never describe a root it was
//! not given.
//!
//! # Staying inside the document's directory
//!
//! Two independent checks, because either one alone has a hole:
//!
//! * a request path may not contain a `..` component (a URI is decoded before it is
//!   split, so `%2e%2e` is caught too), which stops the request from *naming*
//!   anything outside the root;
//! * the file it lands on is canonicalised and must still be under the canonical
//!   root, which stops a *symlink inside the directory* from leading out of it.
//!
//! A request that fails either check is refused; it is never answered with bytes.
//!
//! # Where startup is measured
//!
//! This handler is also the finish line of the cold-start budget (issue #9). The one
//! moment that means "axiomd has a document for the reader" is the moment the page's
//! own bytes leave here — everything before it is the app's to be quick about, and
//! nothing after it is a thing the app does. [`Scheme::startup`] is that measurement,
//! and it is the only number in the app the perf harness cannot take from outside.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::gio;
use gtk::glib;

/// The URI scheme. Shared with `axiomd-render`, which writes it into every rendered
/// document's stylesheet link and content-security policy.
const SCHEME: &str = "axiomd";

/// The host bundled assets are served from; never a document root.
const ASSETS_HOST: &str = "assets";

/// The host prefix each published document gets, followed by its number.
const DOCUMENT_HOST_PREFIX: &str = "doc-";

/// The host prefix a document's loaded remote images get, followed by its number.
///
/// A host of their own rather than a reserved path under the document's, because the
/// document's host is a directory: any path there could be a file the reader has, and
/// a reserved one would be a name they are quietly not allowed to use.
const IMAGE_HOST_PREFIX: &str = "img-";

/// The bundled assets, by the path they are requested under. Compiled in, so an
/// asset request can neither miss nor escape onto the filesystem.
fn asset(path: &str) -> Option<(Vec<u8>, &'static str)> {
    match path {
        "/axiomd.css" => Some((axiomd_render::stylesheet().as_bytes().to_vec(), "text/css")),
        // A plugin's own files, under `plugin/<id>/`. Compiled in like the stylesheet
        // above and looked up the same way, so a plugin can no more reach the
        // filesystem through this handler than the document can. A page only ever names
        // one of these when the plugin that owns it was switched on and used while it
        // was rendered.
        _ => axiomd_render::Plugins::asset(path)
            .map(|asset| (asset.bytes.to_vec(), asset.content_type)),
    }
}

/// The published documents of one application, and the handler that answers for
/// them.
pub(crate) struct Scheme {
    origin: Origin,
    next: Cell<u64>,
}

/// Everything an answer is made from: the documents published now, and the stopwatch
/// the first page served stops.
///
/// One value rather than two because the handler closure needs both and they are the
/// same thing — what this origin has answered, and what it has answered *with*.
#[derive(Clone)]
struct Origin {
    documents: Rc<RefCell<HashMap<u64, Document>>>,
    startup: Rc<Startup>,
}

/// How long this process took to have a document for the reader.
///
/// The clock starts when the application builds itself — the first thing axiomd does
/// that is axiomd rather than the loader — and stops the first time this handler
/// answers with a document's own bytes. Nothing else reads it and nothing branches on
/// it: it is an instrument, like the view's navigation count, and it is what the perf
/// harness holds to the cold-start budget.
struct Startup {
    began: Instant,
    to_first_document: Cell<Option<Duration>>,
}

impl Startup {
    fn new() -> Self {
        Self {
            began: Instant::now(),
            to_first_document: Cell::new(None),
        }
    }

    /// Called every time a document's page is served, and only the first one counts:
    /// a re-render, a second window and a document opened an hour later are not
    /// startup.
    fn served_a_document(&self) {
        if self.to_first_document.get().is_none() {
            self.to_first_document.set(Some(self.began.elapsed()));
        }
    }
}

/// One document as the webview sees it: where its relative references resolve, the
/// page currently rendered for it, and the remote images its reader has asked for.
struct Document {
    /// `None` for a document that has never been saved: it is nowhere, so nothing
    /// relative to it resolves anywhere and every file request it makes is refused.
    root: Option<PathBuf>,
    html: Option<String>,
    /// The images fetched for this document, in the order they were asked for. They
    /// live here, and only here, so closing the window frees them with everything
    /// else it held (invariant 7) and no other document can reach them.
    images: Vec<Image>,
}

/// One remote image the reader pressed the button for, held as the bytes that came
/// back rather than as the URL they came from: nothing in the document can name it
/// again, and nothing about it is fetched twice.
struct Image {
    body: Vec<u8>,
    content_type: String,
}

/// The answer to one request.
#[derive(Debug, PartialEq, Eq)]
enum Served {
    Bytes {
        body: Vec<u8>,
        content_type: String,
    },
    /// Nothing is published at this URI.
    Missing,
    /// The URI named something outside the document's own directory, or a scheme
    /// this handler does not own. Never answered with bytes.
    Refused,
}

impl Scheme {
    pub(crate) fn new() -> Self {
        Self {
            origin: Origin {
                documents: Rc::new(RefCell::new(HashMap::new())),
                startup: Rc::new(Startup::new()),
            },
            next: Cell::new(0),
        }
    }

    /// How long this process took from building itself to answering with its first
    /// document, or `None` while it has not answered with one.
    ///
    /// The cold-start budget (issue #9, VISION: under 300 ms on a typical file) is this
    /// number. It excludes what happens before the application exists — `execve` and
    /// the dynamic loader — because that is the only part of a launch axiomd's own code
    /// has no say in.
    pub(crate) fn startup(&self) -> Option<Duration> {
        self.origin.startup.to_first_document.get()
    }

    /// Makes `context`'s webviews serve their documents from this scheme.
    ///
    /// Must run before any webview using `context` loads a document URI; WebKit
    /// answers an unregistered scheme with a load error.
    pub(crate) fn install(&self, context: &webkit6::WebContext) {
        let origin = self.origin.clone();
        context.register_uri_scheme(SCHEME, move |request| {
            let uri = request.uri().unwrap_or_default();
            let (status, body, content_type) = match origin.serve(&uri) {
                Served::Bytes { body, content_type } => (200, body, content_type),
                Served::Missing => (404, Vec::new(), "text/plain".to_owned()),
                Served::Refused => (403, Vec::new(), "text/plain".to_owned()),
            };
            let length = body.len() as i64;
            let stream = gio::MemoryInputStream::from_bytes(&glib::Bytes::from_owned(body));
            let response = webkit6::URISchemeResponse::new(&stream, length);
            response.set_content_type(&content_type);
            response.set_status(status, None);
            request.finish_with_response(&response);
        });
    }

    /// Publishes `file`, giving its window a URI to load.
    ///
    /// The document starts blank: it shows nothing until [`Publication::show`] hands
    /// it a rendered page. Relative references resolve against `file`'s directory
    /// and nothing above it — and an untitled document, which has no directory,
    /// reaches nothing at all.
    pub(crate) fn publish(&self, file: Option<&Path>) -> Publication {
        let id = self.next.get();
        self.next.set(id + 1);
        let root = file.map(|file| {
            let root = file.parent().unwrap_or(Path::new("."));
            root.canonicalize().unwrap_or_else(|_| root.to_path_buf())
        });
        self.origin.documents.borrow_mut().insert(
            id,
            Document {
                root,
                html: None,
                images: Vec::new(),
            },
        );
        Publication {
            documents: self.origin.documents.clone(),
            id,
            uri: format!("{SCHEME}://{DOCUMENT_HOST_PREFIX}{id}/"),
        }
    }
}

/// A document's place on the scheme, for as long as its window exists.
///
/// Dropping it withdraws the document: its page and its directory become
/// unreachable, so a closed window leaves nothing behind that a webview could still
/// ask for.
pub(crate) struct Publication {
    documents: Rc<RefCell<HashMap<u64, Document>>>,
    id: u64,
    uri: String,
}

impl Publication {
    /// The URI a webview loads to show this document.
    pub(crate) fn uri(&self) -> &str {
        &self.uri
    }

    /// Makes `html` the page served for this document.
    pub(crate) fn show(&self, html: String) {
        if let Some(document) = self.documents.borrow_mut().get_mut(&self.id) {
            document.html = Some(html);
        }
    }

    /// Puts a remote image the reader asked for where this document's view can see
    /// it, and answers with the URI to point at it.
    ///
    /// The bytes are served from this document's own host, so the reader's image
    /// arrives through the same origin as the rest of their document — the page's
    /// `img-src axiomd:` policy needs no exception, and no other window can reach it.
    pub(crate) fn attach_image(&self, body: Vec<u8>, content_type: String) -> String {
        let mut documents = self.documents.borrow_mut();
        let Some(document) = documents.get_mut(&self.id) else {
            return String::new();
        };
        document.images.push(Image { body, content_type });
        format!(
            "{SCHEME}://{IMAGE_HOST_PREFIX}{}/{}",
            self.id,
            document.images.len() - 1
        )
    }

    /// The directory this document's relative references resolve under, or `None`
    /// for an untitled document, which has none.
    pub(crate) fn root(&self) -> Option<PathBuf> {
        self.documents
            .borrow()
            .get(&self.id)
            .and_then(|document| document.root.clone())
    }
}

impl Drop for Publication {
    fn drop(&mut self) {
        self.documents.borrow_mut().remove(&self.id);
    }
}

impl Origin {
    /// Answers one request URI. The whole policy of the scheme lives here.
    fn serve(&self, uri: &str) -> Served {
        let Ok(parsed) = glib::Uri::parse(uri, glib::UriFlags::NONE) else {
            return Served::Refused;
        };
        if parsed.scheme() != SCHEME {
            return Served::Refused;
        }
        let path = parsed.path();
        let host = parsed.host().unwrap_or_default();

        if host == ASSETS_HOST {
            return match asset(&path) {
                Some((body, content_type)) => Served::Bytes {
                    body,
                    content_type: content_type.to_owned(),
                },
                None => Served::Missing,
            };
        }

        if let Some(id) = number_after(&host, IMAGE_HOST_PREFIX) {
            return match self
                .documents
                .borrow()
                .get(&id)
                .and_then(|document| document.images.get(index_of(&path)?))
            {
                Some(image) => Served::Bytes {
                    body: image.body.clone(),
                    content_type: image.content_type.clone(),
                },
                None => Served::Missing,
            };
        }

        let Some(id) = number_after(&host, DOCUMENT_HOST_PREFIX) else {
            return Served::Refused;
        };
        let documents = self.documents.borrow();
        let Some(document) = documents.get(&id) else {
            return Served::Missing;
        };

        if path.is_empty() || path == "/" {
            return match &document.html {
                // The finish line of the cold-start budget, and the only place it can
                // be: this is the moment a document's own bytes leave axiomd.
                Some(html) => {
                    self.startup.served_a_document();
                    Served::Bytes {
                        body: html.clone().into_bytes(),
                        content_type: "text/html".to_owned(),
                    }
                }
                None => Served::Missing,
            };
        }
        match &document.root {
            Some(root) => file_under(root, &path),
            // An untitled document is nowhere on disk, so there is nothing under it.
            None => Served::Refused,
        }
    }
}

/// The number a host carries after `prefix`, when it is that kind of host at all.
fn number_after(host: &str, prefix: &str) -> Option<u64> {
    host.strip_prefix(prefix)?.parse().ok()
}

/// Which of a document's loaded images `/3` names.
fn index_of(path: &str) -> Option<usize> {
    path.strip_prefix('/')?.parse().ok()
}

/// The file `request` names under `root`, or `None` when it names something outside
/// it — however the escape is spelled, and whether it is spelled at all.
///
/// This is the containment rule itself, and the same one applies wherever a document
/// reaches for a path: the bytes the scheme will serve it, and the links it may
/// follow (`links.rs`). A returned path is inside the directory; it is not a promise
/// that anything is there.
pub(crate) fn path_under(root: &Path, request: &str) -> Option<PathBuf> {
    let mut target = root.to_path_buf();
    for component in Path::new(request).components() {
        match component {
            Component::Normal(name) => target.push(name),
            Component::RootDir | Component::CurDir => {}
            // The request naming anything above the root, before a filesystem is
            // even consulted.
            Component::ParentDir | Component::Prefix(_) => return None,
        }
    }
    // And a symlink inside the directory leading out of it, which the check above
    // cannot see. Only meaningful for a path that exists; one that does not is a
    // broken link, and saying so is the window's job rather than this rule's.
    if let Ok(resolved) = target.canonicalize() {
        if !resolved.starts_with(root.canonicalize().ok()?) {
            return None;
        }
        return Some(resolved);
    }
    Some(target)
}

/// Reads the file `request` names under `root`, refusing anything that leaves it.
fn file_under(root: &Path, request: &str) -> Served {
    let Some(resolved) = path_under(root, request) else {
        return Served::Refused;
    };
    match std::fs::read(&resolved) {
        Ok(body) => Served::Bytes {
            body,
            content_type: content_type(&resolved).to_owned(),
        },
        Err(_) => Served::Missing,
    }
}

/// What a document's own files are declared as. Shared with the exporter, so a
/// picture carried inside an exported page is declared exactly as the app serves
/// it. Anything the pipeline does not put in a document stays
/// `application/octet-stream`; the rendered page's content-security policy accepts
/// images and styles only, so an unknown type is inert even if something asks for it.
pub(crate) fn content_type(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        Some("bmp") => "image/bmp",
        Some("ico") => "image/vnd.microsoft.icon",
        Some("css") => "text/css",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::ScratchDir;

    /// The bytes of a one-pixel PNG, so an image test asserts on real image content
    /// rather than on a text file wearing a `.png` name.
    const PIXEL_PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR";

    fn served_bytes(served: Served) -> (Vec<u8>, String) {
        match served {
            Served::Bytes { body, content_type } => (body, content_type),
            other => panic!("expected bytes, got {other:?}"),
        }
    }

    #[test]
    fn serves_the_rendered_page_at_the_documents_own_uri() {
        let scratch = ScratchDir::new("scheme-page");
        let file = scratch.write("notes.md", "# Notes\n");
        let scheme = Scheme::new();
        let publication = scheme.publish(Some(&file));
        publication.show("<!DOCTYPE html><h1>Notes</h1>".to_owned());

        let (body, content_type) = served_bytes(scheme.origin.serve(publication.uri()));

        assert_eq!(
            String::from_utf8(body).unwrap(),
            "<!DOCTYPE html><h1>Notes</h1>"
        );
        assert_eq!(content_type, "text/html");
    }

    #[test]
    fn shows_nothing_until_a_page_has_been_rendered() {
        let scratch = ScratchDir::new("scheme-blank");
        let file = scratch.write("notes.md", "# Notes\n");
        let scheme = Scheme::new();
        let publication = scheme.publish(Some(&file));

        assert_eq!(scheme.origin.serve(publication.uri()), Served::Missing);
    }

    #[test]
    fn resolves_an_image_the_document_names_relative_to_itself() {
        let scratch = ScratchDir::new("scheme-image");
        let file = scratch.write("notes.md", "![](images/logo.png)\n");
        scratch.write("images/logo.png", PIXEL_PNG);
        let scheme = Scheme::new();
        let publication = scheme.publish(Some(&file));

        let request = format!("{}images/logo.png", publication.uri());
        let (body, content_type) = served_bytes(scheme.origin.serve(&request));

        assert_eq!(body, PIXEL_PNG);
        assert_eq!(content_type, "image/png");
    }

    /// Nothing above the document's own directory is ever answered with bytes,
    /// however the request spells the climb.
    #[test]
    fn serves_nothing_from_outside_the_document_directory() {
        let scratch = ScratchDir::new("scheme-climb");
        let file = scratch.write("inner/notes.md", "# Notes\n");
        scratch.write("secret.txt", "credentials");
        let scheme = Scheme::new();
        let publication = scheme.publish(Some(&file));

        for escape in [
            "../secret.txt",
            "%2e%2e/secret.txt",
            "images/../../secret.txt",
            "..%2Fsecret.txt",
            "/etc/passwd",
        ] {
            let request = format!("{}{escape}", publication.uri());
            let served = scheme.origin.serve(&request);
            assert!(
                !matches!(served, Served::Bytes { .. }),
                "{request} was answered with {served:?}",
            );
        }
    }

    /// The guard that does not depend on a URI parser having normalised the request
    /// first: a climbing path is refused on sight, before anything outside the
    /// document's directory is so much as looked at. That is why a climb to a file
    /// that does not exist is refused too, rather than reported as missing.
    #[test]
    fn refuses_a_path_that_climbs_out_of_the_document_directory() {
        let scratch = ScratchDir::new("scheme-climb-raw");
        scratch.write("inner/notes.md", "# Notes\n");
        scratch.write("secret.txt", "credentials");
        let root = scratch.path().join("inner");

        assert_eq!(file_under(&root, "/../secret.txt"), Served::Refused);
        assert_eq!(
            file_under(&root, "/images/../../secret.txt"),
            Served::Refused
        );
        assert_eq!(file_under(&root, "/../nothing-here.txt"), Served::Refused);
    }

    #[test]
    fn refuses_a_symlink_that_leads_out_of_the_document_directory() {
        let scratch = ScratchDir::new("scheme-symlink");
        let file = scratch.write("inner/notes.md", "# Notes\n");
        let secret = scratch.write("secret.txt", "credentials");
        std::os::unix::fs::symlink(&secret, scratch.path().join("inner/leak.txt"))
            .expect("create symlink");
        let scheme = Scheme::new();
        let publication = scheme.publish(Some(&file));

        let request = format!("{}leak.txt", publication.uri());

        assert_eq!(scheme.origin.serve(&request), Served::Refused);
    }

    /// The rendered document links this exact URI; if the scheme stopped answering
    /// it every document would render unstyled.
    #[test]
    fn serves_the_stylesheet_the_render_pipeline_links_to() {
        let scheme = Scheme::new();

        let (body, content_type) = served_bytes(scheme.origin.serve(axiomd_render::STYLESHEET_URI));

        assert_eq!(
            String::from_utf8(body).unwrap(),
            axiomd_render::stylesheet()
        );
        assert_eq!(content_type, "text/css");
    }

    #[test]
    fn has_no_asset_other_than_the_ones_it_bundles() {
        let scheme = Scheme::new();

        assert_eq!(
            scheme.origin.serve("axiomd://assets/../../../etc/passwd"),
            Served::Missing,
        );
        assert_eq!(
            scheme.origin.serve("axiomd://assets/anything.js"),
            Served::Missing,
        );
    }

    #[test]
    fn answers_for_no_scheme_but_its_own() {
        let scheme = Scheme::new();

        assert_eq!(scheme.origin.serve("file:///etc/passwd"), Served::Refused,);
        assert_eq!(
            scheme.origin.serve("https://example.com/tracker.png"),
            Served::Refused,
        );
    }

    /// One window's document must not become reachable from another's URI space:
    /// each publication is its own host over its own directory.
    #[test]
    fn documents_do_not_share_pages_or_directories() {
        let first_dir = ScratchDir::new("scheme-first");
        let second_dir = ScratchDir::new("scheme-second");
        let first_file = first_dir.write("a.md", "# A\n");
        let second_file = second_dir.write("b.md", "# B\n");
        first_dir.write("only-in-first.png", PIXEL_PNG);
        let scheme = Scheme::new();
        let first = scheme.publish(Some(&first_file));
        let second = scheme.publish(Some(&second_file));
        first.show("<h1>A</h1>".to_owned());
        second.show("<h1>B</h1>".to_owned());

        assert_ne!(first.uri(), second.uri());
        assert_eq!(
            served_bytes(scheme.origin.serve(first.uri())).0,
            b"<h1>A</h1>",
        );
        assert_eq!(
            served_bytes(scheme.origin.serve(second.uri())).0,
            b"<h1>B</h1>",
        );
        assert_eq!(
            scheme
                .origin
                .serve(&format!("{}only-in-first.png", second.uri())),
            Served::Missing,
        );
    }

    /// The cold-start budget is measured off this, so it has to start unanswered and
    /// stop on the page rather than on anything else the handler answers. A stylesheet
    /// or an image served first would otherwise report a startup that happened before
    /// the reader had a document.
    #[test]
    fn startup_is_the_moment_the_first_document_page_is_served() {
        let scratch = ScratchDir::new("scheme-startup");
        let file = scratch.write("notes.md", "# Notes\n");
        scratch.write("logo.png", PIXEL_PNG);
        let scheme = Scheme::new();
        let publication = scheme.publish(Some(&file));
        publication.show("<h1>Notes</h1>".to_owned());

        assert_eq!(scheme.startup(), None, "startup was over before it began");

        scheme.origin.serve(axiomd_render::STYLESHEET_URI);
        scheme
            .origin
            .serve(&format!("{}logo.png", publication.uri()));
        assert_eq!(
            scheme.startup(),
            None,
            "an asset the reader cannot read counted as their document",
        );

        scheme.origin.serve(publication.uri());
        let first = scheme.startup().expect("the document was served");
        assert!(first > Duration::ZERO, "no time at all passed");

        // A second window, a re-render, a document opened an hour later: none of them
        // are this process starting up, and none of them may move the number.
        let second = scheme.publish(Some(&file));
        second.show("<h1>Other</h1>".to_owned());
        scheme.origin.serve(second.uri());
        assert_eq!(scheme.startup(), Some(first), "a later page moved startup");
    }

    /// Nothing is served for a document whose page has not been rendered yet, so
    /// nothing may claim startup is over either.
    #[test]
    fn startup_is_not_over_while_a_document_has_no_page() {
        let scratch = ScratchDir::new("scheme-startup-blank");
        let file = scratch.write("notes.md", "# Notes\n");
        let scheme = Scheme::new();
        let publication = scheme.publish(Some(&file));

        assert_eq!(scheme.origin.serve(publication.uri()), Served::Missing);

        assert_eq!(scheme.startup(), None);
    }

    /// Closing a window drops its publication; nothing it could serve survives.
    #[test]
    fn a_closed_documents_page_and_files_stop_being_reachable() {
        let scratch = ScratchDir::new("scheme-closed");
        let file = scratch.write("notes.md", "# Notes\n");
        scratch.write("logo.png", PIXEL_PNG);
        let scheme = Scheme::new();
        let publication = scheme.publish(Some(&file));
        publication.show("<h1>Notes</h1>".to_owned());
        let page = publication.uri().to_owned();
        let image = format!("{page}logo.png");
        let loaded = publication.attach_image(PIXEL_PNG.to_vec(), "image/png".to_owned());

        drop(publication);

        assert_eq!(scheme.origin.serve(&page), Served::Missing);
        assert_eq!(scheme.origin.serve(&image), Served::Missing);
        assert_eq!(scheme.origin.serve(&loaded), Served::Missing);
        assert!(scheme.origin.documents.borrow().is_empty());
    }

    /// An image the reader asked for comes back through the document's own origin,
    /// which is what lets it be displayed without loosening the page's policy.
    #[test]
    fn an_image_the_reader_loaded_is_served_from_the_documents_own_origin() {
        let scratch = ScratchDir::new("scheme-loaded");
        let file = scratch.write("notes.md", "# Notes\n");
        let scheme = Scheme::new();
        let publication = scheme.publish(Some(&file));

        let first = publication.attach_image(PIXEL_PNG.to_vec(), "image/png".to_owned());
        let second = publication.attach_image(b"GIF89a".to_vec(), "image/gif".to_owned());

        assert!(first.starts_with("axiomd://img-"), "{first}");
        assert_ne!(first, second, "two images shared one URI");
        assert_eq!(
            served_bytes(scheme.origin.serve(&first)),
            (PIXEL_PNG.to_vec(), "image/png".to_owned()),
        );
        assert_eq!(
            served_bytes(scheme.origin.serve(&second)),
            (b"GIF89a".to_vec(), "image/gif".to_owned()),
        );
        assert_eq!(
            scheme.origin.serve(&format!("{first}9")),
            Served::Missing,
            "an image nobody loaded was answered",
        );
    }

    /// One window's loaded image is not another window's, however the URI is spelled.
    #[test]
    fn a_loaded_image_belongs_to_the_document_that_loaded_it() {
        let first_dir = ScratchDir::new("scheme-img-first");
        let second_dir = ScratchDir::new("scheme-img-second");
        let scheme = Scheme::new();
        let first = scheme.publish(Some(&first_dir.write("a.md", "# A\n")));
        let second = scheme.publish(Some(&second_dir.write("b.md", "# B\n")));

        let mine = first.attach_image(PIXEL_PNG.to_vec(), "image/png".to_owned());
        let theirs = second.attach_image(b"GIF89a".to_vec(), "image/gif".to_owned());

        assert_ne!(mine, theirs);
        drop(first);
        assert_eq!(scheme.origin.serve(&mine), Served::Missing);
        assert_eq!(
            served_bytes(scheme.origin.serve(&theirs)).0,
            b"GIF89a",
            "closing one window took another window's image with it",
        );
    }
}
