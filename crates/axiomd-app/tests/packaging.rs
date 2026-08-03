//! The packaged axiomd: what installing it leaves behind, and what the sandbox it
//! runs in is allowed to reach (issue #14).
//!
//! Two halves, and the split is what the quality gate runs them by.
//!
//! The first half needs nothing installed. It reads the files a package is built
//! from — the flatpak manifest, the AppStream metainfo, the icons, `Cargo.lock`'s
//! offline mirror — and the prefix `scripts/install.sh` produces from them. Those
//! are ordinary tests, run by the gate's own `cargo test`, because a broken icon or
//! a widened permission must fail the gate on the machine of whoever wrote it
//! rather than only on the machine that happens to build a flatpak.
//!
//! The second half is `#[ignore]`d: it drives the flatpak actually installed on this
//! machine, so it needs one built and installed first. `scripts/quality.d/40-flatpak.sh`
//! does that and then runs exactly these tests — the probes `docs/TESTING.md`
//! category 3 asks for, in place of somebody opening the app and looking.
//!
//! # The permission set is pinned in one place
//!
//! `build-aux/flatpak/permissions.pinned` is the whole of what axiomd is allowed to
//! reach, and both halves are held to it: the manifest's `finish-args` must add up to
//! it, and the installed application's own `flatpak info --show-permissions` must be
//! it. A permission cannot therefore be widened in the manifest, or by a hand-run
//! `flatpak override`, without a test saying so.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

mod support;

use axiomd_e2e::Fixture;
use support::Origin;

/// The application id, which the manifest, the desktop entry, the metainfo, the icons
/// and the running application all have to agree on.
const APP_ID: &str = "io.github.etf.axiomd";

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repository this test was built from")
}

/// Runs a packaging tool, insisting it is present rather than quietly skipping: a
/// check that silently does not run is a check that is not there. The same rule the
/// desktop-integration suite is written to (`desktop.rs`).
fn tool(name: &str, install_with: &str) -> Command {
    let found = std::env::var_os("PATH")
        .and_then(|path| {
            path.to_str().map(|path| {
                path.split(':')
                    .any(|directory| Path::new(directory).join(name).is_file())
            })
        })
        .unwrap_or(false);
    assert!(
        found,
        "{name} is required to check axiomd's packaging (install it with: {install_with})",
    );
    Command::new(name)
}

/// A directory of this test's own, removed when the test is done with it.
struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "axiomd-packaging-{label}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir_all(&path).unwrap_or_else(|error| panic!("create {path:?}: {error}"));
        Scratch { path }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// axiomd installed into a staging root by `scripts/install.sh`, exactly as the
/// flatpak build installs it — the same script, the same prefix, the real binary.
struct Installed {
    scratch: Scratch,
}

impl Installed {
    fn new(label: &str) -> Installed {
        let scratch = Scratch::new(label);
        let install = Command::new(repository().join("scripts/install.sh"))
            .args(["--prefix", "/app"])
            .arg("--destdir")
            .arg(&scratch.path)
            .arg("--binary")
            .arg(env!("CARGO_BIN_EXE_axiomd"))
            .output()
            .expect("run scripts/install.sh");
        assert!(
            install.status.success(),
            "scripts/install.sh failed:\n{}{}",
            String::from_utf8_lossy(&install.stdout),
            String::from_utf8_lossy(&install.stderr),
        );
        Installed { scratch }
    }

    /// The prefix as the installed application sees it: `/app` inside the staging root.
    fn prefix(&self) -> PathBuf {
        self.scratch.path.join("app")
    }
}

/// The permissions axiomd is allowed, as `flatpak info --show-permissions` prints
/// them, read from the one file that pins them. Its comments — the reason each
/// permission is there, which is the reason to pin them at all — are not part of what
/// flatpak prints, so they are dropped here.
fn pinned_permissions() -> Vec<String> {
    let path = repository().join("build-aux/flatpak/permissions.pinned");
    let pinned =
        std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {path:?}: {error}"));
    permission_lines(&pinned)
}

/// The lines of a permission listing that say something: no comments, no blanks, and
/// not the group header every listing starts with.
fn permission_lines(listing: &str) -> Vec<String> {
    listing
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#') && *line != "[Context]")
        .map(str::to_owned)
        .collect()
}

