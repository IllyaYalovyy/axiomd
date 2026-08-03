//! The headless display the application runs on.
//!
//! # Why weston, and what else was measured
//!
//! Probed live on this machine (Fedora 43, GTK 4.20.4, libadwaita 1.8.6,
//! WebKitGTK 2.52.5) on 2026-08-02, running the real application and a WebKit
//! capability probe under each candidate:
//!
//! * **`weston --backend=headless --renderer=pixman`** — chosen. A real Wayland
//!   compositor speaking the same xdg-shell the app meets on GNOME, rendering in
//!   software so nothing depends on a GPU. It listens on one Unix socket inside the
//!   runtime directory it is given and opens no network port. Both `evaluate_javascript`
//!   and `webkit_web_view_snapshot` work, and snapshots came back byte-identical
//!   across runs and processes.
//! * **`gtk4-broadwayd`** — works, and needs no extra package (it ships inside GTK
//!   itself). Rejected: it binds a TCP port on every interface — `gtk4-broadwayd :7`
//!   was observed listening on `*:8087` — so every quality-gate run would publish a
//!   live view of the machine's test session on the network. In a project whose
//!   stated policy is zero implicit network that is the wrong default, and the port
//!   is a global resource two concurrent runs would fight over.
//! * **`mutter --headless`** — closest to production, and rejected as the heaviest:
//!   it wants a login session and D-Bus services the gate deliberately does without.
//! * **`xvfb-run` / `Xvfb`** — not installed, and X11 is not the platform axiomd
//!   targets.
//!
//! # Determinism
//!
//! The display is only half of it. The application is also given a private set of
//! XDG directories, settings of its own and a pinned font configuration, so a
//! rendered document does not change because the developer running the suite prefers
//! a different theme, text scale or hinting. Those settings live in memory and die
//! with the launch, unless the test is *about* preferences and gives it a store that
//! outlives it. See [`Environment`].

use std::io::ErrorKind;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The compositor's surface. Larger than the application's 900x700 window so the
/// window is never resized to fit, which would make a screenshot depend on the
/// display rather than on the document.
const SURFACE: (u32, u32) = (1400, 1000);

/// A compositor of one test's own, and the directories the application under it sees.
pub(crate) struct Display {
    weston: Child,
    socket: String,
    runtime_dir: PathBuf,
    log: PathBuf,
}

impl Display {
    /// Starts a compositor whose entire world is `scratch`, and waits until it
    /// accepts clients.
    pub(crate) fn start(scratch: &Path) -> Display {
        let runtime_dir = scratch.join("run");
        make_private_dir(&runtime_dir);
        let socket = "wayland-e2e".to_owned();
        let log = scratch.join("weston.log");

        let weston = Command::new("weston")
            .args([
                "--backend=headless",
                // Software rendering: no GPU, no driver, same pixels everywhere.
                "--renderer=pixman",
                // Never read the developer's weston.ini.
                "--no-config",
                &format!("--socket={socket}"),
                &format!("--width={}", SURFACE.0),
                &format!("--height={}", SURFACE.1),
            ])
            .env("XDG_RUNTIME_DIR", &runtime_dir)
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("DISPLAY")
            .stdin(Stdio::null())
            .stdout(log_file(&log))
            .stderr(log_file(&log))
            .spawn()
            .unwrap_or_else(|error| {
                panic!(
                    "the e2e suite needs a headless compositor and could not start one: \
                     {error}. Install weston (`sudo dnf install weston`) and run again."
                )
            });

        let mut display = Display {
            weston,
            socket,
            runtime_dir,
            log,
        };
        display.wait_until_accepting();
        display
    }

    /// The environment an application launched onto this display needs.
    pub(crate) fn wayland(&self) -> [(&str, PathBuf); 2] {
        [
            ("XDG_RUNTIME_DIR", self.runtime_dir.clone()),
            ("WAYLAND_DISPLAY", PathBuf::from(&self.socket)),
        ]
    }

