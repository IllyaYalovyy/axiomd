//! The test-control channel: how an automated test drives the real application.
//!
//! Compiled into every build and inert in all of them but one. Unless
//! `AXIOMD_TEST_CONTROL` names a Unix socket a test is already listening on, nothing
//! in this module runs, nothing is bound, and no capability is granted. When it does,
//! axiomd connects out to that socket and answers commands on the GTK main loop for
//! as long as the test holds the connection open — and quits when the test lets go,
//! so a test that dies cannot leave an application behind.
//!
//! # Why the tests drive this rather than a parallel test app
//!
//! Every command lands in the code path a user's action lands in: `open` goes through
//! [`Shell::show`], the same call a double-click in Files ends in, and `activate`
//! activates the very action a menu item or accelerator activates. A test can only
//! observe what a user could observe: the rendered DOM, the window's title, the
//! number of windows, the pixels on screen.
//!
//! # Reading the document without giving documents a voice
//!
//! DOM queries are evaluated in a named JavaScript world
//! ([`WORLD`]), not in the page's own. That is what lets the assertion primitive
//! exist at all: on WebKitGTK 2.52.5 a document rendered under
//! `enable-javascript = false` and `default-src 'none'` refuses main-world evaluation
//! with "Cannot execute JavaScript in this document", while a named world reads the
//! same DOM and returns the same answers. So the document keeps every restriction the
//! shipping app puts on it — the settings in `window.rs` are not relaxed by a single
//! flag for tests — and the test still sees what the user sees.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use webkit6::prelude::*;

use crate::outline::Browsing;
use crate::shell::Shell;
use crate::window::DocumentWindow;

/// The environment variable that names the test's socket. Absent in every real run.
const CONTROL_SOCKET: &str = "AXIOMD_TEST_CONTROL";

/// The JavaScript world DOM queries run in — never the document's own.
const WORLD: &str = "axiomd-test";

/// Connects `app` to the test that launched it, if a test launched it.
///
/// Returns immediately either way. With no socket named this is the whole of the
/// module's effect on a normal run.
pub(crate) fn arm(app: &adw::Application, shell: &Rc<Shell>) {
    let Some(socket) = std::env::var_os(CONTROL_SOCKET) else {
        return;
    };
    let socket = PathBuf::from(socket);

    // Held so the application outlives its last window: a test closes every window
    // and then still asks how many are left.
    let hold = app.hold();
    let app = app.clone();
    let shell = shell.clone();
    glib::spawn_future_local(async move {
        let session = Session::connect(&socket).await;
        match session {
            Ok(session) => session.serve(&app, &shell).await,
            Err(error) => eprintln!("axiomd: test control socket {socket:?}: {error}"),
        }
        // The test is gone: so is the reason to keep running.
        drop(hold);
        app.quit();
    });
}

/// One test's connection, and the window its commands act on.
struct Session {
    /// Never read. Held because a `GSocketConnection` closes its streams when it is
    /// finalised: letting go of it here would hang up on the test mid-command.
    #[expect(dead_code, reason = "ownership: it keeps the streams below open")]
    connection: gio::SocketConnection,
    incoming: gio::DataInputStream,
    outgoing: gio::OutputStream,
    /// Which window commands address, or `None` for the newest one. Windows arrive
    /// and leave while a test runs, so the newest is the only stable default: it is
    /// the one the command that just ran created.
    selected: Cell<Option<usize>>,
}

/// What a command produced. Both arms travel the same frame; only the status differs,
/// so a test sees a failed command as a failure rather than as an odd-looking value.
type Answer = Result<String, String>;

impl Session {
    async fn connect(socket: &Path) -> Result<Self, glib::Error> {
        let address = gio::UnixSocketAddress::new(socket);
        let connection = gio::SocketClient::new()
            .connect_future(&address.clone().upcast::<gio::SocketAddress>())
            .await?;
        Ok(Self {
            incoming: gio::DataInputStream::new(&connection.input_stream()),
            outgoing: connection.output_stream(),
            connection,
            selected: Cell::new(None),
        })
    }

    /// Answers commands until the test hangs up or asks to quit.
    async fn serve(&self, app: &adw::Application, shell: &Rc<Shell>) {
        while let Some((verb, payload)) = self.next_command().await {
            let quitting = verb == "quit";
            let answer = self.run(&verb, &payload, app, shell).await;
            if self.reply(&answer).await.is_err() || quitting {
                return;
            }
        }
    }

    /// Reads one framed command, or `None` once the test has hung up.
    ///
    /// The frame is `<verb> <byte-count>\n<payload>`: a length rather than a
    /// delimiter, so a payload — a script, a path, a title — never has to be escaped
    /// and can hold anything, newlines included.
    async fn next_command(&self) -> Option<(String, String)> {
        let header = self
            .incoming
            .read_line_future(glib::Priority::DEFAULT)
            .await
            .ok()??;
        let header = String::from_utf8(header.to_vec()).ok()?;
        let (verb, length) = header.trim_end().split_once(' ')?;
        let payload = self.read_exactly(length.parse().ok()?).await?;
        Some((verb.to_owned(), payload))
    }

