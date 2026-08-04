//! Drives the real axiomd, headless, and asserts what the user would see.
//!
//! ```no_run
//! # use std::path::Path;
//! let app = axiomd_e2e::launch(Path::new("notes.md"));
//!
//! assert_eq!(app.dom_text("h1"), "Notes");
//! assert_eq!(app.window_count(), 1);
//! assert!(app.close().is_empty(), "something outlived the application");
//! ```
//!
//! # What "real" means here
//!
//! [`launch`] starts the shipped `axiomd` binary as its own process, on a headless
//! compositor of its own, with a private set of configuration directories — nothing
//! is stubbed and no code path is test-only. Commands travel to the running
//! application over its test-control channel and land where a user's action lands:
//! [`App::open`] goes through the call a double-click in Files ends in,
//! [`App::activate`] activates the very action a menu item does. What comes back is
//! what a user could observe — the rendered DOM, the window's title, the number of
//! windows, the pixels.
//!
//! The document itself is unchanged by being tested. It is still displayed with
//! JavaScript off and under its own content-security policy; [`App::dom`] reads it
//! from a separate JavaScript world, which is what lets the document keep every
//! restriction while the test still sees the result.
//!
//! # No sleeps
//!
//! Every wait here is a condition with a deadline: the application connecting, a
//! document appearing in the DOM, a window count changing, the process exiting. A
//! wait that could be answered by sleeping "long enough" is a flaky test waiting to
//! happen, so there are none.

#![deny(missing_docs)]

pub mod budget;
mod control;
pub mod corpus;
mod display;
mod golden;
mod process;
mod scratch;

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub use golden::Screenshot;
pub use process::Footprint;

use control::Control;
use display::{Display, Environment};
pub(crate) use scratch::Scratch;

/// How long a condition — a document rendering, a window closing — may take before
/// the test that waited for it fails.
const SETTLES_WITHIN: Duration = Duration::from_secs(30);

/// The documents one test opens, in a directory of their own that goes away with it.
///
/// A directory rather than a file because a Markdown document is rarely alone: the
/// image it references, the note it links to and the file it is compared against all
/// have to resolve relative to it, exactly as they do in the user's folder.
pub struct Fixture {
    scratch: Scratch,
}

impl Fixture {
    /// Creates an empty folder for a test's documents. `label` names it, so anything
    /// a killed run leaves behind says which test made it.
    pub fn new(label: &str) -> Fixture {
        Fixture {
            scratch: Scratch::new(label),
        }
    }

    /// Writes `contents` to `name` in the folder and returns the path to open.
    pub fn write(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.scratch.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap_or_else(|error| panic!("{parent:?}: {error}"));
        }
        std::fs::write(&path, contents).unwrap_or_else(|error| panic!("write {path:?}: {error}"));
        path
    }
}

/// The outline sidebar as one moment of reading leaves it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outline {
    /// Whether the sidebar is beside the document at all.
    pub shown: bool,
    /// The sections listed, in document order, each as its heading level and its
    /// words — `h2 Getting started`.
    pub headings: Vec<String>,
    /// The words of the section highlighted as the one the reader is in, or an empty
    /// string when none is.
    pub section: String,
    /// What the sidebar says in place of a list — a document with no headings — or an
    /// empty string while it is listing them.
    pub notice: String,
}

/// The search bar as one moment of searching leaves it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Search {
    /// Whether the bar is up at all.
    pub shown: bool,
    /// What the reader has typed into it.
    pub query: String,
    /// The counter beside the entry — `3 of 12`, `No results`, or empty while there is
    /// nothing to count.
    pub counter: String,
    /// What the bar says about having just carried the reader past an end of the
    /// document, or an empty string when the last step did not.
    pub wrap: String,
    /// Whether the case toggle is pressed.
    pub cased: bool,
}

/// The settings store one or more launches share.
///
/// Preferences outlive the application that changed them, so a test about them needs
/// somewhere for them to live that is neither the developer's own settings nor gone
/// the moment the application is. This is that place: a store of one test's own, which
/// a launch is pointed at with [`launch_with`] and which the next launch over the same
/// store still finds.
///
/// It is also where the *desktop's* own settings live for a launch, because GSettings
/// keeps both in the one store: [`Preferences::set_high_contrast`] is how a test says
/// what kind of desktop the application is running on.
pub struct Preferences {
    scratch: Scratch,
}

/// The group GSettings keeps the desktop's accessibility settings under. libadwaita
/// reads high contrast from here when there is no settings portal to ask, which is
/// every launch in this harness (probed on libadwaita 1.8.6: the style manager reports
/// it at startup and reports a change to it while the application is running).
const A11Y_GROUP: &str = "org/gnome/desktop/a11y/interface";

impl Preferences {
    /// An empty store — every setting at the value a first run gets.
    pub fn new(label: &str) -> Preferences {
        Preferences {
            scratch: Scratch::new(label),
        }
    }