/// The pinned permissions as `finish-args` — the manifest's way of writing the same
/// thing, derived from the pinned file rather than repeated beside it.
///
/// `shared=network;ipc;` becomes `--share=network`, `--share=ipc`; `sockets=` becomes
/// `--socket=`; `devices=` becomes `--device=`; `filesystems=` becomes
/// `--filesystem=`, of which axiomd has none.
fn pinned_finish_args() -> Vec<String> {
    let mut arguments = Vec::new();
    for line in pinned_permissions() {
        let Some((group, values)) = line.split_once('=') else {
            continue;
        };
        let flag = match group {
            "shared" => "--share",
            "sockets" => "--socket",
            "devices" => "--device",
            "filesystems" => "--filesystem",
            "features" => "--allow",
            other => panic!("the pinned permissions name a group this test cannot read: {other}"),
        };
        for value in values.split(';').filter(|value| !value.is_empty()) {
            arguments.push(format!("{flag}={value}"));
        }
    }
    assert!(
        !arguments.is_empty(),
        "the pinned permissions are empty; a manifest granting nothing would pass every check",
    );

    // The one place the two forms are not the same words. flatpak records
    // `--socket=fallback-x11` as the x11 flag *plus* the fallback flag and decides at
    // launch whether to bind X11 at all, so a pinned listing carrying both is a
    // manifest that asks for fallback-x11 alone (probed; see permissions.pinned). The
    // rule is written down rather than inferred because the difference matters:
    // a manifest that also asked for `--socket=x11` would grant X11 unconditionally,
    // and the printed form cannot tell the two apart.
    if arguments
        .iter()
        .any(|argument| argument == "--socket=fallback-x11")
    {
        arguments.retain(|argument| argument != "--socket=x11");
    }

    arguments
}

/// The flatpak manifest, as text: it is this project's own file and every value in it
/// is a plain string, so the strings are read out of it rather than by way of a JSON
/// dependency the application does not otherwise need. `flatpak-builder`'s own
/// `--show-manifest` is what checks it is valid JSON, in `40-flatpak.sh`.
fn manifest() -> String {
    let path = repository().join("build-aux/flatpak/io.github.etf.axiomd.json");
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {path:?}: {error}"))
}