    async fn read_exactly(&self, length: usize) -> Option<String> {
        if length == 0 {
            return Some(String::new());
        }
        let (payload, read, error) = self
            .incoming
            .read_all_future(vec![0u8; length], glib::Priority::DEFAULT)
            .await
            .ok()?;
        if error.is_some() || read != length {
            return None;
        }
        String::from_utf8(payload).ok()
    }

    async fn reply(&self, answer: &Answer) -> Result<(), glib::Error> {
        let (status, body) = match answer {
            Ok(body) => ("ok", body),
            Err(body) => ("err", body),
        };
        let frame = format!("{status} {}\n{body}", body.len()).into_bytes();
        match self
            .outgoing
            .write_all_future(frame, glib::Priority::DEFAULT)
            .await
        {
            Ok((_, _, Some(error))) | Err((_, error)) => Err(error),
            Ok((_, _, None)) => Ok(()),
        }
    }

    async fn run(
        &self,
        verb: &str,
        payload: &str,
        app: &adw::Application,
        shell: &Rc<Shell>,
    ) -> Answer {
        match verb {
            "open" => {
                shell.show(app, Path::new(payload), None);
                // Commands after an open address the document that was just opened.
                self.selected.set(None);
                Ok(String::new())
            }
            // Stops the pages of documents from being answered for, so that a test can
            // stand in the moment between a window showing its document surface and the
            // first pixel of a document arriving in it (issue #40). Off again, every
            // page asked for in between is answered at once.
            "hold-pages" => {
                shell.hold_pages(match payload {
                    "on" => true,
                    "off" => false,
                    other => return Err(format!("not a hold: {other:?}")),
                });
                Ok(String::new())
            }
            // The other half of opening: the document takes over the window the user
            // is in, which is where `Ctrl+O` and the file chooser end.
            "open-here" => {
                let here = self.target(shell)?;
                shell.show(app, Path::new(payload), Some(&here));
                Ok(String::new())
            }
            "select" => {
                let index = payload
                    .parse::<usize>()
                    .map_err(|_| format!("not a window index: {payload:?}"))?;
                if index >= shell.window_count() {
                    return Err(format!(
                        "no window {index}: {} are open",
                        shell.window_count()
                    ));
                }
                self.selected.set(Some(index));
                Ok(String::new())
            }
            // A detailed action name, so a parameterised action — the engine a window
            // reads with, whose menu items each carry their own target — is activated
            // exactly as pressing its menu item does: `win.engine::pulldown-cmark`.
            "activate" => {
                let (name, target) = gio::Action::parse_detailed_name(payload)
                    .map_err(|error| format!("{payload} is not an action name: {error}"))?;
                WidgetExt::activate_action(self.target(shell)?.window(), &name, target.as_ref())
                    .map(|()| String::new())
                    .map_err(|_| format!("{payload} is not an action this window can activate"))
            }
            "eval" => {
                let webview = self.target(shell)?.webview().clone();
                let value = webview
                    .evaluate_javascript_future(payload, Some(WORLD), None)
                    .await
                    .map_err(|error| error.to_string())?;
                Ok(value.to_str().to_string())
            }
            // Typing, exactly as the reader does it: the insertion the buffer sees is
            // the one a key press makes, so it is undoable, it marks the document
            // modified and it starts the same debounce.
            "type" => {
                let window = self.target(shell)?;
                if window.showing() != "editor" {
                    return Err("the window is not in edit mode, so there is nowhere \
                                to type"
                        .to_owned());
                }
                window.type_text(payload);
                Ok(String::new())
            }
            // Typing into the search bar, as the reader does: what follows — the
            // entry's own delay, the search, the counter — is the application's path
            // and nothing here. The bar has to be open, exactly as it does for them.
            "find" => {
                let window = self.target(shell)?;
                if window.search("find-shown").as_deref() != Some("true") {
                    return Err("the search bar is not open, so there is nowhere \
                                to type"
                        .to_owned());
                }
                window.search_for(payload);
                Ok(String::new())
            }
            // Clicking into a line, as the reader does with the pointer: the same
            // call the mode switch makes to put them where they were reading.
            "caret" => {
                let window = self.target(shell)?;
                let line = payload
                    .parse::<u32>()
                    .map_err(|_| format!("not a source line: {payload:?}"))?;
                window.place_caret(line);
                Ok(String::new())
            }
            // Pressing a button the window is showing — in an inline notice or in a
            // dialog — by the words on it, which is all the reader has to go on.
            "press" => press(&self.target(shell)?, payload),
            // Turning a section's chevron in the outline, as clicking it does: the
            // action `GtkTreeExpander` puts on its own gesture and on `Ctrl+Space`. A
            // section the sidebar is not showing, or one with nothing under it to fold,
            // is a failure rather than a quiet nothing.
            "toggle-section" => match self.target(shell)?.fold_section(payload) {
                true => Ok(String::new()),
                false => Err(format!(
                    "the outline is showing no section called {payload:?} with anything \
                     under it to fold"
                )),
            },
            // The pointer running over the sidebar, and the keyboard cursor walking
            // along it. Neither is the reader choosing a section, and neither may move
            // where the sidebar says they are (issue #42). There is no pointer on a
            // headless compositor, so a crossing arrives as the two things a real one
            // produces — see `Outline::browse`.
            "browse" => {
                let what = match payload.split_once(' ') {
                    Some(("pointer", section)) => Browsing::PointerOn(section),
                    Some(("keyboard", section)) => Browsing::KeyboardOn(section),
                    _ if payload == "away" => Browsing::PointerAway,
                    _ => return Err(format!("not a way of browsing the outline: {payload:?}")),
                };
                match self.target(shell)?.browse_outline(what) {
                    true => Ok(String::new()),
                    false => Err(format!(
                        "the outline is showing no section called {payload:?} to browse"
                    )),
                }
            }
            // Pressing one of the window's own controls, found by the name a screen
            // reader announces it as — a control nobody can name is a failure rather
            // than a press that quietly went nowhere.
            "press-control" => match self.target(shell)?.press_control(payload) {
                true => Ok(String::new()),
                false => Err(format!("this window draws no control called {payload:?}")),
            },
            // Pressing a key, spelled the way the application installs it. Answers
            // whether the key did anything, so a key bound to a disabled action is
            // told apart from one that ran.
            "key" => {
                let window = self.target(shell)?;
                match window.press_key(payload) {
                    Some(fired) => Ok(fired.to_string()),
                    None => Err(format!("this window has nothing on {payload}")),
                }
            }
            // The far side of the Save As chooser: the path the reader picked. This
            // lands in the very call the chooser's own callback makes, so everything
            // after the choice — the atomic write, the window following its new file,
            // the title — is the application's own path. The chooser itself is a
            // native dialog outside the window's widget tree and is the one thing here
            // a test cannot press (`docs/TESTING.md`).
            "save-as" => {
                self.target(shell)?.save_to(Path::new(payload));
                Ok(String::new())
            }
            // The far side of the export chooser, for the same reason: the path the
            // reader picked, landing in the very call the chooser's callback makes.
            // Everything after it — which format the name means, the worker, the
            // print job to a file, what the window says while it happens — is the
            // application's own path.
            "export-to" => {
                self.target(shell)?.export_to(Path::new(payload));
                Ok(String::new())
            }
            // Choosing a printer in the print dialog's own list. GTK draws that list
            // inside the dialog rather than in this window, and a headless compositor
            // has no pointer to click a row of it with, so the choice lands where the
            // dialog would leave it — see `export::Printing::choose`. Everything after
            // it is the application's own path: the dialog opens on that printer and
            // the reader presses Print in it.
            "printer" => {
                self.target(shell)?.choose_printer(payload);
                Ok(String::new())
            }
            // The wheel and the touchpad over the document. A headless compositor has
            // no pointer and no touchpad, and GTK 4 offers no way to inject one, so
            // these land in the calls the view's own scroll controller and zoom
            // gesture make, with the values they pass (`zoom.rs`) — the same shape as
            // `press`, which emits a button's own `clicked` rather than moving a
            // pointer onto it.
            "scroll" => {
                let (modifier, delta) = payload
                    .split_once(' ')
                    .ok_or_else(|| format!("not a <modifier> <delta> scroll: {payload:?}"))?;
                let control = match modifier {
                    "ctrl" => true,
                    "plain" => false,
                    other => return Err(format!("not a scroll modifier: {other:?}")),
                };
                let delta = delta
                    .parse::<f64>()
                    .map_err(|_| format!("not a scroll delta: {delta:?}"))?;
                self.target(shell)?.scroll_over_document(delta, control);
                Ok(String::new())
            }
            // The divider between the outline and the document: a drag of it, and the
            // double click that puts it back. Same reason as `scroll` and `pinch` —
            // there is no pointer to move — so these emit the divider's own gesture
            // signals, which is what a press, a move and a release emit (`outline.rs`).
            "divider" => {
                let window = self.target(shell)?;
                if payload == "restore" {
                    window.restore_divider();
                } else if let Some(across) = payload.strip_prefix("drag ") {
                    window.drag_divider(
                        across
                            .parse::<f64>()
                            .map_err(|_| format!("not a drag distance: {across:?}"))?,
                    );
                } else {
                    return Err(format!("not a divider gesture: {payload:?}"));
                }
                Ok(String::new())
            }
            "pinch" => {
                let scale = payload
                    .parse::<f64>()
                    .map_err(|_| format!("not a pinch scale: {payload:?}"))?;
                self.target(shell)?.pinch_over_document(scale);
                Ok(String::new())
            }
            // Resizing the window, as dragging its edge or tiling it does. The width is
            // what decides whether the outline sits beside a document or overlays it,
            // and a breakpoint is only ever exercised by a window that really is that
            // wide.
            "resize" => {
                let (width, height) = payload
                    .split_once('x')
                    .ok_or_else(|| format!("not a <width>x<height> size: {payload:?}"))?;
                let (width, height) = (
                    width
                        .parse::<i32>()
                        .map_err(|_| format!("not a width: {width:?}"))?,
                    height
                        .parse::<i32>()
                        .map_err(|_| format!("not a height: {height:?}"))?,
                );
                self.target(shell)?.window().set_default_size(width, height);
                Ok(String::new())
            }
            // Maximizing it, as double-clicking its header bar does. The compositor
            // decides when and how big, exactly as it does for the reader, so what
            // follows is waited for rather than assumed.
            "maximize" => {
                self.target(shell)?.window().maximize();
                Ok(String::new())
            }
            "window" => self.property(payload, shell),
            // The two halves of driving the preferences dialog: reading a row, and
            // turning it. Turning it sets the very property the reader's click sets,
            // so what happens next — the binding, the setting, the document
            // restyling — is the application's own path and nothing here.
            "preference" => read_row(&self.target(shell)?, payload),
            // And everything it says at once, which is how the words themselves are
            // held to the way GNOME writes them.
            "preferences" => dialog_said(&self.target(shell)?),
            "set-preference" => {
                let (title, value) = payload
                    .split_once('=')
                    .ok_or_else(|| format!("not a <row>=<value> setting: {payload:?}"))?;
                set_row(&self.target(shell)?, title, value)
            }
            // A picture of one part of the window: the rendered document, or the strip
            // of controls above it. Two painters, because they are two different
            // things — the page is what the web process last drew, the header is drawn
            // afresh by the window's own renderer when it is asked for.
            "screenshot" => {
                let (part, path) = payload
                    .split_once(' ')
                    .ok_or_else(|| format!("not a <part> <file> capture: {payload:?}"))?;
                match part {
                    "document" => {
                        let webview = self.target(shell)?.webview().clone();
                        let picture = webview
                            .snapshot_future(
                                webkit6::SnapshotRegion::Visible,
                                webkit6::SnapshotOptions::NONE,
                            )
                            .await
                            .map_err(|error| error.to_string())?;
                        picture
                            .save_to_png(path)
                            .map(|()| String::new())
                            .map_err(|error| error.to_string())
                    }
                    // The pane the document goes in, as the *window* draws it rather
                    // than as WebKit describes the page: the picture above is the web
                    // process's own answer about its own page, and the frame issue #41
                    // is about is one no page has been drawn into yet. This is the only
                    // capture that can show it, because it is taken from the scene the
                    // window presents — the webview, and whatever stands in front of it.
                    "pane" => {
                        let pane = self.target(shell)?.pane().clone();
                        drawn(&pane).await;
                        capture(&pane, path)
                    }
                    "header" => {
                        let header = self.target(shell)?.header().clone();
                        drawn(&header).await;
                        capture(&header, path)
                    }
                    "sidebar" => {
                        let sidebar = self.target(shell)?.sidebar().clone();
                        drawn(&sidebar).await;
                        capture(&sidebar, path)
                    }
                    // The window entire — the header bar, the outline and the document
                    // in one picture, which is what a person sees and what a software
                    // centre shows (issue #33).
                    "window" => {
                        let window = self.target(shell)?.window().clone();
                        let widget: gtk::Widget = window.upcast();
                        drawn(&widget).await;
                        capture_whole(&widget, path)
                    }
                    other => Err(format!("no such part to capture: {other}")),
                }
            }
            "close" => {
                self.target(shell)?.window().close();
                self.selected.set(None);
                Ok(String::new())
            }
            // Where this copy is running, which is how the harness proves a test copy
            // is nowhere near the developer's own session (issue #44).
            "whereabouts" => Ok(whereabouts(app)),
            "quit" => Ok(String::new()),
            other => Err(format!("no such command: {other}")),
        }
    }

