//! The surface a document is displayed on, and the one rule it keeps: a document is
//! loaded once.
//!
//! The first render of a document is a page load. Every render after it — the file
//! changed on disk, the buffer changed in the editor — is patched into the page that
//! is already there ([`patch.js`](../src/patch.js)), because navigating the view again
//! is a full-page reload: the window flashes, images are fetched again, and the reader
//! loses their place. That is Apostrophe's defining bug, reproduced by design if a
//! re-render ever calls `load_uri` or `load_html` again, and [`DocumentView::show`] is
//! the only place that could.
//!
//! # What the webview is allowed to do
//!
//! Almost nothing. [`document_settings`] turns off JavaScript, media capture, storage
//! and every other capability a reading surface has no use for, and the policy below
//! keeps the view on the document it was given, so nothing a document contains can
//! make the app fetch anything by itself. Together with the rendered document's own
//! content-security policy and the sanitiser upstream, the `axiomd://` scheme is the
//! complete set of bytes a document can reach. There is no `file://` grant anywhere.
//!
//! # Patching a document that cannot run scripts
//!
//! The patch is evaluated in a JavaScript world of the app's own ([`WORLD`]), which is
//! what lets the document keep every restriction while still being updated: on
//! WebKitGTK 2.52.5 a document rendered under `enable-javascript = false` and
//! `default-src 'none'` refuses main-world evaluation with "Cannot execute JavaScript
//! in this document", while a named world reaches the same DOM. The test-control
//! channel reads documents the same way (`control.rs`), and the live-reload e2e suite
//! asserts both halves of the result: the document changes, and a `<script>` inside it
//! still cannot run.
//!
//! # Where a click goes
//!
//! A document cannot run a script, so every link in it arrives here as a navigation
//! the view is about to make, and none of them are its to make: [`crate::links`]
//! decides what each one is and the window does it. All the view keeps is the one
//! thing only it knows — which document it is showing — and the state that belongs to
//! the page rather than to the file: the remote images the reader has asked for, and
//! which of those did not arrive.
//!
//! # Searching what is on screen
//!
//! The view is one of the two surfaces the search bar drives ([`crate::find`]), and the
//! only one whose marks a render can destroy: they stand inside the very blocks the
//! patch replaces. So the search is held here rather than in the bar — taken off before
//! the patch and put back after it, in the same task — and it is the patch's ordering
//! that makes that safe rather than luck.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use axiomd_render::Rendered;
use gtk::glib;
use webkit6::prelude::*;

use crate::find::{Counted, Query, Searchable};
use crate::links::{Follow, follow};
use crate::scheme::Publication;

/// The JavaScript world the app updates a document from — never the document's own.
const WORLD: &str = "axiomd-view";

/// The patch: new blocks in, changed blocks replaced, the reader left where they were.
const PATCH: &str = include_str!("patch.js");

/// The remote images the reader has asked for, put back into the page.
const REMOTE: &str = include_str!("remote.js");

/// The two halves of the span map as the rendered page sees it: which block the reader
/// is looking at, and putting a source line back at the top of the page.
const PLACE: &str = include_str!("place.js");

/// The page saying which section the reader is in, once per frame in which that can
/// have changed.
const TRACK: &str = include_str!("track.js");

/// The reader's search, marked over the words of the page.
const FIND: &str = include_str!("find.js");

/// The message handler the page reports the reader's section to. Registered in
/// [`WORLD`] and nowhere else, so a document — which cannot run a script in the first
/// place — has nothing to post to even if it could.
const SECTION_MESSAGE: &str = "axiomdSection";

