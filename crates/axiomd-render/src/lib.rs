//! Rendering pipeline: engine events in, one sanitized HTML document out.
//!
//! The crate is pure — no I/O, no GTK, no network, no subprocess — so a render is
//! reproducible byte for byte and safe to run on a worker thread while the main loop
//! stays free.
//!
//! ```
//! use axiomd_engine::{ComrakEngine, Extensions, MarkdownEngine};
//!
//! let parsed = ComrakEngine::new().parse("# Title\n\nText.\n", Extensions::FULL);
//! let rendered = axiomd_render::render(&parsed);
//! assert!(rendered.html().contains("<h1 id=\"title\" data-line=\"1\">Title</h1>"));
//! // The blocks alone, for patching them into a document that is already on screen.
//! assert!(rendered.body().starts_with("<h1 id=\"title\" data-line=\"1\">Title</h1>"));
//! // One anchor per top-level block, each carrying the source it came from.
//! assert_eq!(rendered.anchors().len(), 2);
//! assert_eq!(rendered.anchors()[1].line, 3);
//! ```
//!
//! # What the document guarantees
//!
//! * **Anchored.** Every top-level block carries `data-line`, and [`Rendered::anchors`]
//!   is the same map as typed data. Scroll sync, outline tracking, search and
//!   live-reload position preservation read it rather than measuring heights.
//! * **Inert.** The body is sanitized with ammonia after templating and the document
//!   declares a strict CSP: no script, no plugin, no frame, and images only from the
//!   app's own `axiomd:` scheme. A malicious document renders as text.
//! * **Offline.** Nothing the pipeline emits can cause a fetch. A remote image is a
//!   placeholder card that *is* its own load button, and the only thing it can do is
//!   ask the app — through a [`Request`] the reader clicked — to go and get it.
//! * **Linkable.** Every heading carries the anchor id GitHub would give it, so
//!   `guide.md#getting-started` written anywhere lands on the same section here.
//! * **Themed by CSS alone.** Colours — including the code palettes — live in
//!   [`stylesheet`], in a light block and a `prefers-color-scheme: dark` block, so
//!   switching theme restyles a rendered document without re-parsing it.

#![deny(missing_docs)]

mod body;
mod highlight;
mod request;
mod sanitize;
mod slug;

use std::ops::Range;
use std::sync::OnceLock;

use axiomd_engine::Parsed;

pub use request::Request;

/// Where the rendered document loads [`stylesheet`] from. The app serves this URI
/// from its own scheme handler; nothing else in the document is fetchable.
pub const STYLESHEET_URI: &str = "axiomd://assets/axiomd.css";

/// The policy the rendered document is displayed under: no script, no plugins, no
/// frames, no form submission, and images and styles only from the app's own scheme.
const CONTENT_SECURITY_POLICY: &str =
    "default-src 'none'; img-src axiomd:; style-src axiomd:; base-uri 'none'; form-action 'none'";

/// A render happens on a worker thread and its result is handed to the main loop,
/// so the document must be able to cross a thread boundary.
const _: () = {
    const fn crosses_threads<T: Send>() {}
    crosses_threads::<Rendered>();
};

/// A rendered document: the HTML to display, and the map from it back to the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    html: String,
    /// Where [`Rendered::body`] lies inside `html`, rather than a second copy of it:
    /// a document is held once, however many ways it is asked for.
    body: Range<usize>,
    anchors: Vec<Anchor>,
    remote_images: Vec<String>,
}

impl Rendered {
    /// The complete HTML document.
    pub fn html(&self) -> &str {
        &self.html
    }

    /// The document's blocks alone — what lies inside the `<article>` of [`html`].
    ///
    /// This is what a view already showing an earlier render of the same document
    /// patches in: replacing the blocks that changed costs the reader neither their
    /// place nor a page load, where re-navigating to [`html`] costs both.
    ///
    /// [`html`]: Rendered::html
    pub fn body(&self) -> &str {
        &self.html[self.body.clone()]
    }

    /// One entry per top-level block, in document order, with strictly increasing
    /// lines. The block rendered from `anchors()[i]` is the element whose
    /// `data-line` attribute equals `anchors()[i].line`.
    pub fn anchors(&self) -> &[Anchor] {
        &self.anchors
    }

    /// The source of every remote image in the document, in document order, each
    /// standing behind a placeholder card the reader has not pressed.
    ///
    /// This is what "load all" is a list of. It comes from the parse rather than
    /// from the page on screen, so it is the same list however the document has been
    /// patched since.
    pub fn remote_images(&self) -> &[String] {
        &self.remote_images
    }
}

/// Where one rendered block came from in the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchor {
    /// 1-based source line, and the value of the block's `data-line` attribute.
    pub line: u32,
    /// Byte range of the block in the source the parse was handed.
    pub source: Range<usize>,
}

/// Renders a parsed document.
pub fn render(parsed: &Parsed<'_>) -> Rendered {
    let (body, anchors, remote_images) = body::render(parsed);
    let body = sanitize::clean(&body);
    let mut html = format!(
        "<!DOCTYPE html>\n\
         <html>\n\
         <head>\n\
         <meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <meta http-equiv=\"Content-Security-Policy\" content=\"{CONTENT_SECURITY_POLICY}\">\n\
         <link rel=\"stylesheet\" href=\"{STYLESHEET_URI}\">\n\
         </head>\n\
         <body>\n\
         <article class=\"markdown\">\n"
    );
    let start = html.len();
    html.push_str(&body);
    let body = start..html.len();
    html.push_str("</article>\n</body>\n</html>\n");
    Rendered {
        html,
        body,
        anchors,
        remote_images,
    }
}

/// The stylesheet that puts the reader's own layout choices over [`stylesheet`].
///
/// `reading_width` is the measure a document's text is held to, in rem, or `None` for
/// a document that fills the window.
///
/// It is meant to be installed as a *user* stylesheet on the view rather than folded
/// into the document, which is what makes a change to it free: the page on screen
/// restyles in place, and nothing is re-parsed, re-rendered or reloaded to change how
/// wide a document is. `!important` is not decoration — under the cascade (CSS
/// Cascading and Inheritance Level 5, §6.2) a normal user declaration loses to the
/// document's own author one, and only an important user declaration outranks it.
pub fn reader_stylesheet(reading_width: Option<u32>) -> String {
    let measure = match reading_width {
        Some(rem) => format!("{rem}rem"),
        None => "none".to_owned(),
    };
    format!(":root {{ --axiomd-reading-width: {measure} !important; }}\n")
}

/// The default stylesheet, light and dark palettes included.
///
/// It is built once per process: the document typography plus the two code palettes
/// generated from the bundled syntect themes, so the classes the highlighter emits
/// and the classes the stylesheet defines cannot drift apart.
pub fn stylesheet() -> &'static str {
    static STYLESHEET: OnceLock<String> = OnceLock::new();
    STYLESHEET.get_or_init(|| {
        format!(
            "{}\n{}",
            include_str!("../assets/axiomd.css"),
            highlight::palettes()
        )
    })
}
