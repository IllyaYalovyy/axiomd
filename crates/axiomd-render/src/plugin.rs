//! The plugin layer: rendering capabilities beyond the core, each one optional.
//!
//! Core rendering — CommonMark and GFM, tables and images included — is never a
//! plugin (`design_decisions.md`). Everything on top of it is: a plugin claims code
//! fences, rewrites the typed event stream, decorates the finished markup, and carries
//! the styling its output needs. A [`Plugins`] registry is handed to [`render`] and is
//! the whole of what the pipeline knows about them.
//!
//! [`render`]: crate::render
//!
//! # What this module guarantees, so a plugin cannot break it
//!
//! * **A switched-off plugin costs nothing.** It is not in the registry, so it is
//!   never called, its fences fall back to ordinary code blocks, and no asset of its
//!   reaches the document. A document rendered with every plugin off is byte for byte
//!   the document a build with no plugin layer at all would produce.
//! * **A failing plugin loses only its own block.** Whether it answers with an error
//!   or panics outright, the block it claimed is rendered as the source it was, with
//!   an inline badge saying which plugin could not draw it (`ux_decisions.md`: never a
//!   dialog). The rest of the document, the other plugins and the application are
//!   untouched.
//! * **Source spans survive.** A transform cannot move a block: its replacement events
//!   are written at the span of the event it replaced. A post-render hook cannot lose
//!   an anchor: markup that no longer carries every `data-line` the document had is
//!   refused and the plugin's decoration dropped. Outline navigation, scroll sync,
//!   search and live reload all ride on those (invariant 3), so they are enforced here
//!   rather than asked for in prose.
//! * **A plugin is no more privileged than a document.** Its markup goes through the
//!   same sanitiser as the document's own and the page keeps the same content-security
//!   policy, so a plugin can no more run a script or fetch from the network than the
//!   markdown could. A capability that has to *draw* — a diagram is a picture, not
//!   markup — carries a script the application runs beside the document rather than
//!   inside it: the page is still displayed with scripting off, its policy is not
//!   loosened by a word, and what runs is a file compiled into the application, on the
//!   documents that used it and no others.
//!
//! # Versioning
//!
//! [`PLUGIN_API`] is the version of the contract in this module. A plugin declares in
//! its [`Manifest`] the version it was written against, and a registry runs only the
//! plugins whose version it can honour — a plugin built against another one is left
//! out exactly as a switched-off one is, rather than being called through a contract
//! that has changed under it. The number changes when a hook's signature or meaning
//! changes; adding a hook with a default implementation does not change it.

mod emoji;
mod mermaid;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use axiomd_engine::Event;

use crate::Anchor;

/// The version of the plugin contract this build speaks.
pub const PLUGIN_API: u32 = 1;

/// The path every plugin asset is served under, below `axiomd://assets`.
const ASSET_PREFIX: &str = "/plugin/";

/// The content type of an asset the document links rather than merely names.
const STYLESHEET: &str = "text/css";

/// The content type of an asset the *app* runs for the document, in a JavaScript world
/// of its own.
///
/// A rendered document cannot run a script — scripting is off and its policy admits
/// none — and that does not change for a plugin. What a plugin with one of these asks
/// for is that the application run it *beside* the document, from the same world the
/// app patches and scrolls the page in, on the documents that used the plugin and no
/// others. The document is still inert; the app has simply been given something to do
/// with it.
const SCRIPT: &str = "text/javascript";

/// One file a plugin carries: styling for its output, the code that draws it, or
/// something either of those names — a font, an icon.
///
/// It is compiled into the application, which is what makes "rendering fetches
/// nothing" true of plugins too (`design_decisions.md`). It reaches a document only
/// when its plugin is switched on *and* that document used it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Asset {
    /// The file's name, unique within its plugin.
    pub name: &'static str,
    /// What the bytes are. `text/css` is linked into the document, `text/javascript`
    /// is run by the app beside it; anything else is served for one of those to name.
    pub content_type: &'static str,
    /// The file itself.
    pub bytes: &'static [u8],
}