/// One window's view of one document at a time.
pub(crate) struct DocumentView {
    webview: webkit6::WebView,
    /// The document URI the view has been sent to. A render for this URI is patched
    /// in; a render for another document is a different page, and is loaded.
    loaded: RefCell<Option<String>>,
    /// Whether that load has finished, so that there is a document to patch.
    ready: Cell<bool>,
    /// A render that arrived before the first load finished, applied as soon as it
    /// does. Without this a document that changes while it is still opening would
    /// either be lost or cost a second load.
    pending: RefCell<Option<String>>,
    /// How many times this view has committed a load.
    ///
    /// The number a re-render must not move: showing a changed document by navigating
    /// the view is the full-page reload that costs the user their place and flashes
    /// the window (`design_decisions.md`). Counted here because only the view knows,
    /// and read back through the test-control channel.
    navigations: Cell<u32>,
    /// The directory the document on screen lives in — the whole of what its links
    /// may reach. `None` for an untitled document, which is nowhere and reaches
    /// nothing.
    root: RefCell<Option<PathBuf>>,
    /// The source line to bring to the top of the page once it is next up to date.
    ///
    /// Switching out of edit mode is the one thing that has to happen *after* a
    /// render: scrolling the page the reader is leaving would land on blocks the
    /// document no longer has. Held here so that the patch and the scroll are one
    /// task rather than two racing ones.
    place: Cell<Option<u32>>,
    /// What the window does about a link the view will not follow itself.
    followed: RefCell<Option<Followed>>,
    /// What the window does with the reader's section when the page reports it.
    sectioned: RefCell<Option<Sectioned>>,
    /// How many times the page has reported which section the reader is in.
    ///
    /// The bridge's whole promise is that scrolling costs at most one message per
    /// frame — a jump across a whole document is one message, not one per section it
    /// passed — and this is the only thing that promise is visible in. Counted here
    /// because only the view knows, and read back through the test-control channel as
    /// the navigation count is.
    reports: Cell<u32>,
    /// The remote images of the document on screen: every one the reader has asked
    /// for, and every one that did not arrive. Cleared when the view is sent to
    /// another document, because it describes this page and no other.
    images: RefCell<RemoteImages>,
    /// The search the page is showing, or `None` when the reader is not searching.
    ///
    /// Held by the view rather than by the search bar because a render is what
    /// invalidates it: the marks live in the blocks the patch is about to replace, so
    /// they come off before it and go back on after it, in the same task — which is
    /// the only place that ordering can be guaranteed. Shared rather than owned so
    /// that that task can reach it without holding the view alive.
    finding: Rc<RefCell<Option<Finding>>>,
}

/// A search the page is showing.
struct Finding {
    looking_for: Query,
    /// Which match is the current one, counting from zero in document order.
    nth: usize,
    /// Whether the marks for this query are in the page now. A render takes them out.
    marked: bool,
    counted: Counted,
}

/// The window's answer to a link the view refused, held so that a click can reach it.
type Followed = Rc<dyn Fn(Follow)>;

/// The window's answer to the page saying which section the reader is in.
type Sectioned = Rc<dyn Fn(u32)>;

/// The remote images of one page, as the reader has left them.
#[derive(Default)]
struct RemoteImages {
    /// Every remote image in the document, in document order — what "load all" is a
    /// list of.
    sources: Vec<String>,
    /// Source URL to the `axiomd://` URI its bytes are served from.
    loaded: Vec<(String, String)>,
    /// Source URL to what to tell the reader beside its placeholder.
    failed: Vec<(String, String)>,
}