    /// Answers one question about the addressed window.
    fn property(&self, name: &str, shell: &Rc<Shell>) -> Answer {
        // The two questions that are about the application rather than about one
        // window, and the only ones answerable when no window is open at all.
        if name == "count" {
            return Ok(shell.window_count().to_string());
        }
        // How long this launch took to have a document, in microseconds — the number
        // the perf harness holds to the cold-start budget (issue #9). Empty while this
        // launch has served no document, which is an answer rather than a failure: a
        // launch with nothing open never starts one.
        if name == "startup" {
            return Ok(shell
                .startup()
                .map(|took| took.as_micros().to_string())
                .unwrap_or_default());
        }
        let window = self.target(shell)?;
        // The outline sidebar, as the reader sees it: whether it is there, what it
        // lists, which section is highlighted, and how often the page has said where
        // the reader is.
        if let Some(answer) = window.outline(name) {
            return Ok(answer);
        }
        // The search bar, as the reader sees it: whether it is up, what is in it, what
        // the counter says, whether the last step wrapped, and what the source shows
        // highlighted.
        if let Some(answer) = window.search(name) {
            return Ok(answer);
        }
        // And how big the document is, in the words the primary menu shows.
        if let Some(answer) = window.zoom(name) {
            return Ok(answer);
        }
        // And how the source itself is drawn in edit mode: what a given piece of it is
        // coloured, and which of its words are underlined as misspelled.
        if let Some(answer) = window.editing(name) {
            return Ok(answer);
        }
        // The main menu: what it offers, and whether it is open.
        if let Some(answer) = window.menu(name) {
            return Ok(answer);
        }
        // The header bar at the width the window happens to be: the title, whether its
        // end is cut off, and the controls still drawn beside it.
        if let Some(answer) = window.header_bar(name) {
            return Ok(answer);
        }
        // And how every control in the window is named, for the reader and for a screen
        // reader — the one rule they are all named by (issue #32).
        if let Some(answer) = window.controls(name) {
            return Ok(answer);
        }
        // And the two dialogs that say what the application is and what its keys do,
        // each empty while the window is not showing it.
        if let Some(answer) = crate::chrome::showing(window.window(), name) {
            return Ok(answer);
        }
        match name {
            // Where the parts of the window that share its width are drawn, which is
            // how a test settles what belongs over the document and what belongs over
            // the whole window.
            "geometry" => Ok(window.geometry()),
            // Whether the window is filling the screen — the other half of how big it
            // is, and the half that outlives it (issue #30).
            "maximized" => Ok(window.window().is_maximized().to_string()),
            "title" => Ok(window.window().title().unwrap_or_default().to_string()),
            "uri" => Ok(window.webview().uri().unwrap_or_default().to_string()),
            "navigations" => Ok(window.navigations().to_string()),
            // What the pane a document is shown in is presenting: the document, or the
            // page it arrives on. The structural half of issue #41 — the webview is
            // never the thing on screen before it has a frame — is this answer.
            "pane" => Ok(window.pane_showing().to_owned()),
            "renders" => Ok(window.renders().to_string()),
            // How many copies of the document have left this window for a printer.
            // The reader counts them in the output tray; a headless run cannot, because
            // its only printer writes a file and a file written twice looks exactly
            // like a file written once (issue #43, the every-page-twice defect).
            "prints" => Ok(window.prints().to_string()),
            "showing" => Ok(window.showing().to_owned()),
            "mode" => Ok(match window.showing() {
                "editor" => "edit".to_owned(),
                _ => "read".to_owned(),
            }),
            // The header-bar switch as the reader and a screen reader meet it. One
            // question rather than four, because a control drawn as one mode while it
            // announces the other is the defect worth catching (issue #28).
            "mode-switch" => Ok(window.mode_switch()),
            "modified" => Ok(window.is_modified().to_string()),
            // Which engine the main menu shows this window reading with — its own
            // choice, or the reader's preference while it has made none.
            "engine" => Ok(window.engine().to_string()),
            "caret" => Ok(window.caret_line().to_string()),
            "source" => Ok(window.editor_text()),
            "banner" => Ok(window.banner()),
            "dialog" => Ok(window.visible_dialog()),
            other => Err(format!("no such window property: {other}")),
        }
    }