/// What a plugin is, as everything outside it needs to know: who it is, what it
/// claims, and what it carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Manifest {
    /// The [`PLUGIN_API`] version this plugin was written against.
    pub api: u32,
    /// The name the plugin is stored under — in the reader's settings, and in the URI
    /// of its assets. Stable once the plugin ships.
    pub id: &'static str,
    /// What the reader calls it, in preferences.
    pub name: &'static str,
    /// What it does for them, in one line, in preferences.
    pub description: &'static str,
    /// The code-fence languages this plugin draws instead of highlighting. A language
    /// claimed by two plugins belongs to the first of them in the registry.
    pub fences: &'static [&'static str],
    /// The files its output needs.
    pub assets: &'static [Asset],
}

/// One optional rendering capability.
///
/// Every hook has a default that does nothing, so a plugin implements only the ones it
/// is: an event transform overrides [`rewrite`], a fence renderer names its languages
/// in the manifest and overrides [`fence`], and a decorator overrides [`decorate`].
///
/// Implementations run on the render worker, so they must be usable from any thread,
/// and they are pure: a hook that did I/O would put a fetch or a disk read on the
/// rendering path, which no plugin is allowed (`design_decisions.md`).
///
/// [`rewrite`]: Plugin::rewrite
/// [`fence`]: Plugin::fence
/// [`decorate`]: Plugin::decorate
pub trait Plugin: Send + Sync {
    /// Who this plugin is.
    fn manifest(&self) -> &'static Manifest;

    /// Rewrites one event of the document, or answers `None` to leave it alone.
    ///
    /// The replacement is written where the original was, so a transform cannot move
    /// a block or invalidate a source span however it rewrites the stream. It is never
    /// offered the contents of a code block: code is not prose, and a fence is the
    /// [`fence`] hook's to claim.
    ///
    /// [`fence`]: Plugin::fence
    fn rewrite<'a>(&self, event: &Event<'a>) -> Option<Vec<Event<'a>>> {
        let _ = event;
        None
    }

    /// Draws a code fence in one of the languages the manifest claims, as the markup
    /// that replaces it — or says why it could not.
    ///
    /// The reason is shown to the reader on the badge beside the block, so it is a
    /// sentence about the document rather than an internal message.
    fn fence(&self, language: &str, source: &str) -> Result<String, String> {
        let _ = source;
        Err(format!("{language} is not a language this plugin draws"))
    }

    /// Decorates the finished document body, or answers `None` to leave it as it is.
    ///
    /// `anchors` is the source map of the markup being decorated, in document order.
    /// Markup that drops one of them is refused: too much of the application reads
    /// that map for a decoration to be worth losing it.
    fn decorate(&self, html: &str, anchors: &[Anchor]) -> Option<String> {
        let _ = (html, anchors);
        None
    }
}

/// The plugins one document is rendered with.
///
/// Cheap to build and to clone: it is built per render from the reader's settings, so
/// switching a plugin on or off applies to the very next render without anything being
/// reloaded or restarted (invariant 14).
#[derive(Clone, Default)]
pub struct Plugins {
    registered: Vec<Arc<dyn Plugin>>,
}

impl Plugins {
    /// Every plugin built into this application, minus the ids the reader has switched
    /// off.
    ///
    /// An unknown id is not an error: a plugin the reader switched off in an older
    /// version is simply not here to switch off, and the setting keeps it in case it
    /// comes back.
    pub fn builtin(disabled: &[String]) -> Self {
        Self::of(builtin().filter(|plugin| !disabled.iter().any(|off| off == plugin.manifest().id)))
    }

