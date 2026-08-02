//! One document, one window.
//!
//! The window owns everything that belongs to its document and nothing that belongs
//! to another: its own webview, its own renderer, and its own place on the
//! `axiomd://` scheme. Closing it drops all three, so nothing survives a closed
//! window — no shared state, no reachable page, no worker result with anywhere to go.
//!
//! # What the webview is allowed to do
//!
//! Almost nothing. [`document_settings`] turns off JavaScript, media capture,
//! storage and every other capability a reading surface has no use for, and
//! [`navigation_stays_in_document`] keeps the view on the document it was given, so
//! a link in a document cannot make the app fetch anything. Together with the
//! rendered document's own content-security policy and the sanitiser upstream, the
//! `axiomd://` scheme is the complete set of bytes a document can reach. There is no
//! `file://` grant anywhere.
//!
//! # What the user sees while something is wrong
//!
//! An unreadable file, a file that is not text, an empty window: all of them are a
//! status page inside the window, never a dialog. Opening and reading are never
//! interrupted by a question (`ux_decisions.md`).

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use adw::prelude::*;
use gtk::gio;
use gtk::glib;
use webkit6::prelude::*;

use crate::document::{FileId, Page, Renderer};
use crate::scheme::Scheme;

/// A window showing one document, or none yet.
pub(crate) struct DocumentWindow {
    window: adw::ApplicationWindow,
    title: adw::WindowTitle,
    webview: webkit6::WebView,
    status: adw::StatusPage,
    pages: gtk::Stack,
    scheme: Rc<Scheme>,
    open: RefCell<Option<OpenDocument>>,
}

/// The document a window currently holds.
struct OpenDocument {
    id: FileId,
    /// Never read — held so that the document lives exactly as long as the window
    /// shows it. Dropping the renderer drops the callback that keeps the document
    /// published, which withdraws it from the scheme.
    #[expect(
        dead_code,
        reason = "ownership, not data: it ties the document to the window"
    )]
    renderer: Renderer,
}

/// Names for the two things a window can be showing.
const DOCUMENT_PAGE: &str = "document";
const STATUS_PAGE: &str = "status";

impl DocumentWindow {
    /// Builds an empty window, ready for a document.
    pub(crate) fn new(
        app: &adw::Application,
        context: &webkit6::WebContext,
        scheme: &Rc<Scheme>,
    ) -> Rc<Self> {
        let webview = build_webview(context);
        let status = adw::StatusPage::builder()
            .icon_name("text-x-generic-symbolic")
            .title("No document open")
            .description("Open a Markdown file to start reading.")
            .child(&open_button())
            .build();

        let pages = gtk::Stack::new();
        pages.add_named(&status, Some(STATUS_PAGE));
        pages.add_named(&webview, Some(DOCUMENT_PAGE));
        pages.set_visible_child_name(STATUS_PAGE);

        let title = adw::WindowTitle::new("axiomd", "");
        let header = adw::HeaderBar::builder().title_widget(&title).build();
        header.pack_start(&open_button());
        header.pack_end(&primary_menu_button());

        let layout = adw::ToolbarView::builder().content(&pages).build();
        layout.add_top_bar(&header);

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("axiomd")
            .default_width(900)
            .default_height(700)
            .content(&layout)
            .build();

        Rc::new(Self {
            window,
            title,
            webview,
            status,
            pages,
            scheme: scheme.clone(),
            open: RefCell::new(None),
        })
    }

    pub(crate) fn window(&self) -> &adw::ApplicationWindow {
        &self.window
    }

    pub(crate) fn present(&self) {
        self.window.present();
    }

    /// Which file this window holds, if any. Windows are deduplicated on this.
    pub(crate) fn file_id(&self) -> Option<FileId> {
        self.open.borrow().as_ref().map(|open| open.id)
    }

    /// Shows `file` in this window, replacing whatever it held.
    ///
    /// Returns immediately: the document is read and rendered on a worker and
    /// appears when it is ready. A file that cannot be shown becomes a status page
    /// inside the window, never a dialog.
    pub(crate) fn show(&self, file: &Path) {
        let file = file.to_path_buf();
        let Some(id) = FileId::of(&file) else {
            self.show_unavailable(
                &format!("Could not open {}", file_name(&file)),
                "There is no such file.",
            );
            self.retitle(&file);
            return;
        };

        let publication = Rc::new(self.scheme.publish(&file));
        let renderer = Renderer::new({
            let publication = publication.clone();
            let webview = self.webview.clone();
            let status = self.status.clone();
            let pages = self.pages.clone();
            move |page| match page {
                Page::Rendered(html) => {
                    publication.show(html);
                    if webview.uri().as_deref() == Some(publication.uri()) {
                        webview.reload();
                    } else {
                        webview.load_uri(publication.uri());
                    }
                    pages.set_visible_child_name(DOCUMENT_PAGE);
                }
                Page::Unavailable { title, detail } => {
                    status.set_title(&title);
                    status.set_description(Some(&detail));
                    pages.set_visible_child_name(STATUS_PAGE);
                }
            }
        });
        renderer.render(file.clone());

        self.retitle(&file);
        *self.open.borrow_mut() = Some(OpenDocument { id, renderer });
    }

    /// Says, inside the window, why there is nothing to read — never in a dialog.
    pub(crate) fn show_unavailable(&self, title: &str, detail: &str) {
        self.status.set_title(title);
        self.status.set_description(Some(detail));
        self.pages.set_visible_child_name(STATUS_PAGE);
    }

    fn retitle(&self, file: &Path) {
        let name = file_name(file);
        self.window.set_title(Some(&name));
        self.title.set_title(&name);
        self.title.set_subtitle(&folder_of(file));
    }
}

fn file_name(file: &Path) -> String {
    file.file_name()
        .unwrap_or(file.as_os_str())
        .to_string_lossy()
        .into_owned()
}

/// The document's folder as the user thinks of it, with their home shortened.
fn folder_of(file: &Path) -> String {
    let folder = file.parent().unwrap_or(Path::new("")).display().to_string();
    match glib::home_dir().to_str() {
        Some(home) if folder == home => "~".to_owned(),
        Some(home) => match folder.strip_prefix(&format!("{home}/")) {
            Some(rest) => format!("~/{rest}"),
            None => folder,
        },
        None => folder,
    }
}

fn open_button() -> gtk::Button {
    gtk::Button::builder()
        .icon_name("document-open-symbolic")
        .tooltip_text("Open a document")
        .action_name("app.open")
        .build()
}

fn primary_menu_button() -> gtk::MenuButton {
    let documents = gio::Menu::new();
    documents.append(Some("_New Window"), Some("app.new"));
    documents.append(Some("_Open…"), Some("app.open"));

    let application = gio::Menu::new();
    application.append(Some("_Close Window"), Some("app.close-window"));
    application.append(Some("_Quit"), Some("app.quit"));

    let menu = gio::Menu::new();
    menu.append_section(None, &documents);
    menu.append_section(None, &application);

    gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .tooltip_text("Main menu")
        .menu_model(&menu)
        .build()
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
}