    /// Blocks until a client can connect, so no test ever races the compositor.
    fn wait_until_accepting(&mut self) {
        let path = self.runtime_dir.join(&self.socket);
        let alive = |weston: &mut Child| weston.try_wait().ok().flatten().is_none();
        let accepting = || match UnixStream::connect(&path) {
            Ok(stream) => {
                let _ = stream.shutdown(Shutdown::Both);
                true
            }
            Err(error) if error.kind() == ErrorKind::NotFound => false,
            // The socket exists but is not listening yet.
            Err(_) => false,
        };

        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if accepting() {
                return;
            }
            if !alive(&mut self.weston) {
                panic!(
                    "the headless compositor exited during startup:\n{}",
                    self.diary()
                );
            }
            if Instant::now() >= deadline {
                panic!(
                    "the headless compositor never accepted a client:\n{}",
                    self.diary()
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// What the compositor said, for a failure message that can be acted on.
    pub(crate) fn diary(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

impl Drop for Display {
    fn drop(&mut self) {
        let _ = self.weston.kill();
        let _ = self.weston.wait();
    }
}

/// Everything besides the display that a rendered document must not depend on.
///
/// Each entry is a source of variation between two machines — or between two
/// developers on one machine — pinned to a single value, so a screenshot that
/// changes means the document changed.
pub(crate) struct Environment {
    values: Vec<(String, PathBuf)>,
}

impl Environment {
    /// Builds the private world an application under test lives in, writing the
    /// configuration files it names into `scratch`.
    ///
    /// `settings` is the store the application keeps the reader's preferences in, for
    /// a test that changes one or launches twice over the same one. Without it the
    /// application gets settings that live in memory and die with it, which is what
    /// every other test wants: a rendered document must not depend on what the last
    /// test preferred.
    pub(crate) fn pin(scratch: &Path, settings: Option<&Path>) -> Environment {
        let config = settings.map_or_else(|| scratch.join("config"), Path::to_path_buf);
        let fonts = scratch.join("fonts.conf");
        let font_cache = scratch.join("fontcache");
        make_private_dir(&config.join("gtk-4.0"));
        make_private_dir(&font_cache);

        // GTK reads these once at startup. Animations off so a screenshot is never
        // taken mid-transition; the font, hinting and subpixel settings pinned
        // because they are what the developer's desktop would otherwise decide.
        std::fs::write(
            config.join("gtk-4.0/settings.ini"),
            "[Settings]\n\
             gtk-enable-animations=0\n\
             gtk-font-name=Cantarell 11\n\
             gtk-xft-antialias=1\n\
             gtk-xft-hinting=1\n\
             gtk-xft-hintstyle=hintslight\n\
             gtk-xft-rgba=none\n\
             gtk-application-prefer-dark-theme=0\n",
        )
        .expect("write the pinned GTK settings");

        // The same three knobs again for the text WebKit lays out, which asks
        // fontconfig rather than GTK. The cache is ours so a stale one cannot answer.
        std::fs::write(
            &fonts,
            format!(
                "<?xml version=\"1.0\"?>\n\
                 <!DOCTYPE fontconfig SYSTEM \"urn:fontconfig:fonts.dtd\">\n\
                 <fontconfig>\n\
                 \x20 <include ignore_missing=\"yes\">/etc/fonts/fonts.conf</include>\n\
                 \x20 <cachedir>{}</cachedir>\n\
                 \x20 <match target=\"font\">\n\
                 \x20   <edit name=\"antialias\" mode=\"assign\"><bool>true</bool></edit>\n\
                 \x20   <edit name=\"hinting\" mode=\"assign\"><bool>true</bool></edit>\n\
                 \x20   <edit name=\"hintstyle\" mode=\"assign\"><const>hintslight</const></edit>\n\
                 \x20   <edit name=\"rgba\" mode=\"assign\"><const>none</const></edit>\n\
                 \x20   <edit name=\"dpi\" mode=\"assign\"><double>96</double></edit>\n\
                 \x20 </match>\n\
                 </fontconfig>\n",
                font_cache.display()
            ),
        )
        .expect("write the pinned font configuration");

        let data = scratch.join("data");
        let cache = scratch.join("cache");
        let state = scratch.join("state");
        for directory in [&data, &cache, &state] {
            make_private_dir(directory);
        }
        pin_the_default_handler(scratch, &config, &data);

        Environment {
            values: [
                ("XDG_CONFIG_HOME", config),
                ("XDG_DATA_HOME", data),
                ("XDG_CACHE_HOME", cache),
                ("XDG_STATE_HOME", state),
                ("FONTCONFIG_FILE", fonts),
                // No dconf either way: the desktop's colour scheme and text scale stay
                // outside. A test that gave a settings store gets the keyfile backend,
                // which keeps the reader's preferences in a file under the pinned
                // configuration directory — so they outlive the application, exactly
                // as they must on a real desktop.
                (
                    "GSETTINGS_BACKEND",
                    PathBuf::from(match settings {
                        Some(_) => "keyfile",
                        None => "memory",
                    }),
                ),
                ("GDK_BACKEND", PathBuf::from("wayland")),
                // Nothing here needs the accessibility bus, and asking for one that
                // is not there is a warning on every launch.
                ("GTK_A11Y", PathBuf::from("none")),
                ("LC_ALL", PathBuf::from("C.UTF-8")),
                ("LANG", PathBuf::from("C.UTF-8")),
                ("TZ", PathBuf::from("UTC")),
            ]
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect(),
        }
    }

    /// Applies the pinned world to a command, along with `extra`.
    pub(crate) fn apply(
        &self,
        command: &mut Command,
        extra: impl IntoIterator<Item = (String, PathBuf)>,
    ) {
        for (name, value) in self.values.iter().cloned().chain(extra) {
            command.env(name, value);
        }
        command.env_remove("DISPLAY");
        // axiomd is single-instance: with a session bus to reach, a second copy hands
        // its document to the developer's already-running axiomd and exits, and the
        // test would drive nothing. Without a bus there is no first copy.
        command.env("DBUS_SESSION_BUS_ADDRESS", "disabled:");
    }
}

/// The file every address and document axiomd hands to the desktop is written into.
pub(crate) fn handed_over_log(scratch: &Path) -> PathBuf {
    scratch.join("handed-over.log")
}

/// Makes this launch's desktop answer for the things axiomd hands it.
///
/// Two reasons, and the second is the important one:
///
/// * a test can assert what actually reached the desktop, which is as far as
///   "opens in the browser" can be automated (`docs/TESTING.md`, category 2 — the
///   dispatch beyond this point is the platform's contract, not axiomd's);
/// * without it, a test that clicks an external link opens the *developer's real
///   browser*, every time the gate runs. `XDG_DATA_DIRS` still names the system's
///   applications, so there is a default handler for `https` on any desktop machine,
///   and a suite that starts one is a suite nobody will run.
///
/// The handler is this launch's own: it lives in the pinned `XDG_CONFIG_HOME` and
/// `XDG_DATA_HOME`, so nothing about the developer's session is read or changed.
fn pin_the_default_handler(scratch: &Path, config: &Path, data: &Path) {
    use std::os::unix::fs::PermissionsExt;

    // A script rather than a command line: a desktop entry's `Exec` is parsed with
    // rules of its own, and a shell one-liner inside it is a quoting puzzle with a
    // silent failure at the end of it.
    let handler = scratch.join("handler.sh");
    std::fs::write(
        &handler,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$1\" >> {}\n",
            handed_over_log(scratch).display(),
        ),
    )
    .expect("write the desktop handler");
    std::fs::set_permissions(&handler, std::fs::Permissions::from_mode(0o700))
        .expect("make the desktop handler runnable");

    let types = [
        "x-scheme-handler/http",
        "x-scheme-handler/https",
        "x-scheme-handler/mailto",
        "application/pdf",
        "application/octet-stream",
        "text/plain",
    ];
    make_private_dir(&data.join("applications"));
    std::fs::write(
        data.join("applications/axiomd-e2e-handler.desktop"),
        format!(
            "[Desktop Entry]\nType=Application\nName=axiomd e2e handler\n\
             Exec={} %u\nNoDisplay=true\nTerminal=false\nMimeType={};\n",
            handler.display(),
            types.join(";"),
        ),
    )
    .expect("write the desktop entry");
    std::fs::write(
        config.join("mimeapps.list"),
        format!(
            "[Default Applications]\n{}\n",
            types
                .iter()
                .map(|kind| format!("{kind}=axiomd-e2e-handler.desktop"))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    )
    .expect("write the default-application list");
}

fn make_private_dir(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    std::fs::create_dir_all(path).unwrap_or_else(|error| panic!("create {path:?}: {error}"));
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
}

fn log_file(path: &Path) -> Stdio {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap_or_else(|error| panic!("open {path:?}: {error}"));
    Stdio::from(file)
}
