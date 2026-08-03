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
//! let plugins = axiomd_render::Plugins::builtin(&[]);
//! // What lies beside the document, which is the whole of what a `[[wikilink]]` in it
//! // can reach. Nothing, here: this document is alone.
//! let beside = axiomd_render::Folder::empty();
//! let rendered = axiomd_render::render(&parsed, "notes", &plugins, &beside);
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
//!   [`stylesheet`], in a light block and a screen-only `prefers-color-scheme: dark`
//!   block, so switching theme restyles a rendered document without re-parsing it,
//!   and neither paper nor an exported file ever goes dark. What the reader's own
//!   desktop asks for on top of that — a measure, high contrast — is a second
//!   stylesheet ([`reader_stylesheet`]) the app installs over the first, for the same
//!   reason and at the same cost.
//! * **Extensible without being weakened.** Everything beyond core CommonMark and GFM
//!   is an optional [`Plugin`] the reader can switch off: it claims fences, rewrites
//!   events and decorates markup through [`Plugins`], and it is held to every rule
//!   above — its output is sanitised with the document's, its styling is bundled, and
//!   a plugin that fails loses its own block and nothing else. One that has to *draw*
//!   rather than write markup carries the code that draws it ([`Rendered::scripts`]),
//!   which the app runs beside the document and never inside it: the page stays as
//!   inert as this list says it is.
//! * **Portable.** The same parse becomes a page that needs the app
//!   ([`render`]) or one that needs nothing at all ([`standalone`]): styling inlined,
//!   pictures carried inside it, and not one reference that would be fetched when
//!   somebody opens it.

#![deny(missing_docs)]

mod body;
mod callout;
mod footnote;
mod highlight;
mod meta;
mod plugin;
mod request;
mod sanitize;
mod slug;
mod wikilink;

use std::ops::Range;
use std::sync::OnceLock;

use axiomd_engine::Parsed;

pub use plugin::{Asset, Manifest, PLUGIN_API, Plugin, Plugins};
pub use request::Request;
pub use wikilink::Folder;

/// The bundled file served at `path` under `axiomd://assets`, or `None` for a path
/// that names none.
///
/// Every byte a rendered document can reach that is not the document itself: the
/// icons a callout is drawn with, and the files of the plugins compiled into the
/// application. It answers from a table compiled into the binary, so a request can
/// neither miss nor escape onto the reader's filesystem, and it is the one place that
/// knows what an `axiomd://assets` path means — the app's scheme handler asks rather
/// than keeping a second list that could drift from this one.
pub fn asset(path: &str) -> Option<Asset> {
    callout::asset(path).or_else(|| plugin::asset(path))
}

/// Where the rendered document loads [`stylesheet`] from. The app serves this URI
/// from its own scheme handler; nothing else in the document is fetchable.
pub const STYLESHEET_URI: &str = "axiomd://assets/axiomd.css";

/// The policy the rendered document is displayed under: no script, no plugins, no
/// frames, no form submission, and images, styles and fonts only from the app's own
/// scheme.
///
/// `font-src` is not a loosening: without it a face would fall back to `default-src
/// 'none'` and be refused, and every face this scheme can answer for is a file
/// compiled into the application (`plugin::asset`). It is here so that a capability
/// may carry the typography its output is unreadable without — mathematics is the
/// first — while the page stays a page that can fetch nothing.
const CONTENT_SECURITY_POLICY: &str = "default-src 'none'; img-src axiomd:; style-src axiomd:; font-src axiomd:; \
     base-uri 'none'; form-action 'none'";

/// The same policy for a document that has left axiomd: everything it needs is inside
/// it, so the only picture it may show is one it carries and the only styling it may
/// use is the one written into it. A browser enforces this, which makes "an exported
/// document fetches nothing" true of the file rather than only of the code that wrote
/// it.
const EXPORTED_SECURITY_POLICY: &str = "default-src 'none'; img-src data:; font-src data:; style-src 'unsafe-inline'; \
     base-uri 'none'; form-action 'none'";

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
    outline: Vec<Heading>,
    remote_images: Vec<String>,
    stylesheets: Vec<String>,
    scripts: Vec<String>,
}

impl Rendered {
    /// The complete HTML document.
    pub fn html(&self) -> &str {
        &self.html
    }

    /// The styling this document needs beyond the bundled stylesheet: one URI per
    /// plugin that contributed to it, in registration order, already linked in the head
    /// of [`html`].
    ///
    /// It travels beside [`body`] rather than inside it because a view that patches a
    /// document it is already showing replaces the blocks and not the head — and a
    /// capability switched on or off between two renders changes exactly this list.
    ///
    /// [`html`]: Rendered::html
    /// [`body`]: Rendered::body
    pub fn stylesheets(&self) -> &[String] {
        &self.stylesheets
    }