impl DocumentView {
    pub(crate) fn new(context: &webkit6::WebContext) -> Rc<Self> {
        // This window's own, so the reader's styling of one document cannot reach
        // another window's and the bridge below carries this window's place and no
        // other (invariant 7). It dies with the view.
        let content = webkit6::UserContentManager::new();
        let view = Rc::new(Self {
            webview: build_webview(context, &content),
            loaded: RefCell::new(None),
            ready: Cell::new(false),
            pending: RefCell::new(None),
            navigations: Cell::new(0),
            root: RefCell::new(None),
            place: Cell::new(None),
            followed: RefCell::new(None),
            sectioned: RefCell::new(None),
            reports: Cell::new(0),
            images: RefCell::new(RemoteImages::default()),
            finding: Rc::new(RefCell::new(None)),
        });

        // The one thing the page is allowed to say to the app, and it can only say it
        // from the app's own JavaScript world.
        content.register_script_message_handler(SECTION_MESSAGE, Some(WORLD));
        let reporting = Rc::downgrade(&view);
        content.connect_script_message_received(Some(SECTION_MESSAGE), move |_, reported| {
            let Some(view) = reporting.upgrade() else {
                return;
            };
            view.reports.set(view.reports.get() + 1);
            // Zero is a real answer: the reader is above the document's first heading,
            // which is not a section.
            let line = u32::try_from(reported.to_int32()).unwrap_or(0);
            let handler = view.sectioned.borrow().clone();
            if let Some(handler) = handler {
                handler(line);
            }
        });

        let watched = Rc::downgrade(&view);
        view.webview.connect_load_changed(move |_, event| {
            let Some(view) = watched.upgrade() else {
                return;
            };
            match event {
                webkit6::LoadEvent::Committed => {
                    view.navigations.set(view.navigations.get() + 1);
                }
                webkit6::LoadEvent::Finished => {
                    view.ready.set(true);
                    let waiting = view.pending.borrow_mut().take();
                    view.update(waiting.as_deref());
                }
                _ => {}
            }
        });

        // A document is not a browser tab: what it may do to the view is decided
        // here, and everything else it asks for is the window's to do or to refuse.
        let deciding = Rc::downgrade(&view);
        view.webview
            .connect_decide_policy(move |webview, decision, kind| match kind {
                webkit6::PolicyDecisionType::NavigationAction => {
                    let Some(view) = deciding.upgrade() else {
                        return false;
                    };
                    let Some(decision) =
                        decision.downcast_ref::<webkit6::NavigationPolicyDecision>()
                    else {
                        return false;
                    };
                    let action = decision.navigation_action();
                    let target = action
                        .as_ref()
                        .and_then(|action| action.request())
                        .and_then(|request| request.uri())
                        .unwrap_or_default();
                    // Only a link the reader activated may do anything but stay: a
                    // redirect, a reload or anything else the engine started is not
                    // somebody asking for it.
                    let clicked = action.as_ref().is_some_and(|action| {
                        action.navigation_type() == webkit6::NavigationType::LinkClicked
                    });
                    let here = webview.uri().unwrap_or_default();

                    match follow(&here, view.root.borrow().as_deref(), &target, clicked) {
                        Follow::Stay => decision.use_(),
                        elsewhere => {
                            decision.ignore();
                            let handler = view.followed.borrow().clone();
                            if let Some(handler) = handler {
                                handler(elsewhere);
                            }
                        }
                    }
                    true
                }
                webkit6::PolicyDecisionType::NewWindowAction => {
                    decision.ignore();
                    true
                }
                _ => false,
            });

        view
    }

    pub(crate) fn widget(&self) -> &webkit6::WebView {
        &self.webview
    }

    /// Hands `handler` everything the view will not do itself: another document, a
    /// file for the desktop, a link for the browser, a request the document is making.
    pub(crate) fn connect_follow(&self, handler: impl Fn(Follow) + 'static) {
        *self.followed.borrow_mut() = Some(Rc::new(handler));
    }

    /// How many loads this view has committed since it was built.
    pub(crate) fn navigations(&self) -> u32 {
        self.navigations.get()
    }

    /// Calls `handler` with the source line of the heading whose section the reader is
    /// in, whenever the page reports it: after every render, and whenever scrolling
    /// carries them into another one. Zero means they are above the first heading.
    pub(crate) fn connect_sectioned(&self, handler: impl Fn(u32) + 'static) {
        *self.sectioned.borrow_mut() = Some(Rc::new(handler));
    }

    /// How many times the page has reported the reader's section since this view was
    /// built.
    pub(crate) fn section_reports(&self) -> u32 {
        self.reports.get()
    }

    /// Glides the page to the block that source `line` belongs to — what picking a
    /// section in the outline does.
    ///
    /// Smooth, unlike every other way the page is placed: the reader asked to be moved,
    /// so they are shown the move. It is the same "block a line belongs to" rule
    /// everything else uses, stated once in `place.js`.
    pub(crate) fn scroll_to_line(&self, line: u32) {
        if !self.ready.get() {
            // The page is still arriving — the document's blocks are in the DOM but
            // its load has not finished, which is a window a reader can click in and
            // did. Their choice travels with the page instead of being dropped; there
            // is nothing to glide away from yet, so it simply lands there.
            self.place.set(Some(line));
            return;
        }
        let webview = self.webview.clone();
        glib::spawn_future_local(async move {
            if let Err(error) = webview
                .evaluate_javascript_future(
                    &format!("({PLACE}).scrollTo({line}, true)"),
                    Some(WORLD),
                    None,
                )
                .await
            {
                eprintln!("axiomd: the document could not be scrolled: {error}");
            }
        });
    }

