//! Keeping what a test starts out of the developer's own session (issue #44).
//!
//! The owner found 1,026 `app-flatpak-io.github.etf.axiomd-*.scope` units started in
//! their desktop session in a day, every one of them a `flatpak run` from a gate run.
//! The ruling: a test must clean up after itself and must never be visible as an
//! instance of the application. This module is the whole of how that is arranged, and
//! how every launch proves it.
//!
//! # The three ways out of a launch, and what closes each
//!
//! * **The display.** A sandbox is handed the harness compositor by absolute path, but
//!   `--socket=wayland` would also mount the *session's* socket beside it — one lost
//!   environment variable away from a window on the developer's screen. [`sandboxed`]
//!   takes that socket away (`--nosocket=wayland`, and the X11 pair with it), so the
//!   harness compositor is not merely the one chosen: it is the only one there is.
//! * **The session bus.** axiomd is single-instance, so a copy that can see the
//!   developer's axiomd owning `io.github.etf.axiomd` hands them its document and exits.
//!   Native launches are pinned to `DBUS_SESSION_BUS_ADDRESS=disabled:` and have no bus
//!   at all; a sandbox cannot be, because flatpak puts a proxy in front of whatever bus
//!   is there and overrides that variable with the proxy's address (probed on flatpak
//!   1.16.6: with the pin set and `--nosocket=session-bus`, a shell in the sandbox still
//!   reached the developer's bus, `GetNameOwner io.github.etf.axiomd` answered with the
//!   owner's running copy, and `RequestName` answered `EXISTS` — which is exactly the
//!   moment GApplication becomes a remote instance and forwards). So a sandboxed launch
//!   is given a [`Session`] of its own: a real `dbus-daemon`, private to the launch,
//!   where the name is free and where the document portal the probes need activates as
//!   itself.
//! * **The session's accounting.** `flatpak run` asks the session's service manager for
//!   a transient scope named after the application, which is what makes a launch visible
//!   as a running app. It does not go through any bus this harness can redirect: the
//!   path `/run/user/<uid>/systemd/private` is built from `getuid()` and connected to
//!   directly (`strings /usr/bin/flatpak`, and confirmed by measurement — neither
//!   `DBUS_SESSION_BUS_ADDRESS` nor `XDG_RUNTIME_DIR` changes it). What does change it is
//!   not being able to reach that socket, so the launch runs in a mount namespace with
//!   an empty directory over it. Probed: no scope is started, and flatpak carries on
//!   without one.
//!
//! # And the proof, at every launch
//!
//! None of the above is trusted. Every launch, of every kind, is asked where it ended up
//! before a test may use it ([`confirm`]), and the answer is checked against the world
//! this harness built for it. A containment hole therefore fails the launch that has it,
//! loudly, instead of quietly putting a window on somebody's desktop.

use std::cell::RefCell;
use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The application id the package installs under, which is also the bus name a launch
/// claims — and the name a scope in the developer's session would be called after.
const APP_ID: &str = "io.github.etf.axiomd";

/// Where a launch ended up: what it is drawing on, what else it could have drawn on, the
/// session bus it registered with, and the session unit it runs under.
///
/// Read from the running application itself rather than from what the harness asked for,
/// which is the only reason it is worth anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Whereabouts {
    /// The kind of display it opened — `GdkWaylandDisplay` for every supported launch.
    pub backend: String,
    /// The compositor socket it is drawing on, as an absolute path.
    pub display: PathBuf,
    /// Which socket that path names — its filesystem and inode. The path alone cannot
    /// say: a sandbox reaches the harness's compositor under a name flatpak chose for
    /// it, and two different compositors can be called the same thing in two different
    /// runtime directories.
    pub display_id: String,
    /// Every other compositor it could have drawn on instead. Empty is the answer that
    /// makes the one above a fact rather than a preference.
    pub strays: Vec<PathBuf>,
    /// The process id of the daemon behind the session bus it registered on, or `None`
    /// when it registered on no bus at all.
    pub bus: Option<u32>,
    /// The control group it runs in, by the name of the session unit — an
    /// `app-flatpak-io.github.etf.axiomd-*.scope` here is the defect issue #44 reports.
    pub scope: String,
}

impl Whereabouts {
    /// Reads what the application answered.
    pub(crate) fn read(said: &str) -> Whereabouts {
        let line = |named: &str| {
            said.lines()
                .find_map(|line| line.strip_prefix(&format!("{named} ")))
                .unwrap_or_default()
                .trim()
                .to_owned()
        };
        Whereabouts {
            backend: line("backend"),
            display: PathBuf::from(line("display")),
            display_id: line("display-id"),
            strays: line("strays")
                .split_whitespace()
                .map(PathBuf::from)
                .collect(),
            bus: line("bus").parse().ok(),
            scope: line("scope"),
        }
    }
}