    /// The code this document needs the app to run for it, in the order it is to be
    /// run: one URI per script of a plugin that contributed to the document, and
    /// nothing at all for a document no drawing plugin contributed to.
    ///
    /// It is *not* linked from [`html`], and a document could not run it if it were:
    /// the page is displayed with scripting off and its policy admits no script. These
    /// are files for the application to run beside the document, in the JavaScript
    /// world it already patches and scrolls the page from — a diagram is a picture, and
    /// something has to draw it. Each is named by the `axiomd://` URI the app's own
    /// scheme answers for, which is both where the bytes are and what says two renders
    /// mean the same file.
    ///
    /// [`html`]: Rendered::html
    pub fn scripts(&self) -> &[String] {
        &self.scripts
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

    /// The document's headings, in document order — what the outline sidebar shows.
    ///
    /// It is the part of [`anchors`] that happens to be a heading rather than a second
    /// reading of the document, so every entry's `line` is a line [`anchors`] has and
    /// the block carrying that `data-line` is the section it names. A heading nested
    /// inside a container is not here: it has no anchor, so there is nowhere to send a
    /// reader who clicks it.
    ///
    /// [`anchors`]: Rendered::anchors
    pub fn outline(&self) -> &[Heading] {
        &self.outline
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

/// One picture, as a document that carries its own pictures needs it.
///
/// What [`standalone`] is answered with for each picture a document names: the file's
/// bytes and what they are. Nothing else about the file travels — not its path, not
/// its name — because nothing else survives being written into the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    /// The bytes of the picture itself.
    pub bytes: Vec<u8>,
    /// What those bytes are, as a content type: `image/png` and the like.
    pub content_type: String,
}

/// One heading of the document, as the outline names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    /// 1 to 6, as the document wrote it — what the entry is nested under.
    pub level: u8,
    /// The heading's words, with its markup read out and collapsed to one line.
    pub text: String,
    /// The source line of the heading's block, and the `data-line` of the element it
    /// was rendered as. This is what clicking the entry scrolls to.
    pub line: u32,
}

/// Where one rendered block came from in the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchor {
    /// 1-based source line, and the value of the block's `data-line` attribute.
    pub line: u32,
    /// Byte range of the block in the source the parse was handed.
    pub source: Range<usize>,
}

/// Renders a parsed document for the window.
///
/// `name` is what the reader calls the file, and is used only when the document
/// gives no title of its own — in frontmatter or in its first heading. The title
/// matters beyond the window: printing this page names the job with it, and a PDF
/// made from it carries it as metadata.
pub fn render(parsed: &Parsed<'_>, name: &str, plugins: &Plugins, beside: &Folder) -> Rendered {
    let rendered = body::render(
        parsed,
        &body::Page {
            to: body::Destination::Screen,
            plugins,
            beside,
        },
    );
    let body = sanitize::clean(&rendered.markup);
    let stylesheets: Vec<String> = rendered
        .stylesheets
        .iter()
        .map(|(id, asset)| plugin::asset_uri(id, asset))
        .collect();
    let mut html = format!(
        "<!DOCTYPE html>\n\
         <html>\n\
         <head>\n\
         <meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <meta http-equiv=\"Content-Security-Policy\" content=\"{CONTENT_SECURITY_POLICY}\">\n\
         <title>{title}</title>\n\
         <link rel=\"stylesheet\" href=\"{STYLESHEET_URI}\">\n\
         {plugin_stylesheets}</head>\n\
         <body>\n\
         <article class=\"markdown\">\n",
        title = body::escape_text(&meta::title(parsed, name)),
        plugin_stylesheets = stylesheets
            .iter()
            .map(|uri| format!("<link rel=\"stylesheet\" href=\"{uri}\">\n"))
            .collect::<String>(),
    );
    let start = html.len();
    html.push_str(&body);
    let body = start..html.len();
    html.push_str("</article>\n</body>\n</html>\n");
    Rendered {
        html,
        body,
        anchors: rendered.anchors,
        outline: rendered.outline,
        remote_images: rendered.remote_images,
        stylesheets,
        scripts: rendered
            .scripts
            .iter()
            .map(|(id, asset)| plugin::asset_uri(id, asset))
            .collect(),
    }
}

