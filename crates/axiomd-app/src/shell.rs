//! The application shell: everything between the process entry point and a
//! document on screen.
//!
//! Hides the GTK/libadwaita stack behind a single call, and owns the two things that
//! belong to the application rather than to any one window: the set of open document
//! windows, and the `axiomd://` scheme they publish to. Everything else — widgets,
//! webview, rendering — belongs to a window and dies with it.
//!
//! # How a document reaches a window
//!
//! `axiomd file.md`, a double-click in Files and `xdg-open` all arrive the same way:
//! the application declares `HANDLES_OPEN`, so the desktop hands it documents rather
//! than command-line arguments. Every route ends in [`Shell::show`], which brings the
//! window that already holds that file to the front instead of opening a second one.
//! Sameness is the file's identity on disk, so a relative path, an absolute path and
//! a symlink all find the window that is already open.

use std::cell::{OnceCell, RefCell};
use std::path::Path;
use std::process::ExitCode;
use std::rc::Rc;

use adw::prelude::*;
use gtk::gio;
use gtk::glib;

use crate::document::FileId;
use crate::scheme::Scheme;
use crate::window::DocumentWindow;

/// Owner-chosen application id (2026-08-02). Also the D-Bus well-known name and
/// the base name of the desktop file, so it cannot change casually.
const APP_ID: &str = "io.github.etf.axiomd";

/// Every keyboard shortcut the application installs, and the action each one runs.
///
/// The two halves are asserted together: a shortcut naming an action that does not
/// exist is a key that silently does nothing.
const SHORTCUTS: &[(&str, &str)] = &[
    ("app.new", "<Control>n"),
    ("app.open", "<Control>o"),
    ("app.close-window", "<Control>w"),
    ("app.quit", "<Control>q"),
];

/// The same, for the actions that belong to a window rather than to the application:
/// where the reader has been in that window (`window.rs`).
const WINDOW_SHORTCUTS: &[(&str, &str)] = &[
    (crate::window::BACK, "<Alt>Left"),
    (crate::window::FORWARD, "<Alt>Right"),
];