    /// A store the reader has already been in: `key` is set to `value` before the
    /// application is ever started.
    ///
    /// Written the way GLib's keyfile backend writes it, which is the same file the
    /// dialog produces — `wait_until` reads settings back out of it.
    pub fn with(label: &str, key: &str, value: &str) -> Preferences {
        let preferences = Preferences::new(label);
        let keyfile = preferences.keyfile();
        if let Some(parent) = keyfile.parent() {
            std::fs::create_dir_all(parent)
                .unwrap_or_else(|error| panic!("create {parent:?}: {error}"));
        }
        std::fs::write(&keyfile, format!("[io/github/etf/axiomd]\n{key}={value}\n"))
            .unwrap_or_else(|error| panic!("write {keyfile:?}: {error}"));
        preferences
    }

    /// Puts the desktop this store belongs to into high contrast, or takes it out —
    /// before a launch, or while one is reading.
    ///
    /// The reader never chooses this in axiomd: it is the desktop's accessibility
    /// setting, and this is a test standing in for the reader having set it there.
    /// Everything already in the store is kept, so a launch that has written its own
    /// preferences does not lose them to this.
    pub fn set_high_contrast(&self, high_contrast: bool) {
        let keyfile = self.keyfile();
        if let Some(parent) = keyfile.parent() {
            std::fs::create_dir_all(parent)
                .unwrap_or_else(|error| panic!("create {parent:?}: {error}"));
        }
        let existing = std::fs::read_to_string(&keyfile).unwrap_or_default();
        let mut kept = String::new();
        let mut inside = false;
        for line in existing.lines() {
            if line.starts_with('[') {
                inside = line.trim() == format!("[{A11Y_GROUP}]");
            }
            if !inside {
                kept.push_str(line);
                kept.push('\n');
            }
        }
        std::fs::write(
            &keyfile,
            format!("{kept}[{A11Y_GROUP}]\nhigh-contrast={high_contrast}\n"),
        )
        .unwrap_or_else(|error| panic!("write {keyfile:?}: {error}"));
    }

    /// What the store holds for `key` now, as it is written down, or `None` while the
    /// reader has never changed that setting.
    fn get(&self, key: &str) -> Option<String> {
        let stored = std::fs::read_to_string(self.keyfile()).ok()?;
        stored
            .lines()
            .filter_map(|line| line.split_once('='))
            .find(|(name, _)| name.trim() == key)
            .map(|(_, value)| value.trim().to_owned())
    }