/// The quoted strings of a top-level array in the manifest — `finish-args`,
/// `sdk-extensions`, `build-commands`.
fn manifest_array(name: &str) -> Vec<String> {
    let manifest = manifest();
    let opening = manifest
        .find(&format!("\"{name}\": ["))
        .unwrap_or_else(|| panic!("the manifest has no {name:?}"));
    let body = &manifest[opening..];
    let closing = body
        .find(']')
        .expect("an unterminated array in the manifest");
    body[..closing]
        .split('"')
        .skip(3)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

/// A `"key": "value"` of the manifest's top level.
fn manifest_value(key: &str) -> String {
    let manifest = manifest();
    let needle = format!("\"{key}\": \"");
    let opening = manifest
        .find(&needle)
        .unwrap_or_else(|| panic!("the manifest has no {key:?}"))
        + needle.len();
    let body = &manifest[opening..];
    let closing = body
        .find('"')
        .expect("an unterminated string in the manifest");
    body[..closing].to_owned()
}

// ---------------------------------------------------------------------------
// What a package is built from — no flatpak needed.
// ---------------------------------------------------------------------------

/// Both icons have to be loadable by the machinery that draws them: the shell, the
/// about dialog and `appstreamcli` all reach an SVG through gdk-pixbuf, and an SVG it
/// will not open is an application with no icon anywhere.
///
/// This is not hypothetical. Both icons were written with their rationale in an XML
/// comment ahead of the root element, and gdk-pixbuf's format detection reads that as
/// "not an image": `appstreamcli compose` refused the icon, which failed the flatpak
/// build outright. Probed on librsvg 2.61.4 / gdk-pixbuf 2.44.4 — the same file with
/// the comment moved inside `<svg>` loads.
#[test]
fn both_icons_are_pictures_the_desktop_can_load() {
    use gtk::gdk_pixbuf::Pixbuf;

    for (icon, size) in [
        (format!("hicolor/scalable/apps/{APP_ID}.svg"), 128),
        (format!("hicolor/symbolic/apps/{APP_ID}-symbolic.svg"), 16),
    ] {
        let path = repository().join("data/icons").join(&icon);
        let picture = Pixbuf::from_file(&path)
            .unwrap_or_else(|error| panic!("{icon} is not a picture anything can draw: {error}"));

        assert_eq!(
            (picture.width(), picture.height()),
            (size, size),
            "{icon} does not draw at the size it declares",
        );
    }
}

/// The metainfo is what a software centre shows and what `flatpak-builder` composes
/// into the package; AppStream's own validator is the authority on it.
///
/// `--no-net`: the gate does not reach the network, so nothing here depends on a
/// screenshot host being up (invariant 6, and a check that needs the network is a
/// check that fails on a train).
#[test]
fn the_metainfo_is_valid_appstream() {
    let output = tool("appstreamcli", "sudo dnf install appstream")
        .args(["validate", "--no-net", "--explain"])
        .arg(repository().join(format!("data/{APP_ID}.metainfo.xml")))
        .output()
        .expect("run appstreamcli validate");

    assert!(
        output.status.success(),
        "appstreamcli is not happy with the metainfo:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// The step that turns an installed prefix into what a software centre reads, and the
/// step the flatpak build ends with: everything installed has to hold together well
/// enough for AppStream to compose a catalogue entry out of it.
///
/// It fails if the metainfo is unreadable, if the desktop entry it points at is
/// missing, or if the icon cannot be rendered — the three ways a package can install
/// successfully and still be a package nothing will show. `--components` makes a
/// dropped entry a failure rather than an empty success: composing with a filter that
/// matches nothing is an error.
#[test]
fn an_installed_prefix_composes_into_an_appstream_catalogue_entry() {
    let installed = Installed::new("compose");
    let out = Scratch::new("composed");

    let output = tool("appstreamcli", "sudo dnf install appstream")
        .arg("compose")
        .arg("--no-net")
        .arg("--prefix=/")
        .arg("--origin=axiomd")
        .arg(format!("--result-root={}", out.path.display()))
        .arg(format!(
            "--data-dir={}",
            out.path.join("catalogue").display()
        ))
        .arg(format!("--icons-dir={}", out.path.join("icons").display()))
        .arg(format!("--components={APP_ID}"))
        .arg("--print-report=full")
        .arg(installed.prefix())
        .output()
        .expect("run appstreamcli compose");

    assert!(
        output.status.success(),
        "the installed prefix does not compose into a catalogue entry:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// `cargo install` places a binary and nothing else. Everything else an installed
/// axiomd needs is placed by `scripts/install.sh`, and the one that bites hardest if
/// it is missing is the compiled settings schema: without it the first preference read
/// aborts the process, so a package that installs cleanly still cannot start.
#[test]
fn installing_leaves_everything_an_installed_axiomd_reads() {
    let installed = Installed::new("prefix");
    let prefix = installed.prefix();

    for file in [
        "bin/axiomd",
        &format!("share/applications/{APP_ID}.desktop"),
        &format!("share/metainfo/{APP_ID}.metainfo.xml"),
        &format!("share/glib-2.0/schemas/{APP_ID}.gschema.xml"),
        "share/glib-2.0/schemas/gschemas.compiled",
        &format!("share/icons/hicolor/scalable/apps/{APP_ID}.svg"),
        &format!("share/icons/hicolor/symbolic/apps/{APP_ID}-symbolic.svg"),
    ] {
        assert!(
            prefix.join(file).is_file(),
            "an installed axiomd has no {file}",
        );
    }

    // The compiled schema has to be the schema installed beside it, not a stale one:
    // this is the file the running application reads its preferences out of.
    let compiled = std::fs::read(prefix.join("share/glib-2.0/schemas/gschemas.compiled"))
        .expect("read the compiled schemas");
    let key = b"reading-width";
    assert!(
        compiled
            .windows(key.len())
            .any(|window| window == key.as_slice()),
        "the compiled schema does not contain axiomd's settings",
    );
}

/// The sandbox reaches no host filesystem — not the home directory, not `host`, not a
/// single `xdg-` directory. Documents arrive through the portal instead.
///
/// Pinned rather than merely asserted about: the manifest's `finish-args` have to add
/// up to exactly `build-aux/flatpak/permissions.pinned`, so widening the sandbox is a
/// failing test and not a diff nobody read.
#[test]
fn the_manifest_grants_exactly_the_pinned_permissions_and_no_filesystem() {
    let mut granted = manifest_array("finish-args");
    let mut pinned = pinned_finish_args();
    granted.sort();
    pinned.sort();

    assert_eq!(
        granted, pinned,
        "the manifest's finish-args are no longer the pinned permission set.\n  \
         Widening the sandbox is a decision recorded in build-aux/flatpak/permissions.pinned,\n  \
         and `--filesystem=host` is ruled out (issue #14).",
    );

    for argument in &granted {
        assert!(
            !argument.starts_with("--filesystem="),
            "the sandbox was given {argument}; axiomd reaches documents through the portal",
        );
    }
}

/// The runtime is pinned, and pinned to the one the code's GTK and libadwaita feature
/// use is written against. Bumping it is one human-approved change that moves the
/// runtime and those pins together (issue #14), so this test exists to make a lone
/// runtime bump fail.
#[test]
fn the_manifest_pins_the_runtime_the_code_is_written_against() {
    assert_eq!(manifest_value("id"), APP_ID);
    assert_eq!(manifest_value("runtime"), "org.gnome.Platform");
    assert_eq!(manifest_value("runtime-version"), "49");
    assert_eq!(manifest_value("sdk"), "org.gnome.Sdk");
    assert_eq!(manifest_value("command"), "axiomd");
    assert_eq!(
        manifest_array("sdk-extensions"),
        vec!["org.freedesktop.Sdk.Extension.rust-stable".to_owned()],
        "the Rust toolchain comes from the SDK extension, whose branch the SDK itself \
         resolves to its freedesktop base",
    );
}

/// A flatpak build has no network, so every crate has to be a source the build
/// downloads beforehand. That mirror is generated from `Cargo.lock` and committed —
/// which means it goes stale the moment a dependency moves, and a stale mirror is a
/// build that fails with a missing crate long after the change that caused it.
#[test]
fn the_offline_cargo_sources_are_the_lock_file_this_build_uses() {
    let repository = repository();
    let generated = tool("python3", "sudo dnf install python3")
        .arg(repository.join("build-aux/flatpak/cargo-sources.py"))
        .arg(repository.join("Cargo.lock"))
        .output()
        .expect("run the cargo sources generator");
    assert!(
        generated.status.success(),
        "the cargo sources generator failed:\n{}",
        String::from_utf8_lossy(&generated.stderr),
    );

    let committed = std::fs::read(repository.join("build-aux/flatpak/cargo-sources.json"))
        .expect("read the committed cargo sources");

    assert!(
        generated.stdout == committed,
        "build-aux/flatpak/cargo-sources.json is not what Cargo.lock generates.\n  \
         Regenerate it:\n    build-aux/flatpak/cargo-sources.py Cargo.lock \
         -o build-aux/flatpak/cargo-sources.json",
    );
}

// ---------------------------------------------------------------------------
// The installed flatpak — probes, run by scripts/quality.d/40-flatpak.sh.
// ---------------------------------------------------------------------------

/// Fails with what to run rather than with a puzzling error, for anyone who runs one
/// of the probes below by hand without an installed flatpak.
fn installed_flatpak() {
    let installed = Command::new("flatpak")
        .args(["info", "--show-permissions", APP_ID])
        .output();
    let present = installed
        .map(|output| output.status.success())
        .unwrap_or(false);
    assert!(
        present,
        "these probes drive the installed flatpak, and there is none installed.\n  \
         Build and install it, and run them, with:\n    ./scripts/quality.d/40-flatpak.sh",
    );
}

/// What the sandbox axiomd actually runs in is allowed to reach — read from the
/// installed application rather than from the manifest it was built from, because a
/// `flatpak override` on this machine would change one and not the other.
#[test]
#[ignore = "drives the installed flatpak; run by scripts/quality.d/40-flatpak.sh"]
fn the_installed_sandbox_has_exactly_the_pinned_permissions() {
    installed_flatpak();

    let output = Command::new("flatpak")
        .args(["info", "--show-permissions", APP_ID])
        .output()
        .expect("run flatpak info --show-permissions");
    let shown = String::from_utf8(output.stdout).expect("flatpak prints utf-8");

    assert_eq!(
        permission_lines(&shown),
        pinned_permissions(),
        "the installed sandbox's permissions are not the pinned ones. \
         `flatpak info --show-permissions {APP_ID}` says:\n{shown}",
    );
    assert!(
        !shown.contains("filesystems="),
        "the installed sandbox can reach the host filesystem:\n{shown}",
    );
}

/// The probe `docs/TESTING.md` category 3 asks for, and the exit criterion of issue
/// #14: the packaged application, in its own sandbox, opens a document and renders it
/// — and a document full of remote images reaches the network only when the reader
/// presses one.
///
/// The counting is done by being the server (`support::Origin`), which is the only
/// honest way to assert an absence of requests. The origin listens on this machine's
/// loopback, which the sandbox shares because `--share=network` is what the placeholder
/// button needs; if that grant were ever used implicitly, this test sees it.
#[test]
#[ignore = "drives the installed flatpak; run by scripts/quality.d/40-flatpak.sh"]
fn the_installed_flatpak_renders_a_document_and_fetches_nothing_until_asked() {
    installed_flatpak();

    let origin = Origin::start();
    let fixture = Fixture::new("flatpak-sandbox");
    let document = fixture.write(
        "sandboxed.md",
        &format!(
            "# Sandboxed\n\nA paragraph.\n\n![a diagram]({})\n",
            origin.url("/diagram.png"),
        ),
    );

    let app = axiomd_e2e::launch_installed_flatpak(&document);

    assert_eq!(
        app.dom_text("h1"),
        "Sandboxed",
        "the packaged application did not render the document",
    );
    assert_eq!(
        app.dom("document.querySelectorAll('a.remote-image').length"),
        "1",
        "the remote image is not a placeholder card in the sandbox",
    );
    assert_eq!(
        origin.requests(),
        Vec::<String>::new(),
        "the sandboxed application reached the network without being asked",
    );

    app.click("a.remote-image");
    app.wait_until("document.querySelectorAll('img').length === 1");

    assert_eq!(
        origin.requests(),
        vec!["/diagram.png".to_owned()],
        "pressing the placeholder in the sandbox did not load the image the reader asked for",
    );
    assert_eq!(
        app.dom("document.querySelector('img').naturalWidth"),
        "40",
        "the image reached the page from inside the sandbox but never decoded",
    );
}

/// UT-001 for the packaged application, as far as the boundary in `docs/TESTING.md`
/// category 2 goes: with the flatpak installed, the desktop's own answer to "what
/// opens Markdown" is axiomd.
///
/// Asked of the flatpak installation's exported entries alone, never of the session's
/// own configuration: the suite must not depend on — or change — what the machine
/// running it opens Markdown with.
#[test]
#[ignore = "drives the installed flatpak; run by scripts/quality.d/40-flatpak.sh"]
fn the_installed_flatpak_is_what_the_desktop_opens_markdown_with() {
    installed_flatpak();

    let exports = dirs_flatpak_exports();
    let home = Scratch::new("mime-home");
    std::fs::create_dir_all(home.path.join("config")).expect("create a scratch config home");
    std::fs::create_dir_all(home.path.join("data")).expect("create a scratch data home");

    let output = tool("xdg-mime", "sudo dnf install xdg-utils")
        .env_clear()
        .env("PATH", std::env::var("PATH").expect("a PATH"))
        .env("HOME", &home.path)
        .env("XDG_CURRENT_DESKTOP", "GNOME")
        .env("XDG_CONFIG_HOME", home.path.join("config"))
        .env("XDG_DATA_HOME", home.path.join("data"))
        .env("XDG_DATA_DIRS", &exports)
        .args(["query", "default", "text/markdown"])
        .output()
        .expect("run xdg-mime query default");

    let handler = String::from_utf8(output.stdout).expect("xdg-mime prints utf-8");
    assert_eq!(
        handler.trim(),
        format!("{APP_ID}.desktop"),
        "the desktop does not offer the installed flatpak for Markdown; \
         it was asked about {exports}",
    );
}

/// Where a user installation exports the desktop entries and MIME associations of the
/// applications installed into it.
fn dirs_flatpak_exports() -> String {
    let data_home = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| {
        format!(
            "{}/.local/share",
            std::env::var("HOME").expect("a home directory"),
        )
    });
    format!("{data_home}/flatpak/exports/share")
}
