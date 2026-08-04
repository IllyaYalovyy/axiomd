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
        Scratch::under(&std::env::temp_dir(), label)
    }

    /// The same, among the reader's own files instead of the machine's `/tmp`.
    ///
    /// The sandbox's one filesystem grant is `host:ro`, and `host` is not everything:
    /// a flatpak has a `/tmp` of its own and the machine's is not in it, while the
    /// reader's home is (probed on flatpak 1.16.6, 2026-08-04, by reading a file back
    /// from each through `flatpak run --filesystem=host:ro --command=sh`). A probe of
    /// what a document kept beside its pictures does in the sandbox therefore has to
    /// keep them where the reader keeps things, or it would be proving nothing about
    /// the grant it is there to check.
    ///
    /// Under `~/.cache` rather than at the top of the home directory: it is the
    /// reader's home either way, and a suite that scattered folders across the one
    /// they look at every day would not be run twice.
    fn in_the_readers_own_files(label: &str) -> Scratch {
        let home = PathBuf::from(std::env::var_os("HOME").expect("a home directory"));
        Scratch::under(&home.join(".cache"), label)
    }

    fn under(directory: &Path, label: &str) -> Scratch {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let path = directory.join(format!(
            "axiomd-packaging-{label}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir_all(&path).unwrap_or_else(|error| panic!("create {path:?}: {error}"));
        Scratch { path }
    }

    /// Writes a file into this directory — `name` may lead through folders that do not
    /// exist yet — and answers with its path.
    fn write(&self, name: &str, contents: impl AsRef<[u8]>) -> PathBuf {
        let path = self.path.join(name);
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)
                .unwrap_or_else(|error| panic!("create {folder:?}: {error}"));
        }
        std::fs::write(&path, contents).unwrap_or_else(|error| panic!("write {path:?}: {error}"));
        path
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
/// `--socket=`; `devices=` becomes `--device=`; `filesystems=host:ro;` becomes
/// `--filesystem=host:ro`, which is the whole of what axiomd asks of the host.
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

/// The AppStream metainfo, as text — read the way the manifest above is, and for the
/// same reason: it is this project's own file and every value in it is a plain string,
/// so an XML dependency the application does not otherwise need would buy nothing.
/// `appstreamcli validate` is what checks it is well-formed AppStream.
fn metainfo() -> String {
    let path = repository().join(format!("data/{APP_ID}.metainfo.xml"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {path:?}: {error}"))
}

/// One picture the metainfo publishes, as a software centre reads it.
struct Published {
    /// The sentence shown under the picture.
    caption: String,
    /// Where a software centre fetches it from.
    url: String,
    /// The size the entry declares, which a catalogue lays its page out with before it
    /// has fetched anything.
    declared: (i32, i32),
    /// The desktop the picture was taken on, in AppStream's words — `gnome:dark` for
    /// the dark one. Empty when the entry does not say.
    environment: String,
    /// Whether this is the one a listing leads with.
    default: bool,
}

/// Every `<screenshot>` of the metainfo, in the order a listing shows them.
fn published_screenshots() -> Vec<Published> {
    let metainfo = metainfo();
    let opening = metainfo
        .find("<screenshots>")
        .expect("the metainfo publishes no screenshots at all")
        + "<screenshots>".len();
    let closing = metainfo
        .find("</screenshots>")
        .expect("an unterminated <screenshots> in the metainfo");
    metainfo[opening..closing]
        .split("<screenshot")
        .skip(1)
        .map(|entry| {
            let image = between(entry, "<image", "</image>");
            let (attributes, url) = image
                .split_once('>')
                .expect("an unterminated <image> in the metainfo");
            let (tag, _) = entry.split_once('>').expect("an unterminated <screenshot>");
            Published {
                caption: between(entry, "<caption>", "</caption>").trim().to_owned(),
                url: url.trim().to_owned(),
                declared: (
                    attribute(attributes, "width").parse().unwrap_or(0),
                    attribute(attributes, "height").parse().unwrap_or(0),
                ),
                environment: attribute(tag, "environment"),
                default: attribute(tag, "type") == "default",
            }
        })
        .collect()
}

/// What every screenshot URL has to start with: this repository's own files, on the
/// branch a listing reads, as `raw.githubusercontent.com` serves them.
///
/// Built from the homepage the metainfo already gives rather than written out again,
/// so a project that moves does not leave a URL pointing at where it used to be.
fn raw_file_prefix() -> String {
    let homepage = between(&metainfo(), "<url type=\"homepage\">", "</url>");
    format!(
        "{}/main/",
        homepage
            .trim()
            .replace("https://github.com/", "https://raw.githubusercontent.com/")
    )
}

impl Published {
    /// The file of this repository the URL names, relative to its root — and the whole
    /// of what makes a URL checkable: a listing fetches what the URL says, and nothing
    /// but this keeps that in step with the tree.
    fn path(&self) -> String {
        self.url
            .strip_prefix(&raw_file_prefix())
            .unwrap_or_else(|| {
                panic!(
                    "{} is not a file of this repository on its main branch, which is \
                     what {} serves",
                    self.url,
                    raw_file_prefix(),
                )
            })
            .to_owned()
    }

    /// Where that file is in this working tree.
    fn file(&self) -> PathBuf {
        repository().join(self.path())
    }
}

/// How bright a picture is, from 0 for black to 1 for white — each channel weighted
/// the way an eye weights it.
fn brightness(picture: &Path) -> f64 {
    let pixels = gtk::gdk_pixbuf::Pixbuf::from_file(picture)
        .unwrap_or_else(|error| panic!("{} is not a picture: {error}", picture.display()));
    let bytes = pixels.read_pixel_bytes();
    let channels = pixels.n_channels() as usize;
    let rowstride = pixels.rowstride() as usize;
    let (width, height) = (pixels.width() as usize, pixels.height() as usize);

    let mut total = 0.0;
    for row in 0..height {
        for column in 0..width {
            let at = row * rowstride + column * channels;
            total += (0.2126 * f64::from(bytes[at])
                + 0.7152 * f64::from(bytes[at + 1])
                + 0.0722 * f64::from(bytes[at + 2]))
                / 255.0;
        }
    }
    total / (width * height) as f64
}

/// What `opening` and `closing` have between them, or an empty string when they are not
/// both there.
fn between(text: &str, opening: &str, closing: &str) -> String {
    let Some(start) = text.find(opening).map(|at| at + opening.len()) else {
        return String::new();
    };
    match text[start..].find(closing) {
        Some(end) => text[start..start + end].to_owned(),
        None => String::new(),
    }
}

/// The value of `name="…"` in an XML tag, or an empty string when the tag has no such
/// attribute.
fn attribute(tag: &str, name: &str) -> String {
    between(tag, &format!("{name}=\""), "\"")
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

/// The pictures a software centre shows are fetched from this repository over the
/// network, so nothing but this says whether they are there (issue #33).
///
/// The listing pointed at a `reading.png` that was never committed: the metainfo was
/// valid AppStream, the gate was green, and every visitor to the listing got a broken
/// image. What that costs is a URL away from what the file tree says, so the URL is
/// taken apart here and held to the tree — including the size each entry declares,
/// which a catalogue lays its page out with before it has fetched anything.
///
/// The other direction too: a picture nobody publishes is a picture nobody looks at
/// and nobody regenerates, so an unreferenced file under `data/screenshots` fails as
/// well.
#[test]
fn the_metainfo_publishes_pictures_this_repository_really_has() {
    let published = published_screenshots();
    assert!(
        published.len() >= 2,
        "a listing shows the app on a light desktop and on a dark one; this metainfo \
         publishes {} picture(s)",
        published.len(),
    );
    assert_eq!(
        published.iter().filter(|shot| shot.default).count(),
        1,
        "exactly one screenshot is the one a listing leads with",
    );

    let directory = repository().join("data/screenshots");
    let mut referenced = Vec::new();
    for shot in &published {
        assert!(
            !shot.caption.is_empty(),
            "the screenshot at {} has no caption to read under it",
            shot.url,
        );
        let path = shot.path();
        assert!(
            path.starts_with("data/screenshots/"),
            "{path} is published as a screenshot but is not one of the files under \
             data/screenshots",
        );

        let file = shot.file();
        let picture = gtk::gdk_pixbuf::Pixbuf::from_file(&file).unwrap_or_else(|error| {
            panic!("the listing publishes {path}, which is not a picture here: {error}")
        });
        assert_eq!(
            (picture.width(), picture.height()),
            shot.declared,
            "{path} is not the size the metainfo declares it to be",
        );
        referenced.push(file);
    }

    let mut unpublished: Vec<String> = std::fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("read {directory:?}: {error}"))
        .map(|entry| entry.expect("a file under data/screenshots").path())
        .filter(|path| path.extension().is_some_and(|kind| kind == "png"))
        .filter(|path| !referenced.contains(path))
        .map(|path| path.display().to_string())
        .collect();
    unpublished.sort();
    assert!(
        unpublished.is_empty(),
        "these pictures are committed but published nowhere: {}",
        unpublished.join(", "),
    );
}

/// The pair is only a pair if the two are the light and the dark of the same view: a
/// software centre picks between them by the desktop the reader is on, so a dark entry
/// showing the light window is worse than no dark entry at all.
///
/// Asserted of the pixels rather than of the file names, because the name is exactly
/// what a regeneration that lost the theme would keep.
#[test]
fn the_picture_the_metainfo_calls_dark_is_the_dark_one() {
    let published = published_screenshots();
    let dark = published
        .iter()
        .find(|shot| shot.environment.contains("dark"))
        .expect("a listing on a dark desktop needs a screenshot marked as one");
    let light = published
        .iter()
        .find(|shot| !shot.environment.contains("dark"))
        .expect("a listing on a light desktop needs a screenshot that is not the dark one");

    let (dark, light) = (dark.file(), light.file());
    let (dim, bright) = (brightness(&dark), brightness(&light));

    assert!(
        bright > 0.7,
        "{} is published as the light picture and is {bright:.2} bright",
        light.display(),
    );
    assert!(
        dim < 0.3,
        "{} is published as the dark picture and is {dim:.2} bright",
        dark.display(),
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

/// The sandbox reaches the host read-only and no other way: `host:ro` is the one
/// filesystem grant the owner sanctioned (issue #23, 2026-08-03), so that the pictures
/// an author keeps beside a document are there when the document is opened from Files.
///
/// Pinned rather than merely asserted about: the manifest's `finish-args` have to add
/// up to exactly `build-aux/flatpak/permissions.pinned`, so widening the sandbox is a
/// failing test and not a diff nobody read. The check below is what makes the *next*
/// widening fail even if somebody moves the pin with it: `host` without the suffix is
/// write access to everything the reader has, and axiomd writes the one document it was
/// given.
#[test]
fn the_manifest_grants_exactly_the_pinned_permissions_and_no_writable_filesystem() {
    let mut granted = manifest_array("finish-args");
    let mut pinned = pinned_finish_args();
    granted.sort();
    pinned.sort();

    assert_eq!(
        granted, pinned,
        "the manifest's finish-args are no longer the pinned permission set.\n  \
         Widening the sandbox is a decision recorded in build-aux/flatpak/permissions.pinned,\n  \
         and `--filesystem=host:ro` is the whole of what was ruled (issue #23).",
    );

    let filesystems: Vec<&String> = granted
        .iter()
        .filter(|argument| argument.starts_with("--filesystem="))
        .collect();
    assert_eq!(
        filesystems,
        vec!["--filesystem=host:ro"],
        "the sandbox's filesystem access is `host:ro` and nothing else; anything more \
         is a decision for the project owner",
    );
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
// The per-user install — the recommended way to run axiomd (issue #25).
//
// Every one of these runs `scripts/install.sh --user` against a home directory of the
// test's own, so the machine running the suite is never installed into and never
// uninstalled from, and two of them can run at once.
// ---------------------------------------------------------------------------

/// Exactly what a per-user install leaves in the reader's home: the files installed,
/// and the three caches the desktop reads *about* them.
const USER_INSTALL_LAYOUT: [&str; 9] = [
    ".local/bin/axiomd",
    ".local/share/applications/io.github.etf.axiomd.desktop",
    ".local/share/applications/mimeinfo.cache",
    ".local/share/glib-2.0/schemas/gschemas.compiled",
    ".local/share/glib-2.0/schemas/io.github.etf.axiomd.gschema.xml",
    ".local/share/icons/hicolor/icon-theme.cache",
    ".local/share/icons/hicolor/scalable/apps/io.github.etf.axiomd.svg",
    ".local/share/icons/hicolor/symbolic/apps/io.github.etf.axiomd-symbolic.svg",
    ".local/share/metainfo/io.github.etf.axiomd.metainfo.xml",
];

/// axiomd installed with `--user`, into a home directory of this test's own.
struct UserInstalled {
    home: Scratch,
    said: String,
}

impl UserInstalled {
    /// Installs into a home nothing else has touched, with the prefix's `bin` absent
    /// from the PATH the installer sees — the state a reader installing for the first
    /// time is usually in.
    fn new(label: &str) -> UserInstalled {
        UserInstalled::into_home(Scratch::new(label), false)
    }

    /// The same, for a reader who already has `~/.local/bin` on their PATH.
    fn with_bin_on_path(label: &str) -> UserInstalled {
        UserInstalled::into_home(Scratch::new(label), true)
    }

    fn into_home(home: Scratch, bin_on_path: bool) -> UserInstalled {
        let said = UserInstalled::run(
            &home,
            &["--user", "--binary", env!("CARGO_BIN_EXE_axiomd")],
            bin_on_path,
        );
        UserInstalled { home, said }
    }

    /// Takes the install back out, the way the reader who no longer wants it does.
    fn uninstall(&self) -> String {
        UserInstalled::run(&self.home, &["--uninstall", "--user"], false)
    }

    /// Runs the installer against this home and nothing else: the environment is
    /// cleared, so `--user` can only mean the home it is given, and a machine-wide
    /// `XDG_DATA_HOME` cannot quietly redirect it.
    fn run(home: &Scratch, arguments: &[&str], bin_on_path: bool) -> String {
        let path = std::env::var("PATH").expect("a PATH to find the packaging tools on");
        let path = match bin_on_path {
            true => format!("{}:{path}", home.path.join(".local/bin").display()),
            false => path,
        };

        let output = Command::new(repository().join("scripts/install.sh"))
            .env_clear()
            .env("PATH", path)
            .env("HOME", &home.path)
            .args(arguments)
            .output()
            .unwrap_or_else(|error| panic!("run scripts/install.sh {arguments:?}: {error}"));

        assert!(
            output.status.success(),
            "scripts/install.sh {arguments:?} failed:\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        String::from_utf8(output.stdout).expect("the installer prints utf-8")
    }

    /// Where the installed axiomd runs from.
    fn prefix(&self) -> PathBuf {
        self.home.path.join(".local")
    }

    /// Every file under this home, named as the reader would name it — relative to the
    /// home, sorted, so the whole of an install is one comparison.
    fn files(&self) -> Vec<String> {
        let mut found = Vec::new();
        collect_files(&self.home.path, &self.home.path, &mut found);
        found.sort();
        found
    }

    /// Everything left under the prefix, directories as well as files: an uninstall
    /// that leaves a tree of empty directories in the reader's home has not quite taken
    /// itself back out.
    fn left_behind(&self) -> Vec<String> {
        let mut found = Vec::new();
        collect_entries(&self.prefix(), &self.prefix(), &mut found);
        found.sort();
        found
    }

    /// The keys GLib finds for `schema` when it looks at nothing but this home — the
    /// question an installed axiomd asks on its first preference read, asked of the
    /// installed prefix alone. Empty when GLib cannot find the schema at all.
    ///
    /// `XDG_DATA_HOME` is the home's own `share`, which is where a `--user` install
    /// puts things and where GLib looks for schemas (probed with GLib 2.86.5:
    /// `gsettings list-keys` finds a schema compiled under `XDG_DATA_HOME` with
    /// `XDG_DATA_DIRS` pointing nowhere). `XDG_DATA_DIRS` points at an empty directory
    /// so the system's own schemas — which on a developer's machine may well include
    /// an axiomd — cannot be what answers.
    fn keys_glib_finds(&self, schema: &str) -> Vec<String> {
        let nothing = Scratch::new("no-system-schemas");
        let output = tool("gsettings", "sudo dnf install glib2")
            .env_clear()
            .env("PATH", std::env::var("PATH").expect("a PATH"))
            .env("HOME", &self.home.path)
            .env("XDG_DATA_HOME", self.prefix().join("share"))
            .env("XDG_DATA_DIRS", &nothing.path)
            .args(["list-keys", schema])
            .output()
            .expect("run gsettings list-keys");

        if !output.status.success() {
            return Vec::new();
        }
        let mut keys: Vec<String> = String::from_utf8(output.stdout)
            .expect("gsettings prints utf-8")
            .lines()
            .map(str::to_owned)
            .collect();
        keys.sort();
        keys
    }
}

/// Every file under `directory`, relative to `root`.
fn collect_files(root: &Path, directory: &Path, found: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, found);
        } else {
            found.push(named_under(root, &path));
        }
    }
}

/// The same, counting the directories themselves.
fn collect_entries(root: &Path, directory: &Path, found: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        found.push(named_under(root, &path));
        if path.is_dir() {
            collect_entries(root, &path, found);
        }
    }
}

fn named_under(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .expect("a path under the root it was found in")
        .display()
        .to_string()
}

/// axiomd's settings schema, as it is in the repository — the source of both what an
/// installed prefix has to be able to answer and what it has to answer with.
fn declared_schema() -> String {
    std::fs::read_to_string(repository().join(format!("data/{APP_ID}.gschema.xml")))
        .expect("read axiomd's settings schema")
}

/// The keys axiomd's schema declares — the list the installed prefix has to be able to
/// answer with.
fn keys_the_schema_declares() -> Vec<String> {
    let schema = declared_schema();
    let mut keys: Vec<String> = schema
        .split("<key name=\"")
        .skip(1)
        .map(|key| {
            key.split('"')
                .next()
                .expect("a key name in the schema")
                .to_owned()
        })
        .collect();
    keys.sort();
    assert!(!keys.is_empty(), "axiomd's schema declares no keys at all");
    keys
}

/// What the schema says `key` is before the reader has ever changed it — read from the
/// schema rather than repeated here, so this is "what the installed settings say" and
/// not a number that has to be kept in step with them.
fn the_schema_default_for(key: &str) -> String {
    let schema = declared_schema();
    let declaration = schema
        .split_once(&format!("<key name=\"{key}\""))
        .unwrap_or_else(|| panic!("axiomd's schema has no {key}"))
        .1;
    let default = declaration
        .split_once("<default>")
        .expect("a key with no default")
        .1;
    default
        .split_once("</default>")
        .expect("an unterminated default")
        .0
        .trim()
        .to_owned()
}

/// Another application's files, already in the home a reader installs axiomd into.
///
/// The point of it: the desktop's caches are shared. Compiling schemas, rebuilding the
/// MIME cache and rebuilding the icon cache are all whole-directory operations, so an
/// uninstall that reaches for a directory rather than for its own files takes a
/// stranger's application with it.
struct Neighbour {
    files: Vec<String>,
}

impl Neighbour {
    const SCHEMA: &'static str = "io.example.neighbour";

    fn plant(home: &Path) -> Neighbour {
        let applications = home.join(".local/share/applications");
        let schemas = home.join(".local/share/glib-2.0/schemas");
        std::fs::create_dir_all(&applications).expect("create the neighbour's applications");
        std::fs::create_dir_all(&schemas).expect("create the neighbour's schemas");

        std::fs::write(
            applications.join(format!("{}.desktop", Self::SCHEMA)),
            "[Desktop Entry]\nType=Application\nName=Neighbour\nExec=neighbour %F\n\
             Terminal=false\nMimeType=text/x-neighbour;\n",
        )
        .expect("write the neighbour's desktop entry");
        std::fs::write(
            schemas.join(format!("{}.gschema.xml", Self::SCHEMA)),
            format!(
                "<schemalist>\n  <schema id=\"{}\" path=\"/io/example/neighbour/\">\n    \
                 <key name=\"borrowed-cup\" type=\"b\">\n      <default>true</default>\n    \
                 </key>\n  </schema>\n</schemalist>\n",
                Self::SCHEMA,
            ),
        )
        .expect("write the neighbour's schema");

        Neighbour {
            files: vec![
                format!(".local/share/applications/{}.desktop", Self::SCHEMA),
                ".local/share/applications/mimeinfo.cache".to_owned(),
                ".local/share/glib-2.0/schemas/gschemas.compiled".to_owned(),
                format!(".local/share/glib-2.0/schemas/{}.gschema.xml", Self::SCHEMA),
            ],
        }
    }

    /// What must still be there when axiomd has been uninstalled: the neighbour's own
    /// files, and the two caches that have to have been rebuilt around them.
    fn survivors(&self) -> Vec<String> {
        let mut files = self.files.clone();
        files.sort();
        files
    }
}

/// What a per-user install is, in the reader's own home: the binary, the four files the
/// desktop reads, and the three caches it reads them through — and not one thing
/// anywhere else.
#[test]
fn a_user_install_writes_the_whole_of_an_axiomd_into_the_home_it_is_given() {
    let installed = UserInstalled::new("layout");

    assert_eq!(
        installed.files(),
        USER_INSTALL_LAYOUT,
        "a per-user install is not the layout the desktop reads",
    );

    let binary = installed.prefix().join("bin/axiomd");
    let mode = std::os::unix::fs::PermissionsExt::mode(
        &std::fs::metadata(&binary)
            .expect("the installed binary")
            .permissions(),
    );
    assert_eq!(
        mode & 0o111,
        0o111,
        "the installed axiomd is not something the reader can run",
    );
}

/// The preference read that aborts an axiomd with no schema, asked of the installed
/// prefix alone: GLib finds axiomd's schema under the home it was installed into, and
/// finds every key the application has.
///
/// This is the half of "preferences work" that the running application cannot prove on
/// a developer's machine — a copy built from this tree carries the schema its own build
/// compiled as a fallback, so it would start happily even from a prefix with none.
#[test]
fn the_settings_a_user_install_leaves_are_the_ones_glib_finds_under_that_home() {
    let installed = UserInstalled::new("schema");

    assert_eq!(
        installed.keys_glib_finds(APP_ID),
        keys_the_schema_declares(),
        "GLib does not find axiomd's settings under a per-user install, \
         so its first preference read would abort",
    );
}

/// The app grid's half: the entry a user install leaves starts the axiomd beside it
/// however the session's PATH is set — a per-user `bin` is on no PATH by default, and
/// an entry running a bare `axiomd` would be a launcher that does nothing.
#[test]
fn the_entry_a_user_install_leaves_starts_the_axiomd_beside_it() {
    let installed = UserInstalled::new("entry");
    let entry = installed
        .prefix()
        .join(format!("share/applications/{APP_ID}.desktop"));
    let text = std::fs::read_to_string(&entry).expect("read the installed desktop entry");

    let exec = text
        .lines()
        .find_map(|line| line.strip_prefix("Exec="))
        .expect("the installed entry has no Exec");
    let (command, _) = exec.split_once(' ').unwrap_or((exec, ""));
    assert_eq!(
        Path::new(command),
        installed.prefix().join("bin/axiomd"),
        "the installed entry does not start the installed axiomd",
    );
    assert!(
        Path::new(command).is_file(),
        "the installed entry starts something that is not there",
    );

    // Still an entry the desktop will read at all, after the installer has rewritten
    // it: `desktop-file-validate` is the authority on that, and it is silent when happy.
    let checked = tool(
        "desktop-file-validate",
        "sudo dnf install desktop-file-utils",
    )
    .arg(&entry)
    .output()
    .expect("run desktop-file-validate");
    let complaints = format!(
        "{}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr),
    );
    assert!(
        checked.status.success() && complaints.is_empty(),
        "desktop-file-validate is not happy with the installed entry:\n{complaints}",
    );

    // And an entry with a picture beside it: the name it gives its icon has to be an
    // icon the install put in the prefix's own theme, or the app grid draws nothing.
    let icon = text
        .lines()
        .find_map(|line| line.strip_prefix("Icon="))
        .expect("the installed entry names no icon");
    let drawn = installed
        .prefix()
        .join(format!("share/icons/hicolor/scalable/apps/{icon}.svg"));
    gtk::gdk_pixbuf::Pixbuf::from_file(&drawn).unwrap_or_else(|error| {
        panic!("the app grid has no icon to draw for the installed entry: {error}")
    });
}

/// UT-001 for a per-user install, as far as `docs/TESTING.md` category 2 goes: with
/// nothing installed but this, the desktop's own answer to "what opens Markdown" is
/// the axiomd in the reader's home.
///
/// Asked of that home alone — never of the session's own configuration, which the
/// suite must not depend on or change.
#[test]
fn the_desktop_opens_markdown_with_the_axiomd_a_user_install_left() {
    let installed = UserInstalled::new("mime");
    let elsewhere = Scratch::new("mime-config");
    let nothing = Scratch::new("no-system-applications");

    let output = tool("xdg-mime", "sudo dnf install xdg-utils")
        .env_clear()
        .env("PATH", std::env::var("PATH").expect("a PATH"))
        .env("HOME", &installed.home.path)
        .env("XDG_CURRENT_DESKTOP", "GNOME")
        .env("XDG_CONFIG_HOME", &elsewhere.path)
        .env("XDG_DATA_HOME", installed.prefix().join("share"))
        .env("XDG_DATA_DIRS", &nothing.path)
        .args(["query", "default", "text/markdown"])
        .output()
        .expect("run xdg-mime query default");

    let handler = String::from_utf8(output.stdout).expect("xdg-mime prints utf-8");
    assert_eq!(
        handler.trim(),
        format!("{APP_ID}.desktop"),
        "the desktop does not offer a per-user install for Markdown",
    );
}

/// The reader who runs `axiomd` in a terminal is told what to add, and the reader who
/// does not need to be told is not told anything.
#[test]
fn a_user_install_says_how_to_reach_axiomd_from_a_terminal_only_when_it_cannot_be() {
    let hinted = UserInstalled::new("path-hint");
    assert!(
        hinted
            .said
            .contains("export PATH=\"$HOME/.local/bin:$PATH\""),
        "an install into a prefix that is on no PATH did not say how to reach it:\n{}",
        hinted.said,
    );

    let quiet = UserInstalled::with_bin_on_path("path-quiet");
    assert!(
        !quiet.said.contains("PATH"),
        "an install into a prefix already on the PATH talked about the PATH anyway:\n{}",
        quiet.said,
    );
}

/// Uninstalling is the install undone: the home it was installed into is as empty as
/// it was before, caches and all.
#[test]
fn uninstalling_a_user_install_leaves_nothing_behind() {
    let installed = UserInstalled::new("uninstall");
    assert_eq!(installed.files(), USER_INSTALL_LAYOUT);

    installed.uninstall();

    assert_eq!(
        installed.left_behind(),
        Vec::<String>::new(),
        "uninstalling a per-user install left something behind",
    );
}

/// And it is *only* the install undone. The desktop's caches are shared, so the
/// dangerous way to write an uninstall is to reach for the directories rather than for
/// the files: this is the test that says a stranger's application is still installed,
/// still compiled and still the handler for its own documents afterwards.
#[test]
fn uninstalling_a_user_install_leaves_another_application_in_that_home_working() {
    let home = Scratch::new("uninstall-neighbour");
    let neighbour = Neighbour::plant(&home.path);
    let installed = UserInstalled::into_home(home, false);

    installed.uninstall();

    assert_eq!(
        installed.files(),
        neighbour.survivors(),
        "uninstalling axiomd did not leave the neighbouring application's home alone",
    );
    assert_eq!(
        installed.keys_glib_finds(Neighbour::SCHEMA),
        vec!["borrowed-cup".to_owned()],
        "uninstalling axiomd took the neighbour's settings out of the compiled schemas",
    );
    assert_eq!(
        installed.keys_glib_finds(APP_ID),
        Vec::<String>::new(),
        "axiomd's settings are still compiled into a home it was uninstalled from",
    );
}

/// The probe issue #25 asks for, in place of somebody installing axiomd and looking at
/// it: the copy a per-user install leaves is started from where it was installed, with
/// the prefix leading the data directories the desktop reads, and it opens a document
/// and offers the reader their preferences.
#[test]
fn the_axiomd_a_user_install_leaves_reads_a_document_and_offers_its_preferences() {
    let installed = UserInstalled::new("launch");
    let fixture = Fixture::new("user-install");
    let document = fixture.write("notes.md", "# Installed\n\nA paragraph.\n");

    let app = axiomd_e2e::launch_installed(&installed.prefix(), &document);

    assert_eq!(
        app.dom_text("h1"),
        "Installed",
        "the installed axiomd did not render the document it was opened with",
    );

    app.activate("app.preferences");
    assert_eq!(
        app.visible_dialog(),
        "Preferences",
        "the installed axiomd has no preferences to show",
    );
    assert_eq!(
        app.preference("Reading Width"),
        the_schema_default_for("reading-width"),
        "the installed axiomd is not reading its settings out of the schema it installed",
    );

    assert!(
        app.close().is_empty(),
        "the installed axiomd left something running",
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
    // And the rule the pin itself is held to, so that moving the pin is not enough to
    // grant the sandbox a way to write the reader's files: every filesystem axiomd is
    // given is read-only. `host:ro` is what the owner ruled (issue #23); `host` is not.
    for line in permission_lines(&shown) {
        let Some(filesystems) = line.strip_prefix("filesystems=") else {
            continue;
        };
        for granted in filesystems.split(';').filter(|value| !value.is_empty()) {
            assert!(
                granted.ends_with(":ro"),
                "the installed sandbox can write {granted}; axiomd writes the one \
                 document it was given and nothing else:\n{shown}",
            );
        }
    }
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

    // Last, because switching modes re-renders what everything above asserted about:
    // the controls the runtime has to draw, not only the document it renders. An icon
    // the sandbox's icon theme is missing is a missing-image glyph in the header, and
    // the reader inside the flatpak is the only one who would ever see it (issue #28).
    assert_eq!(
        app.mode_switch().icon,
        "document-edit-symbolic",
        "the packaged runtime has no icon for reading's face of the mode switch",
    );
    app.activate("win.mode");
    app.wait_until_mode("edit");
    assert_eq!(
        app.mode_switch().icon,
        "view-reveal-symbolic",
        "the packaged runtime has no icon for editing's face of the mode switch",
    );
}

/// The defect issue #22 reports: a document double-clicked in Files reaches a packaged
/// axiomd through the document portal, and it has to *render* — not be shown as its own
/// source.
///
/// Nothing about the route is faked. `--file-forwarding` is the flag the exported
/// desktop entry carries, so flatpak exports the document to the portal itself and the
/// application is launched with the fuse path it answers with, and the folder the
/// document is in is handed to the sandbox by nobody. What is asserted is what the
/// reader sees: rendered elements, and not one character of Markdown syntax left in the
/// page.
///
/// Two names, because the portal keeps the one the reader's file has (probed on flatpak
/// 1.16.2: forwarding `article.medium.md` arrives as
/// `/run/user/1000/doc/78e9eb22/article.medium.md`). The everyday name is the reported
/// case; the name with no extension at all is the one that would catch a build deciding
/// how to show a document from the shape of its path, which is what the report suspected.
#[test]
#[ignore = "drives the installed flatpak; run by scripts/quality.d/40-flatpak.sh"]
fn the_installed_flatpak_renders_a_document_opened_through_the_document_portal() {
    installed_flatpak();

    let fixture = Fixture::new("flatpak-portal");
    for name in ["article.medium.md", "article"] {
        let document = fixture.write(
            name,
            "# Through the portal\n\n## A section\n\nA paragraph with **bold**.\n",
        );

        // The portal is the route only while it is the *only* route: flatpak forwards
        // a file through the document portal when the sandbox cannot already reach it,
        // and since `host:ro` that turns on where the document is kept (issue #23,
        // RFC-001 Q5). This fixture is under `/tmp`, which a sandbox has its own of and
        // `host` does not carry — asserted rather than trusted, because a fixture moved
        // into the reader's home would leave this test passing and testing nothing.
        let reachable = in_the_sandbox(&format!("cat {}", document.display()));
        assert!(
            !reachable.status.success(),
            "the sandbox can read {} without the portal, so this test is no longer \
             about the portal",
            document.display(),
        );

        let app = axiomd_e2e::launch_installed_flatpak_from_the_desktop(&document);

        assert_eq!(
            app.window_title(),
            name,
            "the portal did not hand the sandbox the document the reader chose",
        );
        assert_eq!(
            app.mode(),
            "read",
            "{name} opened through the portal did not open in read mode",
        );
        assert_eq!(
            app.dom_text("h1"),
            "Through the portal",
            "{name} opened through the portal was not rendered",
        );
        assert_eq!(
            app.dom_text("h2"),
            "A section",
            "{name} opened through the portal was not rendered",
        );
        assert_eq!(
            app.dom("document.querySelectorAll('strong').length"),
            "1",
            "{name} opened through the portal was not rendered",
        );
        assert!(
            !app.dom("document.body.textContent").contains('#'),
            "{name} opened through the portal is showing its own Markdown source",
        );

        // Issue #24: the window said `/run/user/1000/doc/d8ded700` about a document the
        // reader keeps somewhere else entirely. The whole of the resolution runs here —
        // a sandboxed axiomd, a document it can only reach by the portal's fuse path,
        // and the desktop asked over the session bus — and what it must produce is the
        // folder this test wrote the file into.
        let header = app.header();
        assert_eq!(
            header.title, name,
            "the header is not naming the document the reader opened",
        );
        assert_eq!(
            header.where_it_lives,
            document
                .parent()
                .expect("the document has a folder")
                .display()
                .to_string(),
            "the sandbox did not resolve the portal's document back to the reader's \
             own folder",
        );
        assert_eq!(
            header.in_full,
            document.display().to_string(),
            "hovering the title does not give the reader their own path in full",
        );
        for said in [&header.title, &header.where_it_lives, &header.in_full] {
            assert!(
                !said.contains("/run/user") && !said.contains("/doc/"),
                "the window is showing the reader a portal path: {said:?}",
            );
        }

        assert!(
            app.close().is_empty(),
            "the sandboxed launch left something running",
        );
    }
}

/// Runs `script` in the installed sandbox and answers with what it did.
///
/// Nothing is granted for the occasion: no `--filesystem`, no socket, no environment —
/// what this shell can reach is exactly what the packaged axiomd can reach, which is
/// the only reason its answer is worth anything. The shell is the runtime's own:
/// org.gnome.Platform//49 has an `sh` and `--command=sh` runs it (probed 2026-08-04).
fn in_the_sandbox(script: &str) -> std::process::Output {
    tool("flatpak", "sudo dnf install flatpak")
        .args(["run", "--command=sh", APP_ID, "-c"])
        .arg(script)
        .output()
        .expect("run a shell inside the installed sandbox")
}

/// Issue #23, and the scenario the owner met the flatpak with: a document kept among
/// the reader's own files with a picture beside it, opened the way Files opens it, and
/// the picture is *there*.
///
/// Nothing about the route is arranged. The launch is the one the exported desktop
/// entry makes — `--file-forwarding`, the flag a double-click in Files goes through —
/// and what the sandbox is handed is whatever flatpak decides to hand it: since
/// `host:ro` that is the document's own host path rather than a portal one, because
/// flatpak forwards through the document portal only what the sandbox cannot already
/// reach (probed; RFC-001 Q5). The folder the document is in is granted by nobody: this
/// launch grants only the harness's own directory, and the sandbox reads the reader's
/// files because the *package* carries `--filesystem=host:ro`. That is the whole of
/// what makes the picture arrive.
///
/// The second picture is the other half of the ruling. `host:ro` let axiomd read
/// everything the reader can read; it did not let a *document* reach past its own
/// folder. `elsewhere/secret.png` is a real file, and this test proves the sandbox can
/// read it before asserting that the document cannot.
#[test]
#[ignore = "drives the installed flatpak; run by scripts/quality.d/40-flatpak.sh"]
fn the_installed_flatpak_shows_the_picture_kept_beside_the_document() {
    installed_flatpak();

    let kept = Scratch::in_the_readers_own_files("beside");
    let beside = kept.write("article/diagram.png", support::png());
    let outside = kept.write("elsewhere/secret.png", support::png());
    let document = kept.write(
        "article/article.md",
        "# Beside\n\n![a diagram](diagram.png)\n\n![somewhere else](../elsewhere/secret.png)\n",
    );

    let reachable = in_the_sandbox(&format!("cat {}", outside.display()));
    assert!(
        reachable.status.success() && reachable.stdout == support::png(),
        "the sandbox cannot read {}, so what this test asserts about the document \
         reaching it would prove nothing",
        outside.display(),
    );

    let app = axiomd_e2e::launch_installed_flatpak_from_the_desktop(&document);

    // `complete` is true whether a picture decoded or failed to, so waiting on it is
    // what makes the sizes below the answer rather than a race (`links.rs`).
    app.wait_until(
        "document.querySelectorAll('img').length === 2 && \
         [...document.querySelectorAll('img')].every(picture => picture.complete)",
    );

    assert_eq!(
        app.dom("document.querySelectorAll('img')[0].naturalWidth"),
        "40",
        "the picture the author kept beside the document is broken in the package — \
         the whole of issue #23",
    );
    assert_eq!(
        app.dom("document.querySelectorAll('img')[1].naturalWidth"),
        "0",
        "a document reached a file outside its own folder; `host:ro` is what the \
         sandbox may read, never what a document may name",
    );
    assert_eq!(
        std::fs::read(&beside).expect("read the picture back"),
        support::png(),
        "showing the picture changed it",
    );

    assert!(
        app.close().is_empty(),
        "the sandboxed launch left something running",
    );
}

/// The other half of the ruling, asked of the sandbox rather than read off a permission
/// listing: `host:ro` is read capability and nothing more.
///
/// A listing that says `host:ro` is a string. What it has to mean is that the reader's
/// files can be read and cannot be changed, and that is asserted on the files
/// themselves — through a shell with exactly the package's own permissions, so what it
/// may do is what axiomd may do.
#[test]
#[ignore = "drives the installed flatpak; run by scripts/quality.d/40-flatpak.sh"]
fn the_sandbox_reads_the_readers_files_and_writes_none_of_them() {
    installed_flatpak();

    let kept = Scratch::in_the_readers_own_files("read-only");
    let beside = kept.write("beside.md", "# Beside\n");

    let read = in_the_sandbox(&format!("cat {}", beside.display()));
    assert!(
        read.status.success() && read.stdout == b"# Beside\n",
        "the sandbox cannot read the reader's own files, which is what the owner \
         ruled it may do (issue #23): {}",
        String::from_utf8_lossy(&read.stderr),
    );

    // Over a file the reader has, and beside it: the two ways a write would land
    // somewhere no portal ever granted.
    for scribbling in [
        format!("echo scribbled >> {}", beside.display()),
        format!("echo scribbled > {}/invented.md", kept.path.display()),
    ] {
        let refused = in_the_sandbox(&scribbling);
        assert!(
            !refused.status.success(),
            "the sandbox wrote the reader's files: `{scribbling}` succeeded",
        );
    }
    assert_eq!(
        std::fs::read_to_string(&beside).expect("read the file back"),
        "# Beside\n",
        "the sandbox changed a file beside the document",
    );
    assert!(
        !kept.path.join("invented.md").exists(),
        "the sandbox left a file of its own among the reader's",
    );
}

/// What a reader meets when the document they opened is one the package may read and
/// not write — which, since `host:ro`, is every document Files hands a packaged axiomd
/// from the reader's own home.
///
/// Probed on flatpak 1.16.6, 2026-08-04: `--file-forwarding` — the flag the exported
/// desktop entry carries — puts a file in the document portal *only when the sandbox
/// cannot already reach it*. Before `host:ro` that was every document, and the fuse
/// path the portal answered with was writable; now a document under the reader's home
/// arrives as its own host path, and that path is read-only. A document under `/tmp`,
/// which `host` does not carry, still arrives through the portal.
///
/// So Ctrl+S on a document opened from Files cannot reach the file, and what this pins
/// is that it fails the way this project requires a failure to look (invariant 12): the
/// reader is told where they are looking, their work is still in front of them, and the
/// file is untouched — no dialog, and nothing half-written. **Whether that is where the
/// story ends is an open question for the owner: RFC-001 Q5.** Settling it will change
/// this test, which is the point of writing it down.
#[test]
#[ignore = "drives the installed flatpak; run by scripts/quality.d/40-flatpak.sh"]
fn a_save_the_package_cannot_make_is_said_inline_and_costs_the_reader_nothing() {
    installed_flatpak();

    let kept = Scratch::in_the_readers_own_files("unwritable");
    let document = kept.write("article.md", "# Kept\n\nA paragraph.\n");

    let app = axiomd_e2e::launch_installed_flatpak_from_the_desktop(&document);
    app.activate("win.mode");
    app.wait_until_mode("edit");
    app.type_text("Edited in the sandbox.\n\n");
    app.activate("win.save");

    let said = app.wait_for_banner("article.md");
    assert!(
        said.contains("Could not save article.md"),
        "the reader was not told their document did not reach the file: {said:?}",
    );
    assert_eq!(
        std::fs::read_to_string(&document).expect("read the document back"),
        "# Kept\n\nA paragraph.\n",
        "the file changed although the save failed",
    );
    let mut left_behind: Vec<String> = std::fs::read_dir(&kept.path)
        .expect("read the folder back")
        .map(|entry| entry.expect("a file in the folder").file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .collect();
    left_behind.sort();
    assert_eq!(
        left_behind,
        vec!["article.md".to_owned()],
        "a failed save left something of its own beside the reader's document",
    );
    assert_eq!(
        app.window_title(),
        "• article.md",
        "the window says the document is saved when it is not",
    );

    // And the work itself is still in hand: the reader can go on reading what they
    // typed, and take it somewhere writable with Save As.
    app.activate("win.mode");
    app.wait_until_mode("read");
    app.wait_until("document.body.textContent.includes('Edited in the sandbox.')");

    assert!(
        app.close().is_empty(),
        "the sandboxed launch left something running",
    );
}

/// The mechanism issue #24 rests on, asked of the desktop's own document portal rather
/// than described: a document exported to the portal resolves back to the path the
/// reader keeps it at.
///
/// `flatpak document-export` puts a real file into the portal exactly as opening it
/// from Files does, and prints the fuse path the portal answers with. What is asserted
/// is that `Home` turns that path back into the one this test wrote — through
/// `org.freedesktop.portal.Documents.GetHostPaths`, over the session bus, with nothing
/// mocked and nothing assumed about the shape of the answer.
///
/// It is a probe rather than an ordinary test because it needs the desktop it is
/// probing: a document portal on the session bus, and `flatpak` to put something in it.
/// `scripts/quality.d/40-flatpak.sh` is where that is guaranteed.
#[test]
#[ignore = "drives the installed flatpak; run by scripts/quality.d/40-flatpak.sh"]
fn the_document_portal_says_where_a_forwarded_document_really_lives() {
    use axiomd_doc::Home;

    let fixture = Fixture::new("portal-host-path");
    let document = fixture.write("article.medium.md", "# Through the portal\n");
    let exported = Exported::of(&document);

    assert!(
        exported.fuse.starts_with("/run/user/"),
        "the portal did not answer with a path of its own, so this probe proves \
         nothing: {:?}",
        exported.fuse,
    );

    let home = Home::of(&exported.fuse);

    assert_eq!(
        home.path(),
        exported.fuse,
        "the path axiomd has to open the document by was lost",
    );
    assert_eq!(
        home.full(),
        document.display().to_string(),
        "the portal's document was not resolved back to the reader's own file",
    );
    assert_eq!(
        home.folder(),
        document.parent(),
        "a picture beside the document would resolve against the portal's folder",
    );
    for said in [home.shown(), home.full()] {
        assert!(
            !said.contains("/run/user"),
            "a window would show the reader a portal path: {said:?}",
        );
    }
}

/// A file put into the document portal for as long as one test needs it, and taken out
/// again afterwards — the portal's registry outlives the process, and a suite that grew
/// it every run would be leaving the reader's desktop worse than it found it.
struct Exported {
    fuse: PathBuf,
    document: PathBuf,
}

impl Exported {
    fn of(document: &Path) -> Exported {
        let exported = tool("flatpak", "sudo dnf install flatpak")
            .args(["document-export", "--app"])
            .arg(APP_ID)
            .arg(document)
            .output()
            .expect("run flatpak document-export");
        assert!(
            exported.status.success(),
            "flatpak document-export refused {document:?}: {}",
            String::from_utf8_lossy(&exported.stderr),
        );
        Exported {
            fuse: PathBuf::from(
                String::from_utf8(exported.stdout)
                    .expect("flatpak prints utf-8")
                    .trim(),
            ),
            document: document.to_path_buf(),
        }
    }
}

impl Drop for Exported {
    fn drop(&mut self) {
        let _ = Command::new("flatpak")
            .arg("document-unexport")
            .arg(&self.document)
            .status();
    }
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