/// The world this harness built for one launch, which is what its whereabouts have to
/// match.
pub(crate) struct Expected {
    /// The compositor socket this launch's own display listens on, and which socket
    /// that is.
    pub(crate) display: PathBuf,
    pub(crate) display_id: String,
    /// The daemon of the session bus it was given, or `None` for a launch given none.
    pub(crate) bus: Option<u32>,
    /// Whether this launch has a world of its own, in which no other compositor exists.
    ///
    /// True of a sandboxed launch, which is the one that can be *sealed*: it lives in a
    /// filesystem namespace, and [`sandboxed`] leaves no socket in it but the harness's.
    /// False of a launch of the binary itself, which runs in the developer's own
    /// filesystem and can see their session's socket sitting in `/run/user/<uid>`
    /// whether it connects to it or not — for that one, the pinned `WAYLAND_DISPLAY` is
    /// the containment and the display line above is the proof it held.
    pub(crate) alone: bool,
}

/// Checks a launch is where this harness put it, and says exactly what leaked when it is
/// not.
///
/// The message is written for the person who will read it in a failing gate: what got
/// out, which way, and what that means for the desktop the run is happening on.
pub(crate) fn confirm(said: &str, expected: &Expected) -> Result<(), String> {
    let found = Whereabouts::read(said);
    let mut wrong = Vec::new();

    if found.display_id != expected.display_id {
        wrong.push(format!(
            "it is drawing on {} ({}) and this launch's compositor is {} ({}) — a window \
             from this test is on somebody else's screen",
            found.display.display(),
            found.display_id,
            expected.display.display(),
            expected.display_id,
        ));
    }
    if expected.alone && !found.strays.is_empty() {
        wrong.push(format!(
            "it can still reach {} other compositor(s) — {} — so one lost \
             WAYLAND_DISPLAY is a window on the developer's desktop",
            found.strays.len(),
            found
                .strays
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", "),
        ));
    }
    if found.bus != expected.bus {
        wrong.push(format!(
            "it registered on the session bus of {} and this launch was given {} — a \
             copy that can see the developer's axiomd hands them its document",
            found
                .bus
                .map_or_else(|| "no daemon".to_owned(), |pid| format!("process {pid}")),
            expected.bus.map_or_else(
                || "no bus at all".to_owned(),
                |pid| format!("process {pid}")
            ),
        ));
    }
    if found.scope.contains(APP_ID) {
        wrong.push(format!(
            "the session started it as the unit {} — the desktop is counting this test \
             as a running application",
            found.scope,
        ));
    }
    if found.backend != "GdkWaylandDisplay" {
        wrong.push(format!(
            "it opened a {} rather than a Wayland display, so nothing above is a \
             statement about the compositor this harness started",
            found.backend,
        ));
    }

    match wrong.is_empty() {
        true => Ok(()),
        false => Err(wrong.join("\n  ")),
    }
}

thread_local! {
    /// The session this thread's launches happen in, when a test has stood one in for
    /// the developer's own. Empty for every ordinary test, which happens in the
    /// developer's real session — the one every launch has to stay out of.
    static AMBIENT: RefCell<Vec<(String, PathBuf)>> = const { RefCell::new(Vec::new()) };
}

/// A command started in the session this thread is testing in.
///
/// Every launch goes through here, and it is what makes containment testable: a test can
/// put a session of its own — a bus with an axiomd already on it, a compositor with a
/// desktop on it — in the place the developer's session occupies for every other run,
/// and then assert that nothing reached it. Whatever a launch does about containment
/// afterwards overrides these values, which is exactly the point: they are what is left
/// when containment is missing.
pub(crate) fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(program);
    AMBIENT.with_borrow(|values| {
        for (name, value) in values {
            command.env(name, value);
        }
    });
    // Every launch is a tree, and a tree is ended by its group. See [`end`].
    command.process_group(0);
    command
}

/// Stands `values` in for the developer's session on this thread until it is dropped.
pub(crate) fn stand_in_for_the_session(values: Vec<(String, PathBuf)>) -> AmbientSession {
    AMBIENT.with_borrow_mut(|ambient| *ambient = values);
    AmbientSession
}

/// The stand-in, for as long as a test holds it.
pub(crate) struct AmbientSession;

impl Drop for AmbientSession {
    fn drop(&mut self) {
        AMBIENT.with_borrow_mut(Vec::clear);
    }
}

/// Which socket a path names — its filesystem and inode — read the same way the
/// application reads it about the one it is drawing on, so the two answers can be
/// compared across a sandbox boundary.
pub(crate) fn which_socket(path: &Path) -> String {
    use std::os::unix::fs::MetadataExt;

    std::fs::metadata(path).map_or_else(
        |_| "none".to_owned(),
        |socket| format!("{}:{}", socket.dev(), socket.ino()),
    )
}