    /// Restyles whatever this view is showing, and everything it shows afterwards,
    /// with `stylesheet`.
    ///
    /// How a preference the reader just changed reaches the document: as a *user*
    /// stylesheet on the view rather than as an edit to the page. Nothing is parsed,
    /// rendered, patched or loaded again — the page on screen simply restyles, and a
    /// document loaded later is already styled at its first paint rather than
    /// reflowing after it (probed on WebKitGTK 2.52.5, both directions).
    pub(crate) fn restyle(&self, stylesheet: &str) {
        let Some(content) = self.webview.user_content_manager() else {
            return;
        };
        content.remove_all_style_sheets();
        content.add_style_sheet(&webkit6::UserStyleSheet::new(
            stylesheet,
            // The document is the whole of what this view shows; there are no frames
            // in it, and a rendered document cannot make one.
            webkit6::UserContentInjectedFrames::TopFrame,
            webkit6::UserStyleLevel::User,
            &[],
            &[],
        ));
    }

    /// The network session this view's fetches go through — the app's only one.
    pub(crate) fn network_session(&self) -> Option<webkit6::NetworkSession> {
        self.webview.network_session()
    }

    /// Puts `rendered` on screen as the current state of `publication`'s document,
    /// at `fragment` when the reader asked for a section of it.
    ///
    /// The first render of a document is loaded; every render after it is patched into
    /// the page already showing, keeping the reader's place and the view's navigation
    /// count.
    pub(crate) fn show(
        self: &Rc<Self>,
        publication: &Publication,
        rendered: &Rendered,
        fragment: &str,
    ) {
        // Whatever happens to the page in front of the reader, the origin behind it
        // serves the document they are looking at.
        publication.show(rendered.html().to_owned());

        let arriving = self.loaded.borrow().as_deref() != Some(publication.uri());
        if arriving {
            *self.loaded.borrow_mut() = Some(publication.uri().to_owned());
            *self.root.borrow_mut() = publication.root();
            // The images belonged to the document being left, not to this one.
            *self.images.borrow_mut() = RemoteImages::default();
            // And so did the marks: this page has never been searched.
            if let Some(finding) = self.finding.borrow_mut().as_mut() {
                finding.marked = false;
            }
        }
        self.images.borrow_mut().sources = rendered.remote_images().to_vec();

        if arriving {
            self.ready.set(false);
            *self.pending.borrow_mut() = None;
            self.webview.load_uri(&match fragment {
                "" => publication.uri().to_owned(),
                fragment => format!("{}#{fragment}", publication.uri()),
            });
        } else if self.ready.get() {
            self.update(Some(rendered.body()));
        } else {
            *self.pending.borrow_mut() = Some(rendered.body().to_owned());
        }
    }

    /// Asks the page which source line the reader is looking at: the line of the
    /// topmost block that is still on screen, which is what edit mode puts the caret on.
    ///
    /// The answer arrives on the main loop; a page with no blocks in it answers with
    /// the first line, because that is where an empty document starts.
    pub(crate) fn topmost_source_line(&self, then: impl FnOnce(u32) + 'static) {
        let webview = self.webview.clone();
        let ready = self.ready.get();
        glib::spawn_future_local(async move {
            let mut line = 1;
            if ready
                && let Ok(answer) = webview
                    .evaluate_javascript_future(&format!("({PLACE}).topmost()"), Some(WORLD), None)
                    .await
            {
                line = answer.to_str().parse().unwrap_or(1);
            }
            then(line);
        });
    }

    /// Brings the block that source `line` belongs to to the top of the page, once the
    /// page has caught up with the buffer.
    ///
    /// Deliberately not now: the reader is leaving the editor, and the page still
    /// shows the document as it was before they typed. Scrolling it would land on
    /// blocks the document no longer has. The view applies this at the end of the next
    /// update, in the same task as the patch, so the place is always meant against the
    /// blocks it is applied to.
    ///
    /// The rule, stated once in `place.js` and shared with live reload: the block the
    /// reader means is the last one that begins at or before `line`, so a caret inside
    /// a paragraph lands on that paragraph rather than on the one after it.
    pub(crate) fn place_after_next_render(&self, line: u32) {
        self.place.set(Some(line));
    }

