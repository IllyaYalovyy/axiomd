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

/// Starts axiomd showing `document`, the way opening a file from the desktop does.
///
/// Returns once the document is on screen, so a test never has to wait for it.
pub fn launch(document: &Path) -> App {
    let app = App::start(Some(document));
    app.wait_for_a_rendered_document();
    app
}

/// Starts axiomd with nothing open — a bare launch, or `Ctrl+N`.
///
/// Returns once its window exists.
pub fn launch_without_document() -> App {
    let app = App::start(None);
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
    fn start(document: Option<&Path>) -> App {
        let scratch = Scratch::new("app");
        let display = Display::start(scratch.path());
        let environment = Environment::pin(scratch.path());
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
        let path = self.scratch.path().join("screenshot.png");
        let _ = std::fs::remove_file(&path);
        self.command("screenshot", &path.display().to_string());
        Screenshot::read(&path).unwrap_or_else(|error| panic!("{error}"))
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