    /// Waits until the store holds `value` for `key`, and fails saying what it holds
    /// instead.
    ///
    /// This is the far side of the preferences dialog: read from the file the
    /// application wrote rather than asked of the application. Settings reach that
    /// file through the main loop, so a test that read it once would be racing the
    /// write it is about to assert.
    pub fn wait_until(&self, key: &str, value: &str) {
        let deadline = Instant::now() + SETTLES_WITHIN;
        loop {
            let stored = self.get(key);
            if stored.as_deref() == Some(value) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "waited {SETTLES_WITHIN:?} for {key} to be {value} in the reader's \
                 settings and it is {stored:?}",
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Where GLib's keyfile backend keeps this store, under the configuration
    /// directory the launch is given.
    fn keyfile(&self) -> PathBuf {
        self.scratch.path().join("glib-2.0/settings/keyfile")
    }
}

/// Starts axiomd showing `document`, the way opening a file from the desktop does.
///
/// Returns once the document is on screen, so a test never has to wait for it.
pub fn launch(document: &Path) -> App {
    let app = App::start(Some(document), None, None);
    app.wait_for_a_rendered_document();
    app
}

/// The same, for an application launched with `--engine <engine>` — the testing flag
/// that reads documents with an engine other than the reader's preference, without
/// writing anything to their settings (issue #17).
pub fn launch_with_engine(document: &Path, engine: &str) -> App {
    let app = App::start(Some(document), None, Some(engine));
    app.wait_for_a_rendered_document();
    app
}

/// The same, for an application whose preferences are `preferences` — a store that
/// outlives it, so a second launch over the same one is the reader coming back.
pub fn launch_with(document: &Path, preferences: &Preferences) -> App {
    let app = App::start(Some(document), Some(preferences), None);
    app.wait_for_a_rendered_document();
    app
}

/// Starts the axiomd **installed as a flatpak on this machine** showing `document`,
/// and drives it exactly as every other launch here is driven (issue #14).
///
/// This is how a packaged axiomd is asserted about rather than assumed about: the
/// application runs in its own sandbox, with the runtime, the libraries and the
/// installed data files of the package — so what the rendered document proves is that
/// the *package* renders, not that this machine's development build does.
/// `docs/TESTING.md` category 3 asks for exactly this in place of somebody installing
/// the flatpak and looking at it.
///
/// Nothing about the sandbox is relaxed except the two ways in a probe has to have:
///
/// * the launch's own directories — the control socket, the pinned settings, the log —
///   and the folder the document is in are handed to the sandbox as `--filesystem`,
///   because a test that cannot reach the application cannot assert anything about it.
///   These are arguments to *this launch*, not permissions of the package: what the
///   package itself is allowed is pinned in `build-aux/flatpak/permissions.pinned` and
///   asserted from the installed application by `packaging.rs`.
/// * the session bus is taken away, for the same reason it is on the host: a
///   single-instance application with a bus to reach would hand its document to a
///   copy the developer already had open.
///
/// Panics with what to run if no flatpak is installed.
pub fn launch_installed_flatpak(document: &Path) -> App {
    let app = App::start_under(Under::InstalledFlatpak, Some(document), None, None);
    app.wait_for_a_rendered_document();
    app
}

/// Starts axiomd with nothing open — a bare launch, which is a new untitled document
/// in edit mode (`ux_decisions.md`).
///
/// Returns once its window exists.
pub fn launch_without_document() -> App {
    let app = App::start(None, None, None);
    app.wait_until_windows(1);
    app
}

/// The same, over a settings store the test controls.
pub fn launch_without_document_with(preferences: &Preferences) -> App {
    let app = App::start(None, Some(preferences), None);
    app.wait_until_windows(1);
    app
}

/// A running axiomd, and everything it needs to run: its display, its private
/// directories, and the channel commands travel over.
///
/// Dropping it shuts all of that down. [`App::close`] does the same thing and reports
/// what — if anything — survived.
pub struct App {
    control: std::cell::RefCell<Control>,
    axiomd: Child,
    socket: PathBuf,
    // Ordering matters: the application and the compositor are killed before the
    // directories they were living in are removed.
    _display: Display,
    scratch: Scratch,
}

impl App {
    fn start(
        document: Option<&Path>,
        preferences: Option<&Preferences>,
        engine: Option<&str>,
    ) -> App {
        App::start_under(Under::BesideThisTest, document, preferences, engine)
    }

    fn start_under(
        under: Under,
        document: Option<&Path>,
        preferences: Option<&Preferences>,
        engine: Option<&str>,
    ) -> App {
        let scratch = Scratch::new("app");
        let display = Display::start(scratch.path());
        let environment = Environment::pin(
            scratch.path(),
            preferences.map(|preferences| preferences.scratch.path()),
        );
        let mut control = Control::listen(scratch.path());
        let socket = control.socket().to_path_buf();
        let log = scratch.path().join("axiomd.log");

        let control_variable = [("AXIOMD_TEST_CONTROL".to_owned(), socket.clone())];
        let mut command = match under {
            Under::BesideThisTest => {
                let mut command = Command::new(binary());
                environment.apply(
                    &mut command,
                    display
                        .wayland()
                        .into_iter()
                        .map(|(name, value)| (name.to_owned(), value))
                        .chain(control_variable),
                );
                command
            }
            Under::InstalledFlatpak => {
                let sandbox = environment.sandbox_arguments(
                    display
                        .wayland_in_a_sandbox()
                        .into_iter()
                        .map(|(name, value)| (name.to_owned(), value))
                        .chain(control_variable),
                );
                sandboxed_command(
                    sandbox,
                    [Some(scratch.path()), document].into_iter().flatten(),
                )
            }
        };
        if let Some(engine) = engine {
            command.arg(format!("--engine={engine}"));
        }
        if let Some(document) = document {
            command.arg(document);
        }
        let mut axiomd = command
            .stdin(Stdio::null())
            .stdout(append_to(&log))
            .stderr(append_to(&log))
            .spawn()
            .unwrap_or_else(|error| panic!("start {under}: {error}"));

        let diagnostics = {
            let log = log.clone();
            let compositor = display.diary();
            move || {
                format!(
                    "  axiomd said:\n{}\n  the compositor said:\n{compositor}",
                    std::fs::read_to_string(&log).unwrap_or_default(),
                )
            }
        };
        control.accept(&mut axiomd, diagnostics);

        App {
            control: std::cell::RefCell::new(control),
            axiomd,
            socket,
            _display: display,
            scratch,
        }
    }

    /// Shows `document`, as choosing it in the file chooser would.
    ///
    /// Returns once the window has finished with it — showing the document, or
    /// showing the status page that says why it cannot.
    pub fn open(&self, document: &Path) {
        self.command("open", &document.display().to_string());
        self.wait_for_a_finished_page();
    }

    /// Shows `document` in the addressed window, as choosing it in the file chooser
    /// the window opened does — replacing whatever that window held.
    ///
    /// Returns once the window has finished with it.
    pub fn open_here(&self, document: &Path) {
        self.command("open-here", &document.display().to_string());
        self.wait_for_a_finished_page();
    }

    /// Directs later commands at the window at `index`, counting from the first one
    /// opened. Without this every command addresses the newest window.
    pub fn select_window(&self, index: usize) {
        self.command("select", &index.to_string());
    }

    /// Activates a GTK action by name, exactly as its menu item or accelerator does —
    /// `app.new`, `app.close-window`, `win.…`.
    pub fn activate(&self, action: &str) {
        self.command("activate", action);
    }

    /// Evaluates JavaScript against the rendered document and returns the result as
    /// the string JavaScript would make of it.
    ///
    /// This is the assertion primitive: everything the user reads is in that DOM.
    pub fn dom(&self, javascript: &str) -> String {
        self.command("eval", javascript)
    }

    /// The text of the first element matching `selector`, with a failure that names
    /// the selector when nothing matches.
    pub fn dom_text(&self, selector: &str) -> String {
        let script = format!(
            "(() => {{ const found = document.querySelector({selector:?}); \
             return found === null ? {} : found.textContent; }})()",
            format_args!("{:?}", format!("<no element matches {selector}>"))
        );
        let text = self.dom(&script);
        assert!(
            !text.starts_with("<no element matches "),
            "{text} in the rendered document",
        );
        text
    }

    /// Clicks the first element matching `selector`, as the reader would.
    ///
    /// A real activation of a real element: it goes through the document's own
    /// default action, so a link click reaches the app's navigation policy exactly as
    /// a pointer press does.
    pub fn click(&self, selector: &str) {
        let script = format!(
            "(() => {{ const found = document.querySelector({selector:?}); \
             if (found === null) {{ return 'missing'; }} found.click(); return 'clicked'; }})()"
        );
        assert_eq!(
            self.dom(&script),
            "clicked",
            "nothing matching {selector} to click in the rendered document",
        );
    }

    /// Everything axiomd has handed to this launch's desktop — the addresses it sent
    /// to the browser, and the files it sent to their default handler — in order.
    ///
    /// Read from the far side: the desktop's default handler for this launch is a
    /// recorder of the test's own (see `display.rs`), so this is what a browser would
    /// have been started with rather than what axiomd meant to start one with. Where
    /// it goes after that is the platform's contract (`docs/TESTING.md`, category 2).
    pub fn handed_over(&self) -> Vec<String> {
        std::fs::read_to_string(display::handed_over_log(self.scratch.path()))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    /// The documents folder of the desktop this launch runs on — which is where its
    /// print dialog offers to write a file, and so where a test that presses Print
    /// in that dialog finds what came out.
    ///
    /// It is inside the launch's own scratch and goes away with it, so a printing
    /// test never writes into the developer's home (see `display.rs`).
    pub fn documents_dir(&self) -> PathBuf {
        display::documents_dir(self.scratch.path())
    }

    /// Waits until `javascript` evaluates to something truthy in the document.
    ///
    /// The way to wait for anything the DOM shows, and the reason no test here
    /// sleeps.
    pub fn wait_until(&self, javascript: &str) {
        let script = format!("Boolean({javascript})");
        self.settle(&format!("the document to satisfy {javascript}"), || {
            Ok(self.try_command("eval", &script)? == "true")
        });
    }

    /// How many document windows are open.
    pub fn window_count(&self) -> usize {
        self.property("count").parse().expect("a window count")
    }

    /// How long this launch took to have a document for the reader: from the
    /// application building itself to the first document's own bytes leaving the
    /// `axiomd://` handler.
    ///
    /// The cold-start budget is this number (issue #9). It is asked of the application
    /// rather than timed from out here because only the application knows when it
    /// began: everything a test can see from outside includes starting a compositor
    /// and connecting a socket, neither of which a reader waits for. What it leaves
    /// out is `execve` and the dynamic loader, which is the part of a launch axiomd's
    /// own code has no say in.
    ///
    /// Waits for the answer, because a launch is only asked this once it has a
    /// document and the page it renders reaches the handler on the main loop.
    pub fn startup(&self) -> Duration {
        self.settle("this launch to have served a document", || {
            Ok(!self.try_command("window", "startup")?.is_empty())
        });
        Duration::from_micros(self.property("startup").parse().expect("microseconds"))
    }

    /// What this launch is using now: every process it is made of, and their memory
    /// together.
    ///
    /// The memory budgets are read from here, and so is the wait a test does after
    /// closing windows — a web process going away is not instant, and memory read
    /// before it has gone is memory that was about to be freed.
    pub fn footprint(&self) -> Footprint {
        process::footprint(&self.socket)
    }

    /// Waits until `condition` holds, failing with `what` if it never does.
    ///
    /// For the things a test can see from outside the application — a server having
    /// been asked for something — rather than in its DOM. Like every wait here it is
    /// a condition with a deadline, never a sleep.
    pub fn wait_for(&self, what: &str, condition: impl Fn() -> bool) {
        self.settle(what, || Ok(condition()));
    }

    /// Waits until `wanted` things have reached this launch's desktop.
    pub fn wait_for_handed_over(&self, wanted: usize) {
        self.settle(&format!("{wanted} things handed to the desktop"), || {
            Ok(self.handed_over().len() == wanted)
        });
    }

    /// Waits until exactly `wanted` windows are open.
    pub fn wait_until_windows(&self, wanted: usize) {
        self.settle(&format!("{wanted} windows"), || {
            Ok(self.try_command("window", "count")? == wanted.to_string())
        });
    }

    /// The addressed window's title — what the user reads in the header bar and in
    /// the window list.
    pub fn window_title(&self) -> String {
        self.property("title")
    }

    /// How many loads the addressed window's view has committed.
    ///
    /// A re-render that moves this number is a full-page reload: the flash and the
    /// lost scroll position axiomd exists to avoid.
    pub fn navigation_count(&self) -> u32 {
        self.property("navigations").parse().expect("a load count")
    }

    /// How many pages the addressed window has finished — rendered documents and the
    /// status pages that explain why there is none alike.
    ///
    /// What a debounce is measured with: a burst of saves that produced one more page
    /// than the launch did is a burst that was coalesced.
    pub fn render_count(&self) -> u32 {
        self.property("renders").parse().expect("a page count")
    }

    /// Whether the addressed window is showing a document or the status page that
    /// explains why it is not.
    pub fn showing_document(&self) -> bool {
        self.property("showing") == "document"
    }

    /// The title of the dialog the addressed window is showing, or an empty string
    /// when it is showing none.
    ///
    /// Both halves matter: that `Ctrl+comma` puts the preferences dialog up, and that
    /// opening and reading a document puts nothing up at all (`ux_decisions.md`).
    pub fn visible_dialog(&self) -> String {
        self.property("dialog")
    }

    /// What the preferences row titled `row` says — `true`/`false` for a switch, the
    /// number for a number, the label showing for a choice.
    pub fn preference(&self, row: &str) -> String {
        self.command("preference", row)
    }

    /// Turns the preferences row titled `row` to `value`, as the reader turns it.
    ///
    /// The dialog has to be open, exactly as it does for them. What follows the turn
    /// — the row's binding, the setting, the document restyling — is the
    /// application's own doing and nothing the harness reaches into.
    pub fn set_preference(&self, row: &str, value: &str) {
        self.command("set-preference", &format!("{row}={value}"));
    }

    /// Types `text` where the caret is, exactly as pressing the keys does. The window
    /// has to be in edit mode, exactly as it does for the reader.
    pub fn type_text(&self, text: &str) {
        self.command("type", text);
    }

    /// Which way the addressed window is showing its document: `read` or `edit`.
    pub fn mode(&self) -> String {
        self.property("mode")
    }

    /// Waits until the addressed window is in `wanted` mode.
    ///
    /// Switching from reading to editing asks the page where the reader is before it
    /// moves the caret, so it finishes a turn of the loop after the key press.
    pub fn wait_until_mode(&self, wanted: &str) {
        self.settle(&format!("the window to be in {wanted} mode"), || {
            Ok(self.try_command("window", "mode")? == wanted)
        });
    }

    /// Which markdown engine the addressed window's main menu shows it reading with —
    /// the reader's preference, or the engine this window was switched to.
    pub fn engine(&self) -> String {
        self.property("engine")
    }

    /// Waits until the addressed window is reading with `wanted`.
    pub fn wait_until_engine(&self, wanted: &str) {
        self.settle(&format!("the window to read with {wanted}"), || {
            Ok(self.try_command("window", "engine")? == wanted)
        });
    }

    /// Whether the addressed window holds work that is not on disk — what the bullet
    /// in its title says, and what closing it would ask about.
    pub fn is_modified(&self) -> bool {
        self.property("modified") == "true"
    }

    /// Waits until the addressed window's document is clean — what an automatic save
    /// leaves behind.
    pub fn wait_until_saved(&self) {
        self.settle("the document to have nothing unsaved in it", || {
            Ok(self.try_command("window", "modified")? == "false")
        });
    }

    /// Puts the caret on `line` in the addressed window's editor, as clicking into
    /// that line does.
    pub fn place_caret(&self, line: u32) {
        self.command("caret", &line.to_string());
    }

    /// Waits until the addressed window's editor holds exactly `wanted`.
    pub fn wait_until_source(&self, wanted: &str) {
        self.settle(&format!("the editor to hold {wanted:?}"), || {
            Ok(self.try_command("window", "source")? == wanted)
        });
    }

    /// The source line the caret is on in the addressed window's editor.
    pub fn caret_line(&self) -> u32 {
        self.property("caret").parse().expect("a source line")
    }

    /// What the reader has in front of them in edit mode.
    pub fn source(&self) -> String {
        self.property("source")
    }

    /// How the editor draws the first occurrence of `text` in the source, in the words
    /// a reader would use for it — `colour=#1c71d8 weight=bold`, and an empty string
    /// when they see it in the editor's ordinary ink.
    ///
    /// The editing half of reading a rendered block's computed style out of the page.
    pub fn source_style(&self, text: &str) -> String {
        self.property(&format!("drawn {text}"))
    }

    /// Answers the Save As chooser with `file`, as the reader picking it does.
    ///
    /// The chooser itself is a native dialog outside the window's widget tree; this is
    /// its far side, and everything after the choice is the application's own doing.
    pub fn save_as(&self, file: &Path) {
        self.command("save-as", &file.display().to_string());
    }

    /// Answers the export chooser with `file`, as the reader picking it does.
    ///
    /// The format is the name: a `.html` file is a standalone page, anything else is
    /// a PDF. Like the Save As chooser this is the far side of a native dialog, and
    /// everything after the choice is the application's own doing — including the
    /// waiting, so a test must wait for the window to say it is done.
    pub fn export_to(&self, file: &Path) {
        self.command("export-to", &file.display().to_string());
    }

    /// Presses the button labelled `label`, wherever the addressed window is showing
    /// it: beside the document, or in a dialog the reader asked for — one inside the
    /// window, or one standing in front of it.
    pub fn press(&self, label: &str) {
        self.command("press", label);
    }

    /// Waits until the addressed window is showing a dialog that says `wanted`.
    ///
    /// What a dialog is called is what the reader reads at the top of it — for an
    /// alert that is its heading, which libadwaita makes the dialog's title.
    pub fn wait_for_dialog_saying(&self, wanted: &str) {
        self.settle(&format!("a dialog saying {wanted:?}"), || {
            Ok(self.try_command("window", "dialog")?.contains(wanted))
        });
    }

    /// The outline sidebar of the addressed window, as the reader sees it.
    ///
    /// One question rather than four, because the four are only ever meaningful
    /// together: a section highlighted in a sidebar nobody can see is not a highlight,
    /// and a notice standing where a list of headings would be is the same panel
    /// saying something else.
    pub fn outline(&self) -> Outline {
        Outline {
            shown: self.property("outline-shown") == "true",
            headings: self
                .property("outline")
                .lines()
                .map(str::to_owned)
                .collect(),
            section: self.property("outline-section"),
            notice: self.property("outline-notice"),
        }
    }

    /// Waits until the outline highlights `wanted`, and fails saying what it
    /// highlights instead.
    ///
    /// Where the reader is travels from the page to the sidebar over the message
    /// bridge, on the frame in which it changed, so a test that read it once would be
    /// racing the frame it is about to assert.
    pub fn wait_until_section(&self, wanted: &str) {
        self.settle(&format!("the outline to highlight {wanted:?}"), || {
            Ok(self.try_command("window", "outline-section")? == wanted)
        });
    }

    /// How many times the page has told the addressed window which section the reader
    /// is in.
    ///
    /// The bridge promises at most one of these per frame however far the reader
    /// scrolls in it — a jump across a whole document is one message and not one per
    /// section it passed — and this is the only place that promise is visible.
    pub fn section_reports(&self) -> u32 {
        self.property("section-reports").parse().expect("a count")
    }

    /// The search bar of the addressed window, as the reader sees it.
    ///
    /// One question rather than five, because the five are only ever meaningful
    /// together: a counter beside a bar nobody can see counts nothing, and what it says
    /// is about the words in the entry beside it.
    pub fn search(&self) -> Search {
        Search {
            shown: self.property("find-shown") == "true",
            query: self.property("find-query"),
            counter: self.property("find-counter"),
            wrap: self.property("find-wrap"),
            cased: self.property("find-cased") == "true",
        }
    }

    /// Types `text` into the search bar, exactly as pressing the keys does. The bar has
    /// to be open, exactly as it does for the reader.
    pub fn search_for(&self, text: &str) {
        self.command("find", text);
    }

    /// Waits until the counter beside the search entry reads `wanted`, and fails saying
    /// what it reads instead.
    ///
    /// The count of a rendered document comes back from the web process, and the entry
    /// waits for the reader to stop typing before it searches at all, so a test that
    /// read the counter once would be racing both.
    pub fn wait_until_counter(&self, wanted: &str) {
        self.settle(&format!("the search to count {wanted:?}"), || {
            Ok(self.try_command("window", "find-counter")? == wanted)
        });
    }

    /// What the source in the editor is showing highlighted, in order, with the match
    /// the reader is on prefixed by `>`.
    ///
    /// The editing half of reading `mark.axiomd-find` out of the rendered page.
    pub fn source_highlights(&self) -> Vec<String> {
        let highlighted = self.property("find-highlights");
        if highlighted.is_empty() {
            return Vec::new();
        }
        highlighted.lines().map(str::to_owned).collect()
    }

    /// The words the editor is showing the reader underlined as misspelled, in the
    /// order they are written.
    pub fn misspelled(&self) -> Vec<String> {
        let marked = self.property("misspelled");
        if marked.is_empty() {
            return Vec::new();
        }
        marked.lines().map(str::to_owned).collect()
    }

    /// Resizes the addressed window, as dragging its edge or tiling it does.
    pub fn resize(&self, width: i32, height: i32) {
        self.command("resize", &format!("{width}x{height}"));
    }

    /// What the addressed window's inline banner says, or an empty string when no
    /// banner is showing.
    ///
    /// The whole of the app's answer to a document that went wrong while it was being
    /// read: it is said beside the document, never in a dialog.
    pub fn banner(&self) -> String {
        self.property("banner")
    }

    /// Waits until the banner is showing and mentions `wanted`, then returns it.
    pub fn wait_for_banner(&self, wanted: &str) -> String {
        self.settle(&format!("a banner mentioning {wanted:?}"), || {
            Ok(self.try_command("window", "banner")?.contains(wanted))
        });
        self.banner()
    }

    /// Waits until no banner is showing.
    pub fn wait_until_no_banner(&self) {
        self.settle("the banner to go away", || {
            Ok(self.try_command("window", "banner")?.is_empty())
        });
    }

    /// How big the addressed window is showing its document, in the words the primary
    /// menu shows the reader — `"100%"`.
    pub fn zoom(&self) -> String {
        self.property("zoom")
    }

    /// Waits until the menu says the document is at `wanted`, and fails saying what it
    /// says instead.
    ///
    /// Zoom reaches the page through the main loop, so a test that read it once would
    /// be racing the step it is about to assert.
    pub fn wait_until_zoom(&self, wanted: &str) {
        self.settle(&format!("the zoom to reach {wanted}"), || {
            Ok(self.try_command("window", "zoom")? == wanted)
        });
    }

    /// A turn of the scroll wheel over the document, with `Ctrl` held.
    ///
    /// A negative `delta` is a turn towards the top of the document, which is the
    /// direction that has meant "bigger" since a wheel had a notch.
    ///
    /// A headless compositor has no pointer and GTK 4 offers no way to inject one, so
    /// this lands in the call the view's own scroll controller makes, with the values
    /// it passes — the same shape as [`App::press`], which emits a button's own
    /// `clicked` rather than moving a pointer onto it (`docs/TESTING.md`).
    pub fn ctrl_scroll(&self, delta: f64) {
        self.command("scroll", &format!("ctrl {delta}"));
    }

    /// The same turn of the wheel without `Ctrl` — the reader reading rather than
    /// resizing.
    pub fn scroll(&self, delta: f64) {
        self.command("scroll", &format!("plain {delta}"));
    }

    /// A pinch over the document, `scale` being how far it has spread since the
    /// gesture began. Lands where the view's own zoom gesture lands, for the same
    /// reason [`App::ctrl_scroll`] does.
    pub fn pinch(&self, scale: f64) {
        self.command("pinch", &scale.to_string());
    }

    /// Closes the addressed window, as its close button does.
    pub fn close_window(&self) {
        self.command("close", "");
    }

    /// Captures the addressed window's rendered document as pixels.
    pub fn screenshot(&self) -> Screenshot {
        self.wait_until_the_page_has_drawn_again();
        let path = self.scratch.path().join("screenshot.png");
        let _ = std::fs::remove_file(&path);
        self.command("screenshot", &path.display().to_string());
        Screenshot::read(&path).unwrap_or_else(|error| panic!("{error}"))
    }

    /// Waits until the page has produced a frame since being asked.
    ///
    /// Every other assertion here is about the DOM, which exists as soon as the
    /// document is patched in. Pixels do not: a capture taken between the two is a
    /// picture of nothing, and it was seen intermittently under a loaded machine as
    /// "the capture is a single colour". The signal is the document timeline, whose
    /// current time is the last frame's — it stands still between frames and moves
    /// when one is produced, so waiting for it to move is waiting for a render, and it
    /// says nothing whatsoever about what was rendered. A test asserting the picture
    /// is not blank still asserts it.
    fn wait_until_the_page_has_drawn_again(&self) {
        let now = "Number(document.timeline.currentTime)";
        let drawn_at: f64 = self.dom(&format!("String({now})")).parse().unwrap_or(0.0);
        self.wait_until(&format!("{now} > {drawn_at}"));
    }

    /// Shuts everything down and reports the processes that outlived it.
    ///
    /// An empty answer is the assertion a teardown test makes: no application, no web
    /// process, no network process is still running once a window is gone.
    pub fn close(mut self) -> Vec<u32> {
        let _ = self.control.borrow_mut().request("quit", "");
        self.control.borrow_mut().hang_up();
        self.wait_for_exit();
        process::launched_with(&self.socket)
    }

    fn wait_for_exit(&mut self) {
        let deadline = Instant::now() + SETTLES_WITHIN;
        while Instant::now() < deadline {
            if self.axiomd.try_wait().expect("poll axiomd").is_some() {
                // The application is gone; its web processes follow it, and the wait
                // below is for them rather than for it.
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let _ = self.axiomd.kill();
        let _ = self.axiomd.wait();

        let socket = self.socket.clone();
        let deadline = Instant::now() + SETTLES_WITHIN;
        while Instant::now() < deadline && !process::launched_with(&socket).is_empty() {
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Waits until the addressed window has finished with what it was asked to show,
    /// whichever way it finished.
    ///
    /// Rendering happens off the main loop, so this is the harness's answer to "has
    /// it caught up yet": the window counts the pages it finishes, and a page that
    /// never arrives fails the test rather than being slept past.
    fn wait_for_a_finished_page(&self) {
        self.settle("a window", || {
            Ok(self.try_command("window", "count")? != "0")
        });
        self.settle("the window to finish a page", || {
            Ok(self.try_command("window", "renders")? != "0")
        });
        if self.showing_document() {
            self.wait_until("document.querySelector('article.markdown') !== null");
            // And until its styling has arrived with it. A stylesheet is render
            // blocking, so a complete document is a laid-out one — where a document
            // whose blocks are merely *in* the DOM is a page half the height the
            // reader will see, and anything measured or scrolled to on it lands
            // somewhere they never were. Seen as a one-in-ten failure of the outline
            // suite under a loaded machine, where the `axiomd://` stylesheet arrives
            // a few milliseconds behind the document it styles.
            self.wait_until("document.readyState === 'complete'");
        }
    }

    /// Waits until the addressed window is showing a rendered document.
    fn wait_for_a_rendered_document(&self) {
        self.wait_for_a_finished_page();
        self.settle("the document to appear", || {
            Ok(self.try_command("window", "showing")? == "document")
        });
        self.wait_until("document.querySelector('article.markdown') !== null");
    }

    fn property(&self, name: &str) -> String {
        self.command("window", name)
    }

    fn command(&self, verb: &str, payload: &str) -> String {
        self.try_command(verb, payload)
            .unwrap_or_else(|complaint| panic!("{verb} {payload:?} failed: {complaint}"))
    }

    fn try_command(&self, verb: &str, payload: &str) -> Result<String, String> {
        self.control.borrow_mut().request(verb, payload)
    }

    /// Polls `condition` until it holds, or fails the test saying what never happened,
    /// what the application last answered, and what it printed.
    fn settle(&self, what: &str, condition: impl Fn() -> Result<bool, String>) {
        let deadline = Instant::now() + SETTLES_WITHIN;
        loop {
            let last = match condition() {
                Ok(true) => return,
                Ok(false) => "the condition was simply not true yet".to_owned(),
                Err(complaint) => complaint,
            };
            if Instant::now() >= deadline {
                panic!(
                    "waited {SETTLES_WITHIN:?} for {what} and it never happened.\n  \
                     last answer: {last}\n  axiomd said:\n{}",
                    std::fs::read_to_string(self.scratch.path().join("axiomd.log"))
                        .unwrap_or_default(),
                );
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = self.control.borrow_mut().request("quit", "");
        self.control.borrow_mut().hang_up();
        self.wait_for_exit();
    }
}

/// Which axiomd a launch drives.
#[derive(Clone, Copy)]
enum Under {
    /// The binary built beside the test running it — every launch but one.
    BesideThisTest,
    /// The flatpak installed on this machine, in its own sandbox.
    InstalledFlatpak,
}

impl std::fmt::Display for Under {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Under::BesideThisTest => write!(out, "{}", binary().display()),
            Under::InstalledFlatpak => write!(out, "the installed flatpak {APP_ID}"),
        }
    }
}

/// `flatpak run`, with the directories the probe needs to be able to see into it and
/// the pinned environment handed to it.
fn sandboxed_command<'a>(
    environment: Vec<String>,
    visible: impl IntoIterator<Item = &'a Path>,
) -> Command {
    assert!(
        Command::new("flatpak")
            .args(["info", APP_ID])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success()),
        "no axiomd flatpak is installed, so there is none to drive.\n  \
         Build and install one, and run the probes that need it, with:\n    \
         ./scripts/quality.d/40-flatpak.sh",
    );

    let mut command = Command::new("flatpak");
    command.arg("run");
    for directory in visible {
        let directory = match directory.is_dir() {
            true => directory.to_path_buf(),
            // A document: the sandbox is shown the folder it is in, because a document
            // is rarely alone — the images it references resolve beside it.
            false => directory.parent().unwrap_or(directory).to_path_buf(),
        };
        command.arg(format!("--filesystem={}", directory.display()));
    }
    command.arg("--nosocket=session-bus");
    command.args(environment);
    command.arg(APP_ID);
    command
}

/// The application id the package installs under, which is also the bus name a launch
/// would claim if it had a bus (see [`launch_installed_flatpak`]).
const APP_ID: &str = "io.github.etf.axiomd";

/// The `axiomd` this harness drives: the one built beside the test binary running it,
/// so a suite can never test a stale copy from somewhere else on the machine.
fn binary() -> PathBuf {
    let executable = std::env::current_exe().expect("the running test binary");
    let mut directory = executable.parent();
    while let Some(candidate) = directory {
        let binary = candidate.join("axiomd");
        if binary.is_file() {
            return binary;
        }
        directory = candidate.parent().filter(|_| candidate.ends_with("deps"));
    }
    panic!("no axiomd binary beside {}", executable.display());
}

fn append_to(path: &Path) -> Stdio {
    Stdio::from(
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap_or_else(|error| panic!("open {path:?}: {error}")),
    )
}