    /// The remote images of the document on screen that nobody has asked for yet.
    pub(crate) fn unloaded_images(&self) -> Vec<String> {
        let images = self.images.borrow();
        images
            .sources
            .iter()
            .filter(|source| !images.loaded.iter().any(|(url, _)| url == *source))
            .cloned()
            .collect()
    }

    /// Whether the reader has already asked for `source` and got it.
    pub(crate) fn has_image(&self, source: &str) -> bool {
        self.images
            .borrow()
            .loaded
            .iter()
            .any(|(url, _)| url == source)
    }

    /// Puts an image the reader asked for into the page, at every placeholder that
    /// stands for it.
    pub(crate) fn image_arrived(self: &Rc<Self>, source: &str, uri: String) {
        {
            let mut images = self.images.borrow_mut();
            images.failed.retain(|(url, _)| url != source);
            images.loaded.push((source.to_owned(), uri));
        }
        self.update(None);
    }

    /// Says beside the placeholder why the image is not there — inline, and still one
    /// click away from being tried again.
    pub(crate) fn image_failed(self: &Rc<Self>, source: &str, complaint: String) {
        self.images
            .borrow_mut()
            .failed
            .retain(|(url, _)| url != source);
        self.images
            .borrow_mut()
            .failed
            .push((source.to_owned(), complaint));
        self.update(None);
    }

    /// Brings the page up to date: the blocks that changed, and then the remote
    /// images the reader has asked for, which the patch has just rebuilt back into
    /// placeholders.
    ///
    /// Both in one task, in that order, because two tasks would race and the reader
    /// would sometimes be left looking at the placeholder for an image they already
    /// loaded.
    fn update(self: &Rc<Self>, body: Option<&str>) {
        if !self.ready.get() {
            return;
        }
        let patch = body.map(|body| format!("({PATCH})({})", as_js_string(body)));
        let remote = format!("({REMOTE})({})", self.remote_state());
        let place = self
            .place
            .take()
            .map(|line| format!("({PLACE}).scrollTo({line}, false)"));
        let track = format!("({TRACK})({PLACE}, \"{SECTION_MESSAGE}\")");
        // The marks stand in the blocks the patch is about to compare and replace, and
        // a block holding one would be rebuilt as changed even where the reader changed
        // nothing. They come off here and go back on at the end of this same task.
        //
        // Whether or not this view believes it has marked anything: a search asked for a
        // moment ago may still be on its way to the page, and it will get there before
        // this does — the web process runs what it is sent in the order it is sent.
        let unmark = self
            .finding
            .borrow()
            .is_some()
            .then(|| format!("({FIND}).clear()"));
        let searching = self.finding.clone();
        let webview = self.webview.clone();
        glib::spawn_future_local(async move {
            if let Some(unmark) = unmark
                && let Err(error) = webview
                    .evaluate_javascript_future(&unmark, Some(WORLD), None)
                    .await
            {
                eprintln!("axiomd: the search could not be taken off the document: {error}");
            }
            // Returns at once and finishes on the main loop: patching a large document
            // must not be something the window waits for (invariant 4).
            if let Some(patch) = patch
                && let Err(error) = webview
                    .evaluate_javascript_future(&patch, Some(WORLD), None)
                    .await
            {
                eprintln!("axiomd: the document could not be updated in place: {error}");
            }
            if let Err(error) = webview
                .evaluate_javascript_future(&remote, Some(WORLD), None)
                .await
            {
                eprintln!("axiomd: the loaded images could not be shown: {error}");
            }
            // Last, and in this same task: the reader's place is only meaningful
            // against the blocks the page has just been given.
            if let Some(place) = place
                && let Err(error) = webview
                    .evaluate_javascript_future(&place, Some(WORLD), None)
                    .await
            {
                eprintln!("axiomd: the reader's place could not be restored: {error}");
            }
            // And then watch the blocks the page has just been given, and say which
            // section that left the reader in — so the outline of a document that
            // changed under them highlights where they are now rather than where the
            // document they were reading would have put them.
            if let Err(error) = webview
                .evaluate_javascript_future(&track, Some(WORLD), None)
                .await
            {
                eprintln!("axiomd: the reader's section could not be followed: {error}");
            }
            // And last, the reader's search, back onto the blocks the page now has —
            // without moving them, because a document that changed under somebody
            // searching it must not also carry them somewhere else (invariant 5).
            //
            // Marked afresh rather than re-selected: whatever was in the page is in the
            // blocks the patch has just taken out of it. Said here rather than before
            // the patch, because an answer to a search asked for a moment ago may have
            // arrived in between and claimed otherwise.
            if let Some(finding) = searching.borrow_mut().as_mut() {
                finding.marked = false;
            }
            mark(&webview, &searching, false);
        });
    }