/// The same document as one file that needs nothing else — no app, no network, no
/// folder of assets beside it.
///
/// The styling is inlined, every picture the document names is carried inside it, and
/// nothing that only axiomd could answer survives: a remote image is a card that says
/// what is missing rather than a button nobody can press. It is light whoever opens
/// it (owner ruling, 2026-08-02).
///
/// `embed` is asked for each picture the document names relative to itself, and
/// answers with a [`Picture`] — or `None`, for one that cannot be carried and is
/// shown as missing instead. It is the only way anything gets into the file, which is
/// what makes "this document fetches nothing" a property of the pipeline rather than
/// a promise about the caller.
pub fn standalone(
    parsed: &Parsed<'_>,
    name: &str,
    plugins: &Plugins,
    beside: &Folder,
    embed: &dyn Fn(&str) -> Option<Picture>,
) -> String {
    let rendered = body::render(
        parsed,
        &body::Page {
            to: body::Destination::File(embed),
            plugins,
            beside,
        },
    );
    let body = sanitize::clean_for_a_file(&rendered.markup);
    // A plugin's styling travels inside the file like everything else the document
    // needs: an exported document names nothing, so it cannot link an asset the app
    // would have answered for. Its *code* does not travel at all — an exported
    // document is read where there is no axiomd to run it, and a file that carried a
    // script would be a file that runs one. What a drawing plugin leaves in an
    // exported document is the source the author wrote.
    let plugin_stylesheets: String = rendered
        .stylesheets
        .iter()
        .map(|(_, asset)| plugin::carried_inside(&String::from_utf8_lossy(asset.bytes)))
        .collect();
    format!(
        "<!DOCTYPE html>\n\
         <html>\n\
         <head>\n\
         <meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <meta http-equiv=\"Content-Security-Policy\" content=\"{EXPORTED_SECURITY_POLICY}\">\n\
         <title>{title}</title>\n\
         <style>\n{stylesheet}\n{plugin_stylesheets}</style>\n\
         </head>\n\
         <body>\n\
         <article class=\"markdown\">\n\
         {body}</article>\n\
         </body>\n\
         </html>\n",
        title = body::escape_text(&meta::title(parsed, name)),
        stylesheet = exported_stylesheet(),
    )
}

/// The stylesheet an exported document carries: the light document, the light code
/// palette, and a declaration that light is what it is — so a browser in dark mode
/// renders the page the reader exported rather than a dark one they never saw.
fn exported_stylesheet() -> &'static str {
    static STYLESHEET: OnceLock<String> = OnceLock::new();
    STYLESHEET.get_or_init(|| {
        format!(
            "{}\n{}\n{}\n:root {{ color-scheme: light; }}\n",
            include_str!("../assets/axiomd.css"),
            // A file that leaves axiomd names nothing axiomd would have to answer
            // for, so its callout icons travel inside it as the bytes they are.
            callout::icon_styling(&|asset| {
                format!(
                    "data:{};base64,{}",
                    asset.content_type,
                    body::base64(asset.bytes)
                )
            }),
            highlight::light_palette(),
        )
    })
}

/// How much contrast the reader is reading at — the desktop's accessibility answer,
/// not a preference of axiomd's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Contrast {
    /// The palette the document defines, light or dark.
    Normal,
    /// The desktop is asking for high contrast: full-strength ink, borders that are
    /// there rather than suggested, and links told apart by more than their colour.
    High,
}

/// The stylesheet that puts the reader's own way of reading over [`stylesheet`].
///
/// `reading_width` is the measure a document's text is held to, in rem, or `None` for
/// a document that fills the window; `contrast` is what the desktop's accessibility
/// setting asks for.
///
/// It is meant to be installed as a *user* stylesheet on the view rather than folded
/// into the document, which is what makes a change to it free: the page on screen
/// restyles in place, and nothing is re-parsed, re-rendered or reloaded to change how
/// wide a document is or how much contrast it has. `!important` is not decoration —
/// under the cascade (CSS Cascading and Inheritance Level 5, §6.2) a normal user
/// declaration loses to the document's own author one, and only an important user
/// declaration outranks it. That is also why high contrast is written here rather
/// than as a `prefers-contrast` block in the document's own stylesheet: WebKitGTK
/// does not answer `prefers-contrast` from the application's style manager at all
/// (probed on WebKitGTK 2.52.5 and libadwaita 1.8.6 — the media query stayed false
/// with the desktop in high contrast), so the application has to say it.
pub fn reader_stylesheet(reading_width: Option<u32>, contrast: Contrast) -> String {
    let measure = match reading_width {
        Some(rem) => format!("{rem}rem"),
        None => "none".to_owned(),
    };
    let mut sheet = format!(":root {{ --axiomd-reading-width: {measure} !important; }}\n");
    if contrast == Contrast::High {
        sheet.push_str(include_str!("../assets/high-contrast.css"));
    }
    sheet
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
            "{}\n{}\n{}\n{}",
            include_str!("../assets/axiomd.css"),
            include_str!("../assets/dark.css"),
            // On screen an icon is a file the app's own scheme answers for, which is
            // what keeps the document's policy to `axiomd:` and nothing else.
            callout::icon_styling(&callout::icon_uri),
            highlight::palettes()
        )
    })
}