    /// A registry of exactly these plugins, in the order they are offered work.
    ///
    /// A plugin written against another [`PLUGIN_API`] version is left out, as is one
    /// whose id another plugin already has.
    pub fn of(plugins: impl IntoIterator<Item = Arc<dyn Plugin>>) -> Self {
        let mut registered: Vec<Arc<dyn Plugin>> = Vec::new();
        for plugin in plugins {
            let manifest = plugin.manifest();
            let known = registered
                .iter()
                .any(|other| other.manifest().id == manifest.id);
            if manifest.api == PLUGIN_API && !known {
                registered.push(plugin);
            }
        }
        Self { registered }
    }

    /// What is in this registry, in the order preferences lists it.
    pub fn manifests(&self) -> impl Iterator<Item = &'static Manifest> + '_ {
        self.registered.iter().map(|plugin| plugin.manifest())
    }

    /// The asset behind a request path under `axiomd://assets`, or `None` for a path
    /// that names no built-in plugin's file.
    ///
    /// It answers for the plugins compiled into the application rather than for one
    /// registry: a document only ever names an asset whose plugin was switched on and
    /// used when it was rendered, and the page it named it from is the one being
    /// displayed.
    pub fn asset(path: &str) -> Option<Asset> {
        let (id, name) = path.strip_prefix(ASSET_PREFIX)?.split_once('/')?;
        builtin()
            .map(|plugin| plugin.manifest())
            .find(|manifest| manifest.id == id)?
            .assets
            .iter()
            .find(|asset| asset.name == name)
            .copied()
    }

    /// The plugin that draws fences in `language`, if one does.
    pub(crate) fn claiming(&self, language: &str) -> Option<usize> {
        self.registered.iter().position(|plugin| {
            plugin
                .manifest()
                .fences
                .iter()
                .any(|claimed| claimed.eq_ignore_ascii_case(language))
        })
    }

    /// Asks the plugin at `at` to draw one fence. A plugin that panics has failed,
    /// like one that answered with a reason.
    pub(crate) fn fence(
        &self,
        at: usize,
        language: &str,
        source: &str,
        used: &mut Used,
    ) -> Result<String, String> {
        let plugin = &self.registered[at];
        let drawn = guard(|| plugin.fence(language, source))
            .unwrap_or_else(|| Err(format!("{} stopped unexpectedly", plugin.manifest().name)));
        if drawn.is_ok() {
            used.mark(at);
        }
        drawn
    }