    /// Marks the search this page is showing, if it is showing one.
    ///
    /// `bring` is whether the reader asked to be taken to the current match: they did
    /// when they typed or pressed Next, and they did not when the document changed
    /// under them.
    fn mark_matches(&self, bring: bool) {
        if !self.ready.get() {
            // The page is still arriving. Nothing is lost: the load finishes into
            // `update`, which comes back through here.
            return;
        }
        mark(&self.webview, &self.finding, bring);
    }

    /// The remote images of this page as the object `remote.js` is called with.
    fn remote_state(&self) -> String {
        let images = self.images.borrow();
        format!(
            "{{loaded:{},failed:{}}}",
            as_js_object(&images.loaded),
            as_js_object(&images.failed),
        )
    }
}

impl Searchable for DocumentView {
    /// The rendered page, searched by its words: `[needle](https://example.com/needle)`
    /// is the word `needle` once here, where the source has it twice, and a search for
    /// `example.com` finds nothing at all (`find.js`).
    fn show_matches(&self, looking_for: &Query, nth: usize, bring: bool, counted: Counted) {
        {
            let mut finding = self.finding.borrow_mut();
            // Only the current match moved: the marks are already where they belong,
            // so pressing Next costs a class and a scroll rather than a walk of the
            // document.
            let marked = finding
                .as_ref()
                .is_some_and(|finding| finding.marked && finding.looking_for == *looking_for);
            *finding = Some(Finding {
                looking_for: looking_for.clone(),
                nth,
                marked,
                counted,
            });
        }
        self.mark_matches(bring);
    }

    fn hide_matches(&self) {
        let marked = self.finding.borrow_mut().take().is_some();
        if !marked || !self.ready.get() {
            return;
        }
        let webview = self.webview.clone();
        glib::spawn_future_local(async move {
            if let Err(error) = webview
                .evaluate_javascript_future(&format!("({FIND}).clear()"), Some(WORLD), None)
                .await
            {
                eprintln!("axiomd: the search could not be taken off the document: {error}");
            }
        });
    }
}

/// Puts the search in `finding` onto the page `webview` is showing, and tells whoever
/// asked for it how many matches it made.
///
/// Free of the view on purpose: it is called both from the view and from the task that
/// has just patched the document, and that task must be able to finish without keeping
/// a window alive that the reader has closed.
fn mark(webview: &webkit6::WebView, finding: &Rc<RefCell<Option<Finding>>>, bring: bool) {
    let Some((script, looking_for)) = finding.borrow().as_ref().map(|finding| {
        let script = if finding.marked {
            format!("({FIND}).select({}, {bring})", finding.nth)
        } else {
            format!(
                "({FIND}).apply({}, {}, {}, {bring})",
                as_js_string(&finding.looking_for.text),
                finding.looking_for.cased,
                finding.nth,
            )
        };
        (script, finding.looking_for.clone())
    }) else {
        return;
    };

    let finding = finding.clone();
    let webview = webview.clone();
    glib::spawn_future_local(async move {
        let counted = match webview
            .evaluate_javascript_future(&script, Some(WORLD), None)
            .await
        {
            Ok(answer) => answer.to_str().parse().unwrap_or(0),
            Err(error) => {
                eprintln!("axiomd: the document could not be searched: {error}");
                return;
            }
        };
        // The reader may have typed since this went out. The answer says which query
        // it counted, and an answer about a search they have moved on from is dropped
        // rather than shown as the count of the one they are running.
        let telling = {
            let mut finding = finding.borrow_mut();
            match finding.as_mut() {
                Some(current) if current.looking_for == looking_for => {
                    current.marked = true;
                    Some(current.counted.clone())
                }
                _ => None,
            }
        };
        if let Some(telling) = telling {
            telling(&looking_for, counted);
        }
    });
}

