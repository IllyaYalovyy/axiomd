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

mod control;
mod display;
mod golden;
mod process;
mod scratch;

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub use golden::Screenshot;

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

/// The settings store one or more launches share.
///
/// Preferences outlive the application that changed them, so a test about them needs
/// somewhere for them to live that is neither the developer's own settings nor gone
/// the moment the application is. This is that place: a store of one test's own, which
/// a launch is pointed at with [`launch_with`] and which the next launch over the same
/// store still finds.
pub struct Preferences {
    scratch: Scratch,
}

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
    let app = App::start(Some(document), None);
    app.wait_for_a_rendered_document();
    app
}

/// The same, for an application whose preferences are `preferences` — a store that
/// outlives it, so a second launch over the same one is the reader coming back.
pub fn launch_with(document: &Path, preferences: &Preferences) -> App {
    let app = App::start(Some(document), Some(preferences));
    app.wait_for_a_rendered_document();
    app
}

/// Starts axiomd with nothing open — a bare launch, which is a new untitled document
/// in edit mode (`ux_decisions.md`).
///
/// Returns once its window exists.
pub fn launch_without_document() -> App {
    let app = App::start(None, None);
    app.wait_until_windows(1);
    app
}

/// The same, over a settings store the test controls.
pub fn launch_without_document_with(preferences: &Preferences) -> App {
    let app = App::start(None, Some(preferences));
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
    fn start(document: Option<&Path>, preferences: Option<&Preferences>) -> App {
        let scratch = Scratch::new("app");
        let display = Display::start(scratch.path());
        let environment = Environment::pin(
            scratch.path(),
            preferences.map(|preferences| preferences.scratch.path()),
        );
        let mut control = Control::listen(scratch.path());
        let socket = control.socket().to_path_buf();
        let log = scratch.path().join("axiomd.log");

        let mut command = Command::new(binary());
        environment.apply(
            &mut command,
            display
                .wayland()
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value))
                .chain([("AXIOMD_TEST_CONTROL".to_owned(), socket.clone())]),
        );
        if let Some(document) = document {
            command.arg(document);
        }
        let mut axiomd = command
            .stdin(Stdio::null())
            .stdout(append_to(&log))
            .stderr(append_to(&log))
            .spawn()
            .unwrap_or_else(|error| panic!("start {}: {error}", binary().display()));

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

    /// Answers the Save As chooser with `file`, as the reader picking it does.
    ///
    /// The chooser itself is a native dialog outside the window's widget tree; this is
    /// its far side, and everything after the choice is the application's own doing.
    pub fn save_as(&self, file: &Path) {
        self.command("save-as", &file.display().to_string());
    }

    /// Presses the button labelled `label`, wherever the addressed window is showing
    /// it: beside the document, or in a dialog the reader asked for.
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
