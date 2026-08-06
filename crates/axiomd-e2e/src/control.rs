//! The test's end of the control channel.
//!
//! The test listens first and the application connects to it, rather than the other
//! way round. That is what removes the startup race: accepting the connection *is*
//! the proof that the application reached its main loop, so no wait here is a guess
//! about how long a launch takes.
//!
//! Every read carries a deadline. A hung application fails the test that hung it,
//! with the application's own output in the message, instead of hanging the gate.

use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::crash::Watch;

/// How long the application is allowed to take to answer one command. Far above any
/// real answer; it exists so a wedged application fails rather than hangs.
const ANSWER: Duration = Duration::from_secs(30);

/// How long the application is allowed to take to connect after being launched.
const CONNECT: Duration = Duration::from_secs(30);

/// The listening end of the channel, and then the accepted connection.
pub(crate) struct Control {
    socket: PathBuf,
    listener: UnixListener,
    connection: Option<Connection>,
}

struct Connection {
    incoming: BufReader<UnixStream>,
    outgoing: UnixStream,
}

impl Control {
    /// Listens on a socket inside `scratch` and returns its path, to be handed to the
    /// application as `AXIOMD_TEST_CONTROL`.
    pub(crate) fn listen(scratch: &Path) -> Control {
        let socket = scratch.join("control.sock");
        let listener = UnixListener::bind(&socket)
            .unwrap_or_else(|error| panic!("listen on {socket:?}: {error}"));
        listener
            .set_nonblocking(true)
            .expect("make the control socket pollable");
        Control {
            socket,
            listener,
            connection: None,
        }
    }

    pub(crate) fn socket(&self) -> &Path {
        &self.socket
    }

    /// Waits for the application to connect, or reports why it never did.
    ///
    /// The launch is watched alongside the socket so one that died — a missing library,
    /// an unusable display, a crash in the first frame — fails immediately with the
    /// signal that ended it and its own diagnostics, rather than after the full
    /// deadline (issue #45).
    pub(crate) fn accept(&mut self, axiomd: &mut Watch, diagnostics: impl Fn() -> String) {
        let deadline = Instant::now() + CONNECT;
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    // Accepting from a pollable listener does not make the accepted
                    // connection pollable on Linux, but say so rather than rely on
                    // it: a non-blocking connection would turn every command into an
                    // instant "not ready" and every wait into a timeout.
                    stream
                        .set_nonblocking(false)
                        .expect("take the control connection off polling");
                    stream
                        .set_read_timeout(Some(ANSWER))
                        .expect("bound the wait for an answer");
                    stream
                        .set_write_timeout(Some(ANSWER))
                        .expect("bound the wait to send a command");
                    self.connection = Some(Connection {
                        incoming: BufReader::new(
                            stream.try_clone().expect("split the control connection"),
                        ),
                        outgoing: stream,
                    });
                    return;
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => {}
                Err(error) => panic!("accept on the control socket: {error}"),
            }
            if let Some(complaint) = axiomd.check() {
                panic!(
                    "{complaint}\n  It never connected at all.\n{}",
                    diagnostics()
                );
            }
            if Instant::now() >= deadline {
                panic!(
                    "axiomd never connected to the control socket:\n{}",
                    diagnostics()
                );
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Sends one command and returns what it produced.
    ///
    /// The frame is `<verb> <byte-count>\n<payload>` in both directions: a length
    /// rather than a delimiter, so a script or a path never has to be escaped.
    pub(crate) fn request(&mut self, verb: &str, payload: &str) -> Result<String, String> {
        let Some(connection) = self.connection.as_mut() else {
            return Err(format!("the channel was already closed before {verb}"));
        };

        let frame = format!("{verb} {}\n{payload}", payload.len());
        connection
            .outgoing
            .write_all(frame.as_bytes())
            .map_err(|error| format!("sending {verb}: {error}"))?;
        connection
            .outgoing
            .flush()
            .map_err(|error| format!("sending {verb}: {error}"))?;

        let mut header = String::new();
        match connection.incoming.read_line(&mut header) {
            Ok(0) => return Err(format!("axiomd closed the channel during {verb}")),
            Ok(_) => {}
            Err(error) => return Err(format!("waiting for the answer to {verb}: {error}")),
        }
        let (status, length) = header
            .trim_end()
            .split_once(' ')
            .ok_or_else(|| format!("unreadable answer to {verb}: {header:?}"))?;
        let length: usize = length
            .parse()
            .map_err(|_| format!("unreadable answer length in {header:?}"))?;
        let mut body = vec![0u8; length];
        connection
            .incoming
            .read_exact(&mut body)
            .map_err(|error| format!("reading the answer to {verb}: {error}"))?;
        let body = String::from_utf8(body).map_err(|_| format!("answer to {verb} is not UTF-8"))?;

        match status {
            "ok" => Ok(body),
            "err" => Err(body),
            other => Err(format!("unknown answer status {other:?} to {verb}")),
        }
    }

    /// Lets go of the connection, which is what tells the application to quit.
    pub(crate) fn hang_up(&mut self) {
        self.connection = None;
    }
}