    /// The window commands act on: the selected one, or the newest.
    fn target(&self, shell: &Rc<Shell>) -> Result<Rc<DocumentWindow>, String> {
        let count = shell.window_count();
        let index = match self.selected.get() {
            Some(index) => index,
            None => count.checked_sub(1).ok_or("no window is open")?,
        };
        shell
            .window_at(index)
            .ok_or_else(|| format!("window {index} has closed"))
    }
}

/// Where this copy of axiomd is running, in four lines: the compositor it draws on,
/// every other compositor it could have drawn on instead, the session bus it registered
/// with, and the control group the desktop started it in.
///
/// This exists for one assertion, made at every launch of the test harness (issue #44):
/// a copy started by a test must be nowhere the developer can see it — not on their
/// display, not on their session bus, and not in their session's accounting of running
/// applications. The harness compares each line against the world it built for this
/// launch and refuses to hand the launch to a test that does not match, so a containment
/// hole fails a test instead of putting a window on somebody's desktop.
///
/// Every line is read from this process rather than described: the display GDK opened,
/// the sockets that exist beside it, the bus GIO registered on, and the kernel's own
/// answer about this process's control group.
fn whereabouts(app: &adw::Application) -> String {
    let on = display_socket();
    [
        format!("backend {}", backend()),
        format!("display {}", on.display()),
        format!("display-id {}", which_socket(&on)),
        format!(
            "strays {}",
            other_compositors(&on)
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(" "),
        ),
        format!("bus {}", session_bus(app)),
        format!("scope {}", control_group()),
    ]
    .join("\n")
}

