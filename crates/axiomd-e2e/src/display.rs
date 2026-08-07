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
use std::os::unix::process::CommandExt;
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
            // A group of its own, so a compositor is ended with everything else one
            // launch is made of rather than left behind by a killed run (issue #44).
            .process_group(0)
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

    /// The environment an application launched onto this display needs — the same two
    /// values in a sandbox as outside one.
    ///
    /// A sandbox used to be handed the socket by absolute path, on the grounds that its
    /// runtime directory is its own. It is not handed one now: `flatpak run` binds the
    /// compositor named by *its own* `WAYLAND_DISPLAY` into the sandbox and nothing
    /// else, so pointing flatpak itself at this display (`containment::sandboxed`) makes
    /// this compositor the only one in there — where an absolute path merely made it the
    /// chosen one, with the developer's session socket mounted beside it (issue #44).
    pub(crate) fn wayland(&self) -> [(&str, PathBuf); 2] {
        [
            ("XDG_RUNTIME_DIR", self.runtime_dir.clone()),
            ("WAYLAND_DISPLAY", PathBuf::from(&self.socket)),
        ]
    }

    /// The socket this compositor listens on, which is what a launch on it must report
    /// itself to be drawing on (see `containment::confirm`).
    pub(crate) fn socket_path(&self) -> PathBuf {
        self.runtime_dir.join(&self.socket)
    }

    /// The name a client in this display's own runtime directory finds it by.
    pub(crate) fn socket_name(&self) -> &str {
        &self.socket
    }

    /// The runtime directory this display's world lives in — also where a sandboxed
    /// launch's own portals are given to live, so everything they mount goes away with
    /// the launch.
    pub(crate) fn runtime_dir(&self) -> &Path {
        &self.runtime_dir
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
        crate::containment::end(&mut self.weston);
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
    /// `settings` is the directory the application keeps the reader's preferences in,
    /// for a test that changes one or launches twice over the same one. Without it
    /// they are kept in this launch's own scratch directory and die with it, which is
    /// what every other test wants: a rendered document must not depend on what the
    /// last test preferred.
    ///
    /// `animations` is the one pinned value a test may ask for the other way, because
    /// it is the difference between two desktops rather than between two machines: a
    /// desktop that animates, and one whose reader asked it not to.
    pub(crate) fn pin(scratch: &Path, settings: Option<&Path>, animations: bool) -> Environment {
        let config = settings.map_or_else(|| scratch.join("config"), Path::to_path_buf);
        let fonts = scratch.join("fonts.conf");
        let font_cache = scratch.join("fontcache");
        make_private_dir(&config.join("gtk-4.0"));
        make_private_dir(&font_cache);

        // GTK reads these once at startup: the font, hinting and subpixel settings
        // pinned because they are what the developer's desktop would otherwise decide.
        // Animations are named here too, but this is not what decides them — see
        // `pin_setting` below.
        //
        // The print backends are pinned for a blunter reason: a suite that prints
        // must not be able to reach the machine's printers. With only the file
        // backend, "Print to File" is the whole of what exists here — a test cannot
        // put paper through the developer's printer however wrong it goes, and
        // nothing asks CUPS anything. Probed on GTK 4.20.4: with this set, printer
        // enumeration answers with `Print to File` alone and printing to a file
        // still works.
        std::fs::write(
            config.join("gtk-4.0/settings.ini"),
            format!(
                "[Settings]\n\
                 gtk-enable-animations={animations}\n\
                 gtk-font-name=Cantarell 11\n\
                 gtk-xft-antialias=1\n\
                 gtk-xft-hinting=1\n\
                 gtk-xft-hintstyle=hintslight\n\
                 gtk-xft-rgba=none\n\
                 gtk-application-prefer-dark-theme=0\n\
                 gtk-print-backends=file\n",
                animations = u8::from(animations),
            ),
        )
        .expect("write the pinned GTK settings");

        // And the one that actually decides whether anything moves. GTK answers
        // `gtk-enable-animations` from the desktop's own setting in preference to the
        // file above, so the file alone left every launch animating (probed on GTK
        // 4.20.4 and libadwaita 1.8.6: with `gtk-enable-animations=0` in `settings.ini`
        // the application still reported animations on, and reported them off the
        // moment this key said so). Off, so a screenshot can never be taken
        // mid-transition; on for the one test that is about a document appearing.
        pin_setting(
            &config,
            DESKTOP_GROUP,
            &format!("enable-animations={animations}"),
        );

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
        // What the application learned rather than what the reader chose: where they
        // left off in each document they have read (issue #51). It goes beside their
        // settings when a test has given the launch a store of its own, for the same
        // reason the settings do — a second launch over the same store is the reader
        // coming back to their own machine, and a place they left is exactly the kind
        // of thing that has to still be there when they do. A launch without a store
        // keeps it in its own scratch, where it dies with the launch.
        let state =
            settings.map_or_else(|| scratch.join("state"), |settings| settings.join("state"));
        for directory in [&data, &cache, &state] {
            make_private_dir(directory);
        }
        pin_the_default_handler(scratch, &config, &data);
        let home = home_dir(scratch);
        make_private_dir(&home);
        pin_the_documents_directory(scratch, &config);

        Environment {
            values: [
                // The launch's own home. Everything the application is told to keep is
                // already pinned below, so this is what is left: the folder GLib
                // answers `g_get_home_dir` with, which the developer's home must never
                // be — a launch that wrote there would write into the person running
                // the suite. It is also what a window shows under a document's name, so
                // the picture of a launch says `~/Documents` rather than a path with
                // this process's id in it (issue #33).
                ("HOME", home),
                ("XDG_CONFIG_HOME", config),
                ("XDG_DATA_HOME", data),
                ("XDG_CACHE_HOME", cache),
                ("XDG_STATE_HOME", state),
                ("FONTCONFIG_FILE", fonts),
                // Never dconf: the developer's own desktop — its colour scheme, its
                // text scale, its accessibility settings — stays outside. The keyfile
                // backend keeps every setting, the reader's preferences and the
                // desktop's own alike, in one file under the pinned configuration
                // directory. A launch given a store shares that directory with the
                // launches before and after it, so preferences outlive the application
                // exactly as they must on a real desktop; a launch without one gets a
                // file of its own that dies with its scratch directory.
                ("GSETTINGS_BACKEND", PathBuf::from("keyfile")),
                ("GDK_BACKEND", PathBuf::from("wayland")),
                // Draw with Cairo rather than with GSK's GL renderer. Not a rendering
                // choice — the application draws the same window either way, and the
                // pictures the golden tests pin come from WebKit's own snapshot rather
                // than from GSK. It is the biggest single source of variation between
                // two machines there is: on a machine with a GPU, GSK's GL renderer
                // compiles its shaders on the GPU and the first frame is nothing; on
                // one without — a headless run, a machine with no card, a container —
                // Mesa falls back to llvmpipe and JIT-compiles them on the CPU. Probed
                // on 2026-08-03 under this compositor by sampling the main thread's
                // stack during a launch: it sits in LLVM's instruction selector, and
                // the GTK main loop does not turn for 762 ms. The same launch with
                // this set takes 424 ms in total. A budget measured through that is a
                // budget on Mesa's shader compiler (issue #9).
                ("GSK_RENDERER", PathBuf::from("cairo")),
                // Nothing here needs the accessibility *bus*, and asking for one that
                // is not there is a warning on every launch — but a test does need
                // what the application tells it, because what a screen reader would
                // announce is a thing the reader is owed (issue #28). `test` is GTK's
                // own answer to exactly that: an accessibility context that keeps
                // everything the application sets and talks to nothing. Probed on GTK
                // 4.20.4: with `none` the context is absent and every accessible name
                // reads back as whatever it was asked about, which is worse than no
                // answer.
                ("GTK_A11Y", PathBuf::from("test")),
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
        for (name, value) in self.pinned(extra) {
            command.env(name, value);
        }
        command.env_remove("DISPLAY");
    }

    /// The same world as `--env=` arguments, for an application that cannot simply
    /// inherit it: `flatpak run` builds the sandbox's environment itself, so every
    /// pinned value has to be handed to it explicitly, and the one value that has to
    /// be *taken away* has to be named (`--unset-env`).
    pub(crate) fn sandbox_arguments(
        &self,
        extra: impl IntoIterator<Item = (String, PathBuf)>,
    ) -> Vec<String> {
        let mut arguments = vec!["--unset-env=DISPLAY".to_owned()];
        for (name, value) in self.pinned(extra) {
            arguments.push(format!("--env={name}={}", value.display()));
        }
        arguments
    }

    fn pinned(&self, extra: impl IntoIterator<Item = (String, PathBuf)>) -> Vec<(String, PathBuf)> {
        self.values
            .iter()
            .cloned()
            // axiomd is single-instance: with a session bus to reach, a second copy
            // hands its document to the developer's already-running axiomd and exits,
            // and the test would drive nothing. Without a bus there is no first copy.
            //
            // Ahead of `extra` rather than after it, so that the one launch that is
            // *meant* to be on a session — the axiomd a `Desktop` stands the developer's
            // own copy in for — can name a bus and be given it. Nothing else ever does,
            // and a launch that names none still ends up with no bus at all.
            .chain([(
                "DBUS_SESSION_BUS_ADDRESS".to_owned(),
                PathBuf::from("disabled:"),
            )])
            .chain(extra)
            .collect()
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

/// Points the desktop's well-known folders at this launch's own scratch.
///
/// The print dialog's file backend offers to write into the documents folder, so
/// without this a test that presses Print in it would drop a PDF in the developer's
/// real `~/Documents` — silently, every time the gate runs. GLib reads the folders
/// from `user-dirs.dirs` under `XDG_CONFIG_HOME`, which is already this launch's own,
/// so naming them here keeps everything a printing test produces inside the scratch
/// that goes away with it.
fn pin_the_documents_directory(scratch: &Path, config: &Path) {
    let documents = documents_dir(scratch);
    make_private_dir(&documents);
    std::fs::write(
        config.join("user-dirs.dirs"),
        format!(
            "XDG_DESKTOP_DIR=\"{documents}\"\n\
             XDG_DOCUMENTS_DIR=\"{documents}\"\n\
             XDG_DOWNLOAD_DIR=\"{documents}\"\n",
            documents = documents.display(),
        ),
    )
    .expect("write the pinned user directories");
}

/// This launch's home: private, inside its scratch, and gone with it.
pub(crate) fn home_dir(scratch: &Path) -> PathBuf {
    scratch.join("home")
}

/// Where this launch's desktop keeps documents — and so where its print dialog
/// offers to write a file.
///
/// Inside the launch's home rather than beside it, as it is on a real desktop: a
/// window showing a document from here says `~/Documents`.
pub(crate) fn documents_dir(scratch: &Path) -> PathBuf {
    home_dir(scratch).join("Documents")
}

/// The GSettings group the desktop keeps its interface settings under — where GTK
/// reads whether anything on screen may move.
pub(crate) const DESKTOP_GROUP: &str = "org/gnome/desktop/interface";

/// The group the desktop keeps its accessibility settings under. libadwaita reads high
/// contrast from here when there is no settings portal to ask, which is every launch in
/// this harness (probed on libadwaita 1.8.6: the style manager reports it at startup
/// and reports a change to it while the application is running).
pub(crate) const A11Y_GROUP: &str = "org/gnome/desktop/a11y/interface";

/// Writes `settings` as the whole of GSettings group `group` in the store under
/// `config`, keeping every other group already in it.
///
/// The one place a desktop setting is said, whether it is said before a launch or
/// while one is reading: GLib's keyfile backend is a file, and a test standing in for
/// the reader's desktop writes to it the way the desktop would.
pub(crate) fn pin_setting(config: &Path, group: &str, settings: &str) {
    let keyfile = config.join("glib-2.0/settings/keyfile");
    if let Some(parent) = keyfile.parent() {
        make_private_dir(parent);
    }
    let existing = std::fs::read_to_string(&keyfile).unwrap_or_default();
    let mut kept = String::new();
    let mut inside = false;
    for line in existing.lines() {
        if line.starts_with('[') {
            inside = line.trim() == format!("[{group}]");
        }
        if !inside {
            kept.push_str(line);
            kept.push('\n');
        }
    }
    std::fs::write(&keyfile, format!("{kept}[{group}]\n{settings}\n"))
        .unwrap_or_else(|error| panic!("write {keyfile:?}: {error}"));
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