/// `text` as a JavaScript string literal.
///
/// A document is arbitrary text — quotes, backslashes, newlines, and the two
/// separators a JavaScript parser treats as line breaks — and all of it has to survive
/// the trip into the page as data rather than as syntax.
fn as_js_string(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for character in text.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            // Ordinary text in a document; a line terminator to JavaScript.
            '\u{2028}' => quoted.push_str("\\u2028"),
            '\u{2029}' => quoted.push_str("\\u2029"),
            control if control < ' ' => quoted.push_str(&format!("\\u{:04x}", control as u32)),
            plain => quoted.push(plain),
        }
    }
    quoted.push('"');
    quoted
}

/// Builds the webview a document is displayed in, with everything a reading surface
/// does not need switched off.
fn build_webview(
    context: &webkit6::WebContext,
    content: &webkit6::UserContentManager,
) -> webkit6::WebView {
    let webview = webkit6::WebView::builder()
        .web_context(context)
        .settings(&document_settings())
        .user_content_manager(content)
        .vexpand(true)
        .hexpand(true)
        .build();

    // Camera, microphone, geolocation, notifications: a Markdown document has no
    // business asking, so the answer is given without ever reaching the user.
    webview.connect_permission_request(|_, request| {
        request.deny();
        true
    });
    webview.connect_query_permission_state(|_, query| {
        query.finish(webkit6::PermissionState::Denied);
        true
    });

    webview.connect_context_menu(|_, menu, _| {
        for item in menu.items() {
            if !is_reader_menu_item(&item) {
                menu.remove(&item);
            }
        }
        menu.n_items() == 0
    });

    webview
}

/// The settings a document is displayed under.
///
/// Everything that could run code, capture media, keep state, or reach outside the
/// document is off. Images stay on: they are core rendering, and the only ones that
/// load are the ones the scheme resolves inside the document's own directory.
fn document_settings() -> webkit6::Settings {
    let settings = webkit6::Settings::new();

    settings.set_enable_javascript(false);
    settings.set_enable_javascript_markup(false);
    settings.set_javascript_can_access_clipboard(false);
    settings.set_javascript_can_open_windows_automatically(false);

    // Never, under any circumstance: this is the hole that let Apostrophe's preview
    // read the user's filesystem.
    settings.set_allow_file_access_from_file_urls(false);
    settings.set_allow_universal_access_from_file_urls(false);
    settings.set_allow_top_navigation_to_data_urls(false);
    settings.set_disable_web_security(false);

    settings.set_enable_media(false);
    settings.set_enable_media_stream(false);
    settings.set_enable_mediasource(false);
    settings.set_enable_encrypted_media(false);
    settings.set_enable_webrtc(false);
    settings.set_enable_webaudio(false);
    settings.set_enable_webgl(false);
    settings.set_enable_fullscreen(false);

    settings.set_enable_html5_database(false);
    settings.set_enable_html5_local_storage(false);
    settings.set_enable_page_cache(false);
    settings.set_enable_dns_prefetching(false);
    settings.set_enable_developer_extras(false);
    settings.set_allow_modal_dialogs(false);

    settings.set_auto_load_images(true);
    settings.set_enable_smooth_scrolling(true);
    settings.set_enable_back_forward_navigation_gestures(false);

    settings
}

/// `pairs` as a JavaScript object literal, keys and values alike carried as data.
///
/// Both halves come from a document — an image's source is whatever the author
/// wrote — so neither may arrive at the parser as syntax.
fn as_js_object(pairs: &[(String, String)]) -> String {
    let mut object = String::from("{");
    for (key, value) in pairs {
        if object.len() > 1 {
            object.push(',');
        }
        object.push_str(&as_js_string(key));
        object.push(':');
        object.push_str(&as_js_string(value));
    }
    object.push('}');
    object
}