/// A session bus of one launch's own: a real `dbus-daemon`, and nothing else on it.
///
/// Private for two reasons at once. The name `io.github.etf.axiomd` is free here, so a
/// sandboxed copy is the first instance and never hands its document to the developer's;
/// and the document portal the packaged probes go through activates on this bus as
/// itself — the desktop's own binary, in this launch's own runtime directory — so the
/// route issue #22 tests is the real one rather than a stand-in for it.
pub(crate) struct Session {
    daemon: Child,
    address: String,
}

impl Session {
    /// Starts a bus in `scratch`, whose activated services live in `runtime_dir`.
    ///
    /// The runtime directory matters: the document portal mounts its fuse filesystem at
    /// `$XDG_RUNTIME_DIR/doc`, and the session's own is already taken by the desktop's
    /// portal (probed: a second one there fails with `fusermount3: … Permission
    /// denied`). Inside this launch's scratch it mounts, serves, and goes away with the
    /// bus.
    pub(crate) fn start(scratch: &Path, runtime_dir: &Path) -> Session {
        let socket = scratch.join("bus");
        let configuration = configure(scratch, &socket);
        let mut daemon = Command::new("dbus-daemon")
            .args([
                "--nofork",
                "--nopidfile",
                "--print-address",
                &format!("--config-file={}", configuration.display()),
            ])
            .env("XDG_RUNTIME_DIR", runtime_dir)
            // A group of its own, so [`end`] takes the portal it activates with it.
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|error| {
                panic!(
                    "a sandboxed launch needs a session bus of its own and one could not \
                     be started: {error}. Install dbus-daemon (`sudo dnf install dbus-daemon`)."
                )
            });

        // The daemon prints its address once it is listening, so reading that line is
        // how a launch waits for it — no sleep, and no race with the first client.
        let printed = daemon.stdout.take().expect("the bus daemon's output");
        let mut address = String::new();
        BufReader::new(printed)
            .read_line(&mut address)
            .unwrap_or_else(|error| panic!("read the private bus's address: {error}"));
        let address = address.trim().to_owned();
        assert!(
            address.starts_with("unix:path="),
            "the private bus did not say where it is listening: {address:?}",
        );

        Session { daemon, address }
    }

    /// What to put in `DBUS_SESSION_BUS_ADDRESS` to reach it.
    pub(crate) fn address(&self) -> &str {
        &self.address
    }

    /// The bus daemon's own process, which is the only thing that says *which* bus a
    /// launch ended up on — see [`Whereabouts::bus`].
    pub(crate) fn daemon(&self) -> u32 {
        self.daemon.id()
    }
}

/// Writes the configuration this launch's bus runs under, and returns its path.
///
/// A session bus of the ordinary kind would be no containment at all in one respect: it
/// activates services from the machine's own service directories, and one of those —
/// `org.freedesktop.portal.Flatpak` — spawns sandboxes on request. Everything it spawns
/// registers with the developer's service manager as an instance of the application,
/// because the portal is started by the bus and so lives outside the mount namespace the
/// launch itself is contained by. So this bus can activate exactly one service: the
/// document portal the packaged probes reach their document through (issue #22, #24).
///
/// The policy is the standard session one. What is narrowed is what may be *started*.
fn configure(scratch: &Path, socket: &Path) -> PathBuf {
    let services = scratch.join("bus-services");
    std::fs::create_dir_all(&services).expect("make this launch's service directory");
    // The document portal, and nothing else on the machine. Copied rather than named in
    // a service directory of the system's, because a directory is all-or-nothing.
    let service = "org.freedesktop.portal.Documents.service";
    let from = PathBuf::from("/usr/share/dbus-1/services").join(service);
    if from.is_file() {
        let _ = std::fs::copy(&from, services.join(service));
    }

    let configuration = scratch.join("bus.conf");
    std::fs::write(
        &configuration,
        format!(
            "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\" \
             \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n\
             <busconfig>\n\
             \x20 <type>session</type>\n\
             \x20 <listen>unix:path={socket}</listen>\n\
             \x20 <servicedir>{services}</servicedir>\n\
             \x20 <policy context=\"default\">\n\
             \x20   <allow send_destination=\"*\" eavesdrop=\"true\"/>\n\
             \x20   <allow eavesdrop=\"true\"/>\n\
             \x20   <allow own=\"*\"/>\n\
             \x20 </policy>\n\
             </busconfig>\n",
            socket = socket.display(),
            services = services.display(),
        ),
    )
    .expect("write this launch's bus configuration");
    configuration
}

impl Drop for Session {
    fn drop(&mut self) {
        // The bus's own children — the document portal and its permission store — are in
        // the group it was started in and go with it.
        end(&mut self.daemon);
    }
}

