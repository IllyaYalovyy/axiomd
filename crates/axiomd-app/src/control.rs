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
use gtk::gio;
use gtk::glib;
use webkit6::prelude::*;

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
            "activate" => WidgetExt::activate_action(self.target(shell)?.window(), payload, None)
                .map(|()| String::new())
                .map_err(|_| format!("{payload} is not an action this window can activate")),
            "eval" => {
                let webview = self.target(shell)?.webview().clone();
                let value = webview
                    .evaluate_javascript_future(payload, Some(WORLD), None)
                    .await
                    .map_err(|error| error.to_string())?;
                Ok(value.to_str().to_string())
            }
            "window" => self.property(payload, shell),
            "screenshot" => {
                let webview = self.target(shell)?.webview().clone();
                let picture = webview
                    .snapshot_future(
                        webkit6::SnapshotRegion::Visible,
                        webkit6::SnapshotOptions::NONE,
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                picture
                    .save_to_png(payload)
                    .map(|()| String::new())
                    .map_err(|error| error.to_string())
            }
            "close" => {
                self.target(shell)?.window().close();
                self.selected.set(None);
                Ok(String::new())
            }
            "quit" => Ok(String::new()),
            other => Err(format!("no such command: {other}")),
        }
    }

    /// Answers one question about the addressed window.
    fn property(&self, name: &str, shell: &Rc<Shell>) -> Answer {
        // The only question that is about the application rather than about one
        // window, and the only one answerable when no window is open at all.
        if name == "count" {
            return Ok(shell.window_count().to_string());
        }
        let window = self.target(shell)?;
        match name {
            "title" => Ok(window.window().title().unwrap_or_default().to_string()),
            "uri" => Ok(window.webview().uri().unwrap_or_default().to_string()),
            "navigations" => Ok(window.navigations().to_string()),
            "renders" => Ok(window.renders().to_string()),
            "showing" => Ok(window.showing().to_owned()),
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