/// Whether a context-menu entry belongs in a reading surface. Everything a viewer
/// cannot honour — navigation history, downloads, media controls, spelling, the web
/// inspector — is dropped rather than shown doing nothing.
fn is_reader_menu_item(item: &webkit6::ContextMenuItem) -> bool {
    use webkit6::ContextMenuAction::{
        Copy, CopyImageToClipboard, CopyImageUrlToClipboard, CopyLinkToClipboard, SelectAll,
    };

    matches!(
        item.stock_action(),
        Copy | SelectAll | CopyLinkToClipboard | CopyImageToClipboard | CopyImageUrlToClipboard
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::with_webkit;

    /// The capabilities a document is displayed without. WebKit turns several of
    /// these on by default, so the list is the difference between a viewer and a
    /// browser someone can point at your filesystem.
    #[test]
    fn a_document_is_displayed_without_scripting_storage_or_capture() {
        let settings = with_webkit(document_settings);

        assert!(!settings.enables_javascript(), "JavaScript is enabled");
        assert!(!settings.enables_javascript_markup());
        assert!(!settings.is_javascript_can_access_clipboard());
        assert!(!settings.is_javascript_can_open_windows_automatically());

        assert!(
            !settings.allows_file_access_from_file_urls(),
            "documents were granted file:// access",
        );
        assert!(
            !settings.allows_universal_access_from_file_urls(),
            "documents were granted universal access from file:// — never do this",
        );
        assert!(!settings.allows_top_navigation_to_data_urls());
        assert!(!settings.is_disable_web_security());

        assert!(!settings.enables_media());
        assert!(!settings.enables_media_stream(), "media capture is enabled");
        assert!(!settings.enables_webrtc());
        assert!(!settings.enables_html5_database());
        assert!(!settings.enables_html5_local_storage());
        assert!(!settings.allows_modal_dialogs());
    }

    /// Images are the one fetch a document may cause, and only through the scheme.
    #[test]
    fn a_document_still_shows_its_images() {
        assert!(with_webkit(document_settings).is_auto_load_images());
    }

    /// Where the view may and may not go is decided by `links.rs`, and tested there
    /// against every class of link a document can hold.
    ///
    /// A document is text the app did not write, and it is handed to a JavaScript
    /// parser. Everything that could end the string early, or end the line early, is
    /// data by the time it gets there.
    #[test]
    fn a_document_reaches_the_page_as_text_and_never_as_syntax() {
        assert_eq!(as_js_string("<p>plain</p>"), "\"<p>plain</p>\"");
        assert_eq!(
            as_js_string("a \"quote\" and a \\ backslash"),
            "\"a \\\"quote\\\" and a \\\\ backslash\"",
        );
        assert_eq!(as_js_string("one\ntwo\r\n"), "\"one\\ntwo\\r\\n\"");
        assert_eq!(as_js_string("tab\there"), "\"tab\\u0009here\"");
        assert_eq!(
            as_js_string("line\u{2028}separator\u{2029}"),
            "\"line\\u2028separator\\u2029\"",
        );
        // The escape a naive one would add and this one must not: a document's own
        // apostrophes stay exactly as the reader wrote them.
        assert_eq!(as_js_string("it's"), "\"it's\"");
    }

    /// An image's source is whatever the document's author wrote, and it is a *key*
    /// in what the page is handed. A key is as much syntax as a value is.
    #[test]
    fn the_remote_image_state_carries_a_documents_own_urls_as_data() {
        assert_eq!(as_js_object(&[]), "{}");
        assert_eq!(
            as_js_object(&[
                (
                    "https://example.com/a.png".to_owned(),
                    "axiomd://img-1/0".to_owned()
                ),
                ("https://example.com/b.png".to_owned(), "".to_owned()),
            ]),
            "{\"https://example.com/a.png\":\"axiomd://img-1/0\",\
             \"https://example.com/b.png\":\"\"}",
        );
        assert_eq!(
            as_js_object(&[("\"},alert(1),{\"x".to_owned(), "y".to_owned())]),
            "{\"\\\"},alert(1),{\\\"x\":\"y\"}",
        );
    }
}