    /// Runs one event past every plugin, each seeing what the ones before it made of
    /// it. `None` means nothing touched it, which is the common case and costs no
    /// allocation.
    pub(crate) fn rewrite<'a>(&self, event: &Event<'a>, used: &mut Used) -> Option<Vec<Event<'a>>> {
        let mut rewritten: Option<Vec<Event<'a>>> = None;
        for (at, plugin) in self.registered.iter().enumerate() {
            match &mut rewritten {
                None => {
                    if let Some(events) = guard(|| plugin.rewrite(event)).flatten() {
                        used.mark(at);
                        rewritten = Some(events);
                    }
                }
                Some(events) => {
                    let mut next = Vec::with_capacity(events.len());
                    for event in events.drain(..) {
                        match guard(|| plugin.rewrite(&event)).flatten() {
                            Some(replacement) => {
                                used.mark(at);
                                next.extend(replacement);
                            }
                            None => next.push(event),
                        }
                    }
                    *events = next;
                }
            }
        }
        rewritten
    }

    /// Hands the finished body to every plugin in turn, keeping each decoration that
    /// still carries the document's whole source map.
    pub(crate) fn decorate(&self, html: String, anchors: &[Anchor], used: &mut Used) -> String {
        let mut html = html;
        for (at, plugin) in self.registered.iter().enumerate() {
            let Some(decorated) = guard(|| plugin.decorate(&html, anchors)).flatten() else {
                continue;
            };
            if !keeps_the_anchors(&decorated, anchors) {
                continue;
            }
            used.mark(at);
            html = decorated;
        }
        html
    }

    /// The stylesheets the document must link, in registration order: the ones
    /// belonging to plugins that contributed to it.
    pub(crate) fn stylesheets(&self, used: &Used) -> Vec<(&'static str, Asset)> {
        self.carried(used, STYLESHEET)
    }

    /// The scripts the app must run for the document, in the order they are to be run:
    /// the ones belonging to plugins that contributed to it.
    pub(crate) fn scripts(&self, used: &Used) -> Vec<(&'static str, Asset)> {
        self.carried(used, SCRIPT)
    }

    /// The files of one kind belonging to the plugins that contributed to a document,
    /// in registration order and, within a plugin, in the order its manifest lists
    /// them. A plugin that drew nothing carries nothing: that is what makes a
    /// capability nobody used free rather than merely optional.
    fn carried(&self, used: &Used, content_type: &str) -> Vec<(&'static str, Asset)> {
        self.registered
            .iter()
            .enumerate()
            .filter(|(at, _)| used.was_used(*at))
            .flat_map(|(_, plugin)| {
                let manifest = plugin.manifest();
                manifest
                    .assets
                    .iter()
                    .filter(move |asset| asset.content_type == content_type)
                    .map(move |asset| (manifest.id, *asset))
            })
            .collect()
    }

    /// A registry with nothing in it: what the pipeline is handed when the reader has
    /// switched everything off.
    pub(crate) fn is_empty(&self) -> bool {
        self.registered.is_empty()
    }

    /// The name the reader knows the plugin at `at` by, for the badge on a block it
    /// could not draw.
    pub(crate) fn name_of(&self, at: usize) -> &'static str {
        self.registered[at].manifest().name
    }

    /// The id the plugin at `at` is stored under, which is also the class its blocks
    /// carry so its stylesheet can find them.
    pub(crate) fn id_of(&self, at: usize) -> &'static str {
        self.registered[at].manifest().id
    }

    /// A fresh record of what has contributed to one document.
    pub(crate) fn nothing_used(&self) -> Used {
        Used(vec![false; self.registered.len()])
    }
}

impl std::fmt::Debug for Plugins {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.manifests().map(|manifest| manifest.id))
            .finish()
    }
}

/// Which plugins have contributed to the document being rendered — which is exactly
/// the set whose assets it needs.
#[derive(Default)]
pub(crate) struct Used(Vec<bool>);

impl Used {
    fn mark(&mut self, at: usize) {
        self.0[at] = true;
    }

    fn was_used(&self, at: usize) -> bool {
        self.0[at]
    }
}

/// The URI a document links one plugin's stylesheet from.
pub(crate) fn asset_uri(id: &str, asset: &Asset) -> String {
    format!(
        "axiomd://assets{ASSET_PREFIX}{id}/{name}",
        name = asset.name
    )
}

/// Every plugin compiled into the application, in the order they are offered work and
/// listed in preferences.
fn builtin() -> impl Iterator<Item = Arc<dyn Plugin>> {
    [
        Arc::new(emoji::Emoji) as Arc<dyn Plugin>,
        Arc::new(mermaid::Mermaid) as Arc<dyn Plugin>,
    ]
    .into_iter()
}

/// Runs one hook, answering `None` if it panicked.
///
/// A plugin is not allowed to take the document, the other plugins or the application
/// with it (invariant 13), and an unwind here is caught for the same reason a returned
/// error is handled: the reader gets the block as source with a badge, and keeps
/// everything else.
fn guard<T>(hook: impl FnOnce() -> T) -> Option<T> {
    catch_unwind(AssertUnwindSafe(hook)).ok()
}

/// Whether decorated markup still carries every source line the document had.
///
/// The cheap half of the anchor contract: an element with that `data-line` is still
/// there to be scrolled to. Which element it is is the decorator's business; that
/// there is one is not.
fn keeps_the_anchors(html: &str, anchors: &[Anchor]) -> bool {
    anchors
        .iter()
        .all(|anchor| html.contains(&format!("data-line=\"{}\"", anchor.line)))
}
