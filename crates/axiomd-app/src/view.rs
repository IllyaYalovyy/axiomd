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
//! and every other capability a reading surface has no use for, and
//! [`navigation_stays_in_document`] keeps the view on the document it was given, so a
//! link in a document cannot make the app fetch anything. Together with the rendered
//! document's own content-security policy and the sanitiser upstream, the `axiomd://`
//! scheme is the complete set of bytes a document can reach. There is no `file://`
//! grant anywhere.
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

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use axiomd_render::Rendered;
use gtk::glib;
use webkit6::prelude::*;

use crate::scheme::Publication;

/// The JavaScript world the app updates a document from — never the document's own.
const WORLD: &str = "axiomd-view";

/// The patch: new blocks in, changed blocks replaced, the reader left where they were.
const PATCH: &str = include_str!("patch.js");

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
}

impl DocumentView {
    pub(crate) fn new(context: &webkit6::WebContext) -> Rc<Self> {
        let view = Rc::new(Self {
            webview: build_webview(context),
            loaded: RefCell::new(None),
            ready: Cell::new(false),
            pending: RefCell::new(None),
            navigations: Cell::new(0),
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
                    if let Some(body) = waiting {
                        view.patch(&body);
                    }
                }
                _ => {}
            }
        });
        view
    }

    pub(crate) fn widget(&self) -> &webkit6::WebView {
        &self.webview
    }

    /// How many loads this view has committed since it was built.
    pub(crate) fn navigations(&self) -> u32 {
        self.navigations.get()
    }

    /// Puts `rendered` on screen as the current state of `publication`'s document.
    ///
    /// The first render of a document is loaded; every render after it is patched into
    /// the page already showing, keeping the reader's place and the view's navigation
    /// count.
    pub(crate) fn show(&self, publication: &Publication, rendered: &Rendered) {
        // Whatever happens to the page in front of the reader, the origin behind it
        // serves the document they are looking at.
        publication.show(rendered.html().to_owned());

        if self.loaded.borrow().as_deref() != Some(publication.uri()) {
            *self.loaded.borrow_mut() = Some(publication.uri().to_owned());
            self.ready.set(false);
            *self.pending.borrow_mut() = None;
            self.webview.load_uri(publication.uri());
        } else if self.ready.get() {
            self.patch(rendered.body());
        } else {
            *self.pending.borrow_mut() = Some(rendered.body().to_owned());
        }
    }

    /// Makes the document on screen into the one `body` describes, without leaving it.
    fn patch(&self, body: &str) {
        let script = format!("({PATCH})({})", as_js_string(body));
        let webview = self.webview.clone();
        glib::spawn_future_local(async move {
            // Returns at once and finishes on the main loop: patching a large document
            // must not be something the window waits for (invariant 4).
            if let Err(error) = webview
                .evaluate_javascript_future(&script, Some(WORLD), None)
                .await
            {
                eprintln!("axiomd: the document could not be updated in place: {error}");
            }
        });
    }
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
fn build_webview(context: &webkit6::WebContext) -> webkit6::WebView {
    let webview = webkit6::WebView::builder()
        .web_context(context)
        .settings(&document_settings())
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

    // A document is not a browser tab: it cannot navigate the view somewhere else,
    // and it cannot open a window. Following links to other documents and to the
    // browser is issue #6; until then the view stays where it was put.
    webview.connect_decide_policy(|webview, decision, kind| match kind {
        webkit6::PolicyDecisionType::NavigationAction => {
            let Some(decision) = decision.downcast_ref::<webkit6::NavigationPolicyDecision>()
            else {
                return false;
            };
            let target = decision
                .navigation_action()
                .and_then(|action| action.request())
                .and_then(|request| request.uri())
                .unwrap_or_default();
            let here = webview.uri().unwrap_or_default();
            if navigation_stays_in_document(&here, &target) {
                decision.use_();
            } else {
                decision.ignore();
            }
            true
        }
        webkit6::PolicyDecisionType::NewWindowAction => {
            decision.ignore();
            true
        }
        _ => false,
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

/// Whether the view showing `here` may follow `target`.
///
/// Only the document itself and its own fragments qualify. An `http` link, a
/// `file://` path, another window's document: all refused, so no click can turn into
/// a network request or a filesystem read.
fn navigation_stays_in_document(here: &str, target: &str) -> bool {
    if here.is_empty() {
        // The document's own first load, before the view has a URI of its own.
        return target.starts_with("axiomd://");
    }
    target == here
        || target
            .strip_prefix(here)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('#'))
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

    #[test]
    fn the_view_follows_nothing_outside_the_document_it_shows() {
        let here = "axiomd://doc-3/";

        assert!(navigation_stays_in_document(here, here));
        assert!(navigation_stays_in_document(
            here,
            "axiomd://doc-3/#heading"
        ));

        for elsewhere in [
            "https://example.com/",
            "http://example.com/tracker.gif",
            "file:///etc/passwd",
            "axiomd://doc-4/",
            "data:text/html,<h1>hi</h1>",
            "axiomd://doc-3/../../etc/passwd",
        ] {
            assert!(
                !navigation_stays_in_document(here, elsewhere),
                "{elsewhere} was allowed",
            );
        }
    }

    /// The very first load has no current URI to compare against, and must still be
    /// allowed — otherwise no document would ever appear.
    #[test]
    fn the_documents_own_first_load_is_allowed() {
        assert!(navigation_stays_in_document("", "axiomd://doc-0/"));
        assert!(!navigation_stays_in_document("", "https://example.com/"));
    }

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
}