/// The kind of display this copy opened — `GdkWaylandDisplay` for every launch this
/// project supports, and the name of whatever else was opened when it is not, which is
/// an answer rather than a panic.
fn backend() -> String {
    gdk::Display::default().map_or_else(
        || "none".to_owned(),
        |display| display.type_().name().to_string(),
    )
}

/// The compositor socket this copy is drawing on, as an absolute path.
///
/// Resolved the way libwayland resolves it (`wl_display_connect(3)`): `WAYLAND_DISPLAY`
/// verbatim when it is absolute, and otherwise a name inside `XDG_RUNTIME_DIR`. GDK is
/// asked for the name rather than the environment, so this is the display the toolkit
/// really opened.
fn display_socket() -> PathBuf {
    let Some(display) = gdk::Display::default() else {
        return PathBuf::from("none");
    };
    let named = PathBuf::from(display.name().as_str());
    if named.is_absolute() {
        return named;
    }
    runtime_dir().join(named)
}

/// Every *other* compositor this copy could have connected to: a `wayland-…` socket in
/// its runtime directory, or in the session's own, that is not the one it is on.
///
/// The point of the question is the fallback nobody sees coming. A sandbox handed the
/// host's Wayland socket has a live desktop one `WAYLAND_DISPLAY` away, so a launch that
/// lost that variable would draw its window on the developer's screen and still pass
/// every assertion about itself. An empty answer is the harness's proof that there was
/// nowhere else to go.
fn other_compositors(on: &Path) -> Vec<PathBuf> {
    use std::os::unix::fs::MetadataExt;

    let here = which_socket(on);
    let session = std::fs::metadata("/proc/self")
        .map(|me| PathBuf::from(format!("/run/user/{}", me.uid())))
        .unwrap_or_default();
    let mut found = Vec::new();
    for directory in [runtime_dir(), session] {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with("wayland-") || name.ends_with(".lock") {
                continue;
            }
            // By the socket itself rather than by its name: a sandbox is handed the
            // harness's compositor bind-mounted under a name of flatpak's choosing, so
            // the same socket is reachable by two paths and neither is a second display.
            if which_socket(&path) != here && !found.contains(&path) {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// Which socket a path names — its filesystem and inode — so that the same socket under
/// two names is recognised as one, and two sockets under one name are told apart.
fn which_socket(path: &Path) -> String {
    use std::os::unix::fs::MetadataExt;

    std::fs::metadata(path).map_or_else(
        |_| "none".to_owned(),
        |socket| format!("{}:{}", socket.dev(), socket.ino()),
    )
}

/// The session bus this copy registered on, named by the process id of the bus daemon
/// itself — or `none` when it registered on no bus at all.
///
/// The daemon's own process is the only thing about a bus that says *which* bus it is.
/// The address cannot: a sandboxed copy is always given the address of the proxy flatpak
/// puts in front of it, whatever that proxy is talking to on the far side (probed on
/// flatpak 1.16.6: the address inside the sandbox is `/run/flatpak/bus` for the
/// developer's session bus and for a private one alike, and the two even present
/// different bus GUIDs from the ones behind them). `GetConnectionUnixProcessID` of the
/// bus's own name travels through the proxy and is answered by the daemon behind it.
fn session_bus(app: &adw::Application) -> String {
    let Some(bus) = app.dbus_connection() else {
        return "none".to_owned();
    };
    let asked = ("org.freedesktop.DBus",).to_variant();
    let answer = bus.call_sync(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "GetConnectionUnixProcessID",
        Some(&asked),
        None,
        gio::DBusCallFlags::NONE,
        5_000,
        gio::Cancellable::NONE,
    );
    match answer
        .ok()
        .and_then(|answer| answer.try_child_value(0))
        .and_then(|pid| pid.get::<u32>())
    {
        Some(pid) => pid.to_string(),
        // A bus that will not say which daemon it is is not a contained bus.
        None => "unknown".to_owned(),
    }
}

/// The control group this copy runs in, by its last component — which is the name of the
/// systemd unit the session started it under.
///
/// A `flatpak run` asks the session's own service manager for a transient scope named
/// after the application, and that scope is precisely what makes a launch *visible* as a
/// running application to the desktop (issue #44: 1,026 of them in a day). A copy under
/// the scope of whatever started the test is a copy the desktop never counted.
fn control_group() -> String {
    std::fs::read_to_string("/proc/self/cgroup")
        .ok()
        .and_then(|said| {
            said.lines()
                .next_back()
                .and_then(|line| line.rsplit('/').next())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "unknown".to_owned())
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// Waits until the window has painted a frame with `widget` in it.
///
/// What a widget looks like changes on the main loop — a section is highlighted, a
/// chevron turned — and reaches the screen on the frame after. A capture taken in
/// between draws the widget as it was, because the list has not restyled its rows yet
/// (probed on GTK 4.20.4: a sidebar captured immediately after the reader left a section
/// still had the pill on it). So a capture waits for a frame first, which is the widget
/// half of what a document capture does with `document.timeline`.
///
/// `begin_updating` is what makes the frame happen at all: a window with nothing moving
/// in it paints only when something asks, and this is the asking.
async fn drawn(widget: &gtk::Widget) {
    let Some(clock) = widget.frame_clock() else {
        return;
    };
    let painted = Rc::new(Cell::new(false));
    let waiting: Rc<std::cell::RefCell<Option<std::task::Waker>>> = Rc::default();
    let handler = clock.connect_after_paint({
        let painted = painted.clone();
        let waiting = waiting.clone();
        move |_| {
            painted.set(true);
            if let Some(waker) = waiting.borrow_mut().take() {
                waker.wake();
            }
        }
    });
    clock.begin_updating();
    std::future::poll_fn(|context| match painted.get() {
        true => std::task::Poll::Ready(()),
        false => {
            *waiting.borrow_mut() = Some(context.waker().clone());
            std::task::Poll::Pending
        }
    })
    .await;
    clock.end_updating();
    clock.disconnect(handler);
}

/// Writes `widget` to `path` as the pixels it is drawn as.
///
/// The widget is asked to draw itself, through its parent, and the picture is rendered
/// by the window's own renderer. Not through a `GtkWidgetPaintable`, which hands back
/// the frame the widget last *painted*: a sidebar captured that way just after the
/// reader's place moved came back showing where they were before (probed on GTK
/// 4.20.4). `gtk_widget_snapshot_child` draws the widget tree as it stands now.
///
/// A window that has not laid the widget out yet is a failure rather than a blank
/// picture, and so is one that has invalidated it and not drawn it again — which is
/// what [`drawn`] above waits for.
fn capture(widget: &gtk::Widget, path: &str) -> Answer {
    let (width, height) = (widget.width(), widget.height());
    if width <= 0 || height <= 0 {
        return Err(format!(
            "the window has not laid this out yet, so it is {width}x{height}",
        ));
    }
    let renderer = widget
        .native()
        .and_then(|native| native.renderer())
        .ok_or("the window has no renderer to draw with")?;
    let parent = widget
        .parent()
        .ok_or("the widget is not in the window, so nothing draws it")?;
    // The snapshot below is drawn in the parent's coordinates, so the part of it the
    // widget is, is where the widget sits inside that parent.
    let bounds = widget
        .compute_bounds(&parent)
        .ok_or("the window has not laid this out yet")?;
    let snapshot = gtk::Snapshot::new();
    parent.snapshot_child(widget, &snapshot);
    let drawn = snapshot
        .to_node()
        .ok_or("the widget has been changed and not drawn again yet")?;
    renderer
        .render_texture(&drawn, Some(&bounds))
        .save_to_png(path)
        .map(|()| String::new())
        .map_err(|error| error.to_string())
}

/// Writes a whole window to `path` as the pixels it is drawn as, background and all.
///
/// [`capture`] above cannot do this: it draws a widget through its parent, and a
/// toplevel has none. Drawing the window's content instead loses the window's own CSS
/// node — the background every widget in it sits on — and what comes back has a
/// transparent strip wherever nothing else painted. In the dark palette that is the
/// whole header bar: its white title and icons drawn on nothing (probed on GTK 4.20.4
/// and libadwaita 1.8.6, where the same window in the light palette merely looked
/// right by accident, its content being white too).
///
/// A `GtkWidgetPaintable` of the window paints the window node and everything under
/// it. It hands back what the widget last painted, which is what [`drawn`] above has
/// just waited for.
fn capture_whole(window: &gtk::Widget, path: &str) -> Answer {
    let (width, height) = (window.width(), window.height());
    if width <= 0 || height <= 0 {
        return Err(format!(
            "the window has not been laid out yet, so it is {width}x{height}",
        ));
    }
    let renderer = window
        .native()
        .and_then(|native| native.renderer())
        .ok_or("the window has no renderer to draw with")?;
    let snapshot = gtk::Snapshot::new();
    gtk::WidgetPaintable::new(Some(window)).snapshot(
        &snapshot,
        f64::from(width),
        f64::from(height),
    );
    let drawn = snapshot
        .to_node()
        .ok_or("the window has been changed and not drawn again yet")?;
    renderer
        .render_texture(
            &drawn,
            Some(&gtk::graphene::Rect::new(
                0.0,
                0.0,
                width as f32,
                height as f32,
            )),
        )
        .save_to_png(path)
        .map(|()| String::new())
        .map_err(|error| error.to_string())
}

/// Presses the thing labelled `label`, wherever in the window it is — a button in the
/// inline notice beside a document, a button in a dialog the reader asked for, or a
/// section in the outline sidebar.
fn press(window: &Rc<DocumentWindow>, label: &str) -> Answer {
    let found = window
        .pressable()
        .iter()
        .find_map(|surface| find_button(surface, label));
    if let Some(button) = found {
        // The button's own signal, which is what a pointer press emits — rather
        // than `gtk_widget_activate`, whose answer depends on whether the widget
        // is focusable and which is not how a button gets pressed.
        button.emit_clicked();
        return Ok(String::new());
    }
    // The sidebar's rows are not buttons: a section is picked by the list's own
    // activation, which is what a single click on a row emits.
    if window.pick_section(label) {
        return Ok(String::new());
    }
    Err(format!(
        "the window is showing nothing labelled {label:?} to press"
    ))
}

/// Searches `widget` and everything under it for a button the reader could press by
/// that name.
fn find_button(widget: &gtk::Widget, label: &str) -> Option<gtk::Button> {
    if let Some(button) = widget.downcast_ref::<gtk::Button>()
        && button
            .label()
            .is_some_and(|shown| shown.replace('_', "") == label)
    {
        return Some(button.clone());
    }
    let mut child = widget.first_child();
    while let Some(candidate) = child {
        if let Some(found) = find_button(&candidate, label) {
            return Some(found);
        }
        child = candidate.next_sibling();
    }
    None
}

/// Every word the preferences dialog says to the reader, in the order it says them:
/// `group<TAB>title<TAB>description` for a heading, `row<TAB>title<TAB>subtitle<TAB>
/// option|option` for a row.
///
/// Read out whole rather than a row at a time, so that the way this dialog is written —
/// header-capitalised titles, subtitles that are sentences, and never an identifier
/// (issue #31) — is asserted of everything in it, including rows added after the test
/// that holds it to that.
fn dialog_said(window: &Rc<DocumentWindow>) -> Answer {
    let dialog = window
        .window()
        .visible_dialog()
        .ok_or("no dialog is open, so there is nothing for it to say")?;
    let mut said = Vec::new();
    say(dialog.upcast_ref::<gtk::Widget>(), &mut said);
    Ok(said.join("\n"))
}

/// Appends what `widget` and everything under it says, depth first.
fn say(widget: &gtk::Widget, said: &mut Vec<String>) {
    if let Some(group) = widget.downcast_ref::<adw::PreferencesGroup>() {
        let (title, description) = (group.title(), group.description().unwrap_or_default());
        // A group with neither is libadwaita's own — the empty box it fills while the
        // reader is searching the dialog. It says nothing, so it says nothing here.
        if !title.is_empty() || !description.is_empty() {
            said.push(format!("group\t{title}\t{description}"));
        }
    } else if let Some(row) = widget.downcast_ref::<adw::PreferencesRow>() {
        let subtitle = row
            .downcast_ref::<adw::ActionRow>()
            .map(|row| row.subtitle().unwrap_or_default().to_string())
            .unwrap_or_default();
        let options = row
            .downcast_ref::<adw::ComboRow>()
            .map(|choice| labels(choice).join("|"))
            .unwrap_or_default();
        said.push(format!("row\t{}\t{subtitle}\t{options}", row.title()));
    }
    let mut child = widget.first_child();
    while let Some(candidate) = child {
        say(&candidate, said);
        child = candidate.next_sibling();
    }
}

/// What a preferences row currently says, as the reader reads it: a switch as
/// `true`/`false`, a number as itself, a choice as the label it is showing.
fn read_row(window: &Rc<DocumentWindow>, title: &str) -> Answer {
    let row = row(window, title)?;
    if let Some(switch) = row.downcast_ref::<adw::SwitchRow>() {
        return Ok(switch.is_active().to_string());
    }
    if let Some(number) = row.downcast_ref::<adw::SpinRow>() {
        return Ok((number.value().round() as i64).to_string());
    }
    if let Some(choice) = row.downcast_ref::<adw::ComboRow>() {
        return Ok(chosen_label(choice));
    }
    // A row that says something rather than doing something — the note standing where
    // a section's rows will be. It is there, and it has nothing to say back.
    Ok(String::new())
}

/// Turns one preferences row to `value`, as the reader turning it does.
fn set_row(window: &Rc<DocumentWindow>, title: &str, value: &str) -> Answer {
    let row = row(window, title)?;
    if let Some(switch) = row.downcast_ref::<adw::SwitchRow>() {
        let wanted = value
            .parse::<bool>()
            .map_err(|_| format!("{title} is a switch and {value:?} is not true or false"))?;
        switch.set_active(wanted);
        return Ok(String::new());
    }
    if let Some(number) = row.downcast_ref::<adw::SpinRow>() {
        let wanted = value
            .parse::<f64>()
            .map_err(|_| format!("{title} is a number and {value:?} is not one"))?;
        number.set_value(wanted);
        return Ok(String::new());
    }
    if let Some(choice) = row.downcast_ref::<adw::ComboRow>() {
        let options = labels(choice);
        let wanted = options
            .iter()
            .position(|label| label == value)
            .ok_or_else(|| format!("{title} does not offer {value:?}, only {options:?}"))?;
        choice.set_selected(wanted as u32);
        return Ok(String::new());
    }
    Err(format!("{title} is not a row a reader can change"))
}

/// The row the reader would point at: the one titled `title` in the dialog the window
/// is showing, wherever in it that row lives.
fn row(window: &Rc<DocumentWindow>, title: &str) -> Result<adw::PreferencesRow, String> {
    let dialog = window
        .window()
        .visible_dialog()
        .ok_or_else(|| format!("no dialog is open, so there is no {title} row"))?;
    find_row(dialog.upcast_ref::<gtk::Widget>(), title)
        .ok_or_else(|| format!("the dialog on screen has no row titled {title:?}"))
}

/// Searches `widget` and everything under it for a preferences row with that title.
fn find_row(widget: &gtk::Widget, title: &str) -> Option<adw::PreferencesRow> {
    if let Some(row) = widget.downcast_ref::<adw::PreferencesRow>()
        && row.title() == title
    {
        return Some(row.clone());
    }
    let mut child = widget.first_child();
    while let Some(candidate) = child {
        if let Some(found) = find_row(&candidate, title) {
            return Some(found);
        }
        child = candidate.next_sibling();
    }
    None
}

/// The labels a choice row is offering, in the order it offers them.
fn labels(choice: &adw::ComboRow) -> Vec<String> {
    let Some(model) = choice.model().and_downcast::<gtk::StringList>() else {
        return Vec::new();
    };
    (0..model.n_items())
        .filter_map(|index| model.string(index))
        .map(|label| label.to_string())
        .collect()
}

/// The one it is showing now.
fn chosen_label(choice: &adw::ComboRow) -> String {
    labels(choice)
        .into_iter()
        .nth(choice.selected() as usize)
        .unwrap_or_default()
}