/// `flatpak run` with every way out of the sandbox closed, and no scope in the
/// developer's session.
///
/// `arguments` are the ones the launch itself needs (its pinned environment, the
/// directories it must see); everything this adds is containment.
pub(crate) fn sandboxed(session: &Session, runtime_dir: &Path, display: &str) -> Command {
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

    // A mount namespace whose only difference from the machine's own is that the
    // session's service manager cannot be reached — which is what stops `flatpak run`
    // from registering this launch as a running application. bwrap is flatpak's own
    // sandbox tool and is installed with it, so this costs no new dependency.
    // Through `command` rather than `Command::new`, so that this launch starts in
    // whatever session the test is watching and containment is what takes it out of that
    // session — which is what makes the containment suite's assertions mean anything.
    let mut command = without_a_session_scope("flatpak");
    command.arg("run");
    // Nothing this launch starts may outlive the harness process that started it, even
    // if that process is killed rather than closed.
    command.arg("--die-with-parent");
    // The accessibility bus address is read from the session bus, and a proxy for it is
    // one more thread of contact with the developer's desktop that no probe needs.
    command.arg("--no-a11y-bus");

    // What flatpak itself is looking at, which is what decides what the sandbox will
    // hold. `--socket=wayland` is a pinned permission of the package and stays exactly
    // as it is; what it binds is the compositor *this process* is on, so pointing
    // flatpak at the harness's display makes that display the only one inside the
    // sandbox — the developer's own is not mounted, so it is not merely unchosen but
    // unreachable (probed on flatpak 1.16.6). The bus it proxies and the runtime
    // directory its portals live in are this launch's own for the same reason.
    command.env("WAYLAND_DISPLAY", display);
    command.env("XDG_RUNTIME_DIR", runtime_dir);
    command.env("DBUS_SESSION_BUS_ADDRESS", session.address());
    command
}

/// Runs `script` in the installed sandbox — the package's own `sh`, in the package's own
/// world — and answers with what it did.
///
/// For the probes that ask what a packaged axiomd can *reach* rather than what it does:
/// nothing is granted for the occasion, no `--filesystem`, no socket and no environment,
/// so what this shell can reach is exactly what the application can, which is the only
/// reason its answer is worth anything.
///
/// The one thing that is arranged is the one thing that is not a permission: this run is
/// not registered with the developer's service manager, so their session does not count
/// it as an instance of the application (issue #44). Every `flatpak run` did, which is
/// how a day of gate runs left 1,026 of them behind.
pub fn in_the_installed_sandbox(script: &str) -> std::process::Output {
    let mut command = without_a_session_scope("flatpak");
    command
        .args(["run", "--command=sh", APP_ID, "-c"])
        .arg(script)
        .output()
        .expect(
            "run a shell inside the installed sandbox — install one with \
             ./scripts/quality.d/40-flatpak.sh",
        )
}

/// `program`, in a mount namespace where the session's service manager cannot be
/// reached — which is what stops `flatpak run` from registering the run as an
/// application. Nothing else about the machine is different inside it.
///
/// bwrap is flatpak's own sandbox tool and is installed with it, so this costs no new
/// dependency.
fn without_a_session_scope(program: &str) -> Command {
    let mut command = command("bwrap");
    command.args(["--dev-bind", "/", "/"]);
    command.args(["--tmpfs", &systemd_dir().display().to_string()]);
    command.arg("--");
    command.arg(program);
    command
}

/// The directory holding the socket `flatpak run` asks the session's service manager for
/// a scope over.
///
/// Built from this user's own id because that is how flatpak builds it: the path is
/// `/run/user/<getuid()>/systemd/private`, connected to directly rather than through any
/// bus (`strings /usr/bin/flatpak`, confirmed by measurement).
fn systemd_dir() -> PathBuf {
    use std::os::unix::fs::MetadataExt;

    let uid = std::fs::metadata("/proc/self")
        .map(|me| me.uid())
        .expect("this harness needs /proc to know whose session it is in");
    PathBuf::from(format!("/run/user/{uid}/systemd"))
}

/// Ends a child and everything it started, and waits for it to be gone.
///
/// The group rather than the process: a launch is a tree — a sandbox, a bus daemon, the
/// application, the web process that renders its documents — and killing only the one
/// the harness holds leaves the rest of it running on somebody's machine. Every launch is
/// therefore started in a process group of its own (`Command::process_group`), and this
/// is what that group is for.
pub(crate) fn end(child: &mut Child) {
    let group = child.id() as i32;
    // SAFETY: `kill` on a process group this harness created itself. A negative pid is
    // the group, and the process id of a group leader is its group id.
    unsafe {
        libc::kill(-group, libc::SIGTERM);
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if child.try_wait().ok().flatten().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    // SAFETY: as above. Whatever ignored the polite request is not asked twice.
    unsafe {
        libc::kill(-group, libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}