/// Runs axiomd to completion and reports the process exit status.
pub fn run() -> ExitCode {
    // Answered before touching GTK so that `axiomd --version` works over ssh,
    // in a container, or anywhere else without a display.
    if std::env::args_os().skip(1).any(|arg| arg == "--version") {
        println!("axiomd {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }

    let app = build_application(Rc::new(Shell::new()));

    if app.run() == glib::ExitCode::SUCCESS {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// The application's own state: the windows that are open, and the origin they serve
/// their documents from.
pub(crate) struct Shell {
    scheme: Rc<Scheme>,
    /// Built on first use rather than at construction, because it needs an
    /// initialised GTK — and because the scheme has to be installed on it before any
    /// webview asks for a document.
    context: OnceCell<webkit6::WebContext>,
    windows: RefCell<Vec<Rc<DocumentWindow>>>,
}

impl Shell {
    fn new() -> Self {
        Self {
            scheme: Rc::new(Scheme::new()),
            context: OnceCell::new(),
            windows: RefCell::new(Vec::new()),
        }
    }

    /// Puts `file` on screen.
    ///
    /// A file that is already open comes to the front instead of opening twice.
    /// Otherwise it takes over `into` when one is given — the window the user pressed
    /// Ctrl+O in — and gets a new window when not.
    pub(crate) fn show(
        self: &Rc<Self>,
        app: &adw::Application,
        file: &Path,
        into: Option<&Rc<DocumentWindow>>,
    ) {
        if let Some(open) = self.window_holding(file) {
            open.present();
            return;
        }
        let window = match into {
            Some(window) => window.clone(),
            None => self.new_window(app),
        };
        window.show(file);
        window.present();
    }

    /// Opens a window with no document in it.
    fn show_empty(self: &Rc<Self>, app: &adw::Application) {
        self.new_window(app).present();
    }

    /// Opens a window that explains why `location` could not be read.
    fn show_unopenable(self: &Rc<Self>, app: &adw::Application, location: &str) {
        let window = self.new_window(app);
        window.show_unavailable(
            "Could not open this document",
            &format!("{location} is not a file on this computer."),
        );
        window.present();
    }

    /// How many document windows are open, in the order they were opened.
    pub(crate) fn window_count(&self) -> usize {
        self.windows.borrow().len()
    }

    pub(crate) fn window_at(&self, index: usize) -> Option<Rc<DocumentWindow>> {
        self.windows.borrow().get(index).cloned()
    }

    fn window_holding(&self, file: &Path) -> Option<Rc<DocumentWindow>> {
        let wanted = FileId::of(file)?;
        self.windows
            .borrow()
            .iter()
            .find(|window| window.file_id() == Some(wanted))
            .cloned()
    }

    /// The window the user is acting in, as this shell knows it.
    fn active_window(&self, app: &adw::Application) -> Option<Rc<DocumentWindow>> {
        let active = app.active_window()?;
        self.windows
            .borrow()
            .iter()
            .find(|window| window.window().upcast_ref::<gtk::Window>() == &active)
            .cloned()
    }

    fn new_window(self: &Rc<Self>, app: &adw::Application) -> Rc<DocumentWindow> {
        let window = DocumentWindow::new(app, self.context(), &self.scheme);
        self.windows.borrow_mut().push(window.clone());

        // Forgetting the window here is what frees it: with the shell's reference
        // gone, its webview, its renderer and its place on the scheme go too.
        //
        // On `close-request` rather than on `destroy`, which never arrives while the
        // shell is the thing holding the window. GTK4 emits `GtkWidget::destroy` when
        // the widget is disposed — that is, once nothing references it any more — so a
        // handler whose whole job is to drop the last reference cannot run: the
        // reference it would drop is what keeps the emission from happening. Probed on
        // this machine (GTK 4.20.4) by closing a window with handlers on all four
        // signals: `close-request` and `unrealize` fired, `hide` and `destroy` did not,
        // and GTK had already taken the window off `GtkApplication`'s own list. That is
        // the leak `a_document_reopened_after_its_window_closed_comes_back` catches.
        let shell = self.clone();
        window.window().connect_close_request(move |closing| {
            shell
                .windows
                .borrow_mut()
                .retain(|open| open.window() != closing);
            glib::Propagation::Proceed
        });
        window
    }

    /// The one web context the application uses, with the scheme already installed.
    ///
    /// Going through here is what guarantees the ordering WebKit requires: a webview
    /// can only be built from a context that already answers for `axiomd://`.
    fn context(&self) -> &webkit6::WebContext {
        self.context.get_or_init(|| {
            let context = webkit6::WebContext::new();
            self.scheme.install(&context);
            context
        })
    }
}

fn build_application(shell: Rc<Shell>) -> adw::Application {
    let app = adw::Application::builder()
        .application_id(APP_ID)
        // Files, `xdg-open` and the command line all hand axiomd documents, not
        // arguments; without this the desktop cannot launch it with a file at all.
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();

    // Before any window exists, so a test is already listening when the first one
    // appears. Does nothing at all unless a test launched this process.
    app.connect_startup({
        let shell = shell.clone();
        move |app| crate::control::arm(app, &shell)
    });

    app.connect_activate({
        let shell = shell.clone();
        move |app| shell.show_empty(app)
    });

    app.connect_open({
        let shell = shell.clone();
        move |app, files, _hint| {
            for file in files {
                match file.path() {
                    Some(path) => shell.show(app, &path, None),
                    // A location with no local path — something on a remote mount
                    // that was never made a file. The window says so; a launch that
                    // failed silently would leave the user with nothing at all.
                    None => shell.show_unopenable(app, &file.uri()),
                }
            }
        }
    });

    add_action(&app, "new", {
        let shell = shell.clone();
        move |app| shell.show_empty(app)
    });
    add_action(&app, "open", {
        let shell = shell.clone();
        move |app| ask_for_a_document(&shell, app)
    });
    add_action(&app, "close-window", |app| {
        if let Some(window) = app.active_window() {
            window.close();
        }
    });
    add_action(&app, "quit", |app| app.quit());

    for (action, accelerator) in SHORTCUTS.iter().chain(WINDOW_SHORTCUTS) {
        app.set_accels_for_action(action, &[accelerator]);
    }

    app
}

/// Adds an application action that acts on the running application.
fn add_action(app: &adw::Application, name: &str, activate: impl Fn(&adw::Application) + 'static) {
    let action = gio::SimpleAction::new(name, None);
    let weak = app.downgrade();
    action.connect_activate(move |_, _| {
        if let Some(app) = weak.upgrade() {
            activate(&app);
        }
    });
    app.add_action(&action);
}

/// Ctrl+O: the file chooser, and then the document.
///
/// The chooser is a dialog the user asked for, which is the only kind this app has;
/// the document it produces then opens without any further question.
fn ask_for_a_document(shell: &Rc<Shell>, app: &adw::Application) {
    let markdown = gtk::FileFilter::new();
    markdown.set_name(Some("Markdown Documents"));
    markdown.add_mime_type("text/markdown");
    markdown.add_pattern("*.md");
    markdown.add_pattern("*.markdown");

    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&markdown);

    let dialog = gtk::FileDialog::builder()
        .title("Open Document")
        .filters(&filters)
        .default_filter(&markdown)
        .modal(true)
        .build();

    let into = shell.active_window(app);
    let parent = app.active_window();
    let shell = shell.clone();
    let app = app.clone();
    dialog.open(parent.as_ref(), gio::Cancellable::NONE, move |chosen| {
        // A cancelled chooser is not an error, and never becomes a message.
        if let Ok(file) = chosen
            && let Some(path) = file.path()
        {
            shell.show(&app, &path, into.as_ref());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn application() -> adw::Application {
        build_application(Rc::new(Shell::new()))
    }

    /// The id is the app's identity on D-Bus, in the desktop file and to the
    /// session's single-instance handling; a silent change breaks all three.
    #[test]
    fn application_carries_the_owner_chosen_app_id() {
        assert_eq!(
            application().application_id().as_deref(),
            Some("io.github.etf.axiomd"),
        );
    }

    /// Without this flag `axiomd README.md`, `xdg-open README.md` and a double-click
    /// in Files all fail before the application ever sees the document.
    #[test]
    fn the_application_is_launched_with_documents_not_arguments() {
        assert!(
            application()
                .flags()
                .contains(gio::ApplicationFlags::HANDLES_OPEN),
        );
    }

    /// The shortcuts issue #4 specifies, spelled out here rather than read back from
    /// the table that installs them, and each checked twice: the key reaches the
    /// action, and the action is one the application actually has. A key bound to an
    /// action nobody registered is a key that silently does nothing.
    #[test]
    fn the_documented_shortcuts_reach_actions_the_application_has() {
        let app = application();

        for (accelerator, action) in [
            ("<Control>n", "app.new"),
            ("<Control>o", "app.open"),
            ("<Control>w", "app.close-window"),
            ("<Control>q", "app.quit"),
        ] {
            assert_eq!(
                app.accels_for_action(action),
                vec![glib::GString::from(accelerator)],
                "{action} is not on {accelerator}",
            );
            let name = action.strip_prefix("app.").expect("an application action");
            assert!(
                app.lookup_action(name).is_some(),
                "{action} has no action behind it",
            );
        }
    }

    /// Back and forward belong to a window rather than to the application, so this
    /// only checks the keys reach the names; that the names reach something a window
    /// does is asserted against the running app by the link suite, which presses them.
    #[test]
    fn where_the_reader_has_been_is_on_the_keys_a_desktop_uses_for_it() {
        let app = application();

        for (action, accelerator) in [("win.back", "<Alt>Left"), ("win.forward", "<Alt>Right")] {
            assert_eq!(
                app.accels_for_action(action),
                vec![glib::GString::from(accelerator)],
                "{action} is not on {accelerator}",
            );
        }
    }
}
