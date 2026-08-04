//! The buffer turned into a page a window can show — always off the main loop.
//!
//! Parsing and rendering are the expensive things axiomd does, and both happen here,
//! on a worker. The main loop hands over the text the reader's buffer holds and gets a
//! [`Rendered`] back later; it never waits, so neither a huge document nor a fast
//! typist can make the window stop drawing (invariant 4).
//!
//! The text comes from the buffer and never from the file (invariant 11): what the
//! reader sees rendered is what they have in front of them, saved or not.
//!
//! A [`Renderer`] holds one document's worth of work at a time. Asking for another
//! render supersedes the one in flight: the worker abandons it at the next phase
//! boundary and its result is dropped rather than shown, so a window can never display
//! a document the user has already moved past. This is the machinery live reload and
//! mode switching build on.
//!
//! [`FileId`] is the other half: it answers "is this the file that window already
//! has open?" by identity rather than by spelling, so `./README.md`, an absolute path
//! and a symlink all name the same document.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axiomd_engine::{EngineId, Extensions, MarkdownEngine};
use axiomd_render::{Plugins, Rendered};
use gtk::gio;
use gtk::glib;

/// A file's identity on disk, independent of the path used to reach it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FileId {
    device: u64,
    inode: u64,
}

impl FileId {
    /// The identity of the file `path` currently resolves to, or `None` if there is
    /// no such file.
    pub(crate) fn of(path: &Path) -> Option<Self> {
        use std::os::unix::fs::MetadataExt;

        let metadata = std::fs::metadata(path).ok()?;
        Some(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
}

/// Renders one window's document away from the main loop.
pub(crate) struct Renderer {
    generation: Arc<AtomicU64>,
    show: Rc<dyn Fn(Rendered)>,
}

impl Renderer {
    /// Builds a renderer that hands every page it finishes to `show`, on the thread
    /// that created it.
    pub(crate) fn new(show: impl Fn(Rendered) + 'static) -> Self {
        Self {
            generation: Arc::new(AtomicU64::new(0)),
            show: Rc::new(show),
        }
    }

    /// Starts rendering `source`, superseding whatever was in flight.
    ///
    /// `name` is what the reader calls the document, and reaches the page as its
    /// title — which is what a print job and an exported PDF are called.
    ///
    /// Returns at once. The page arrives later through the callback, on the calling
    /// thread; a superseded render never arrives at all.
    pub(crate) fn render(
        &self,
        source: String,
        name: String,
        engine: EngineId,
        plugins: Plugins,
        root: Option<PathBuf>,
    ) {
        let mine = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let generation = self.generation.clone();
        let show = self.show.clone();

        glib::spawn_future_local(async move {
            let worker = generation.clone();
            let composed = gio::spawn_blocking(move || {
                compose(&source, &name, engine, &plugins, root.as_deref(), &|| {
                    worker.load(Ordering::SeqCst) != mine
                })
            })
            .await;

            if generation.load(Ordering::SeqCst) != mine {
                return;
            }
            match composed {
                Ok(Some(page)) => show(page),
                Ok(None) => {}
                // A panicking worker is a bug in the pipeline, not something the reader
                // can act on: they keep the page they have and it is reported here.
                Err(_) => eprintln!("axiomd: the renderer stopped unexpectedly"),
            }
        });
    }
}

/// The engine `id` names, or the one a document is read with when nothing has chosen.
///
/// The single place a name becomes something that parses. A name the registry does not
/// know — a preference written by a build that had an engine this one does not, or an
/// engine that has been renamed — is answered with a document rather than with nothing:
/// a reader must never be left staring at an empty window because of a string.
fn engine_named(id: EngineId) -> &'static dyn MarkdownEngine {
    axiomd_engine::engine(id.as_str()).unwrap_or_else(|| {
        eprintln!("axiomd: no {id} engine in this build; reading with the default");
        axiomd_engine::engines()[0]
    })
}

/// Parses and renders `source` with the engine and plugins the reader is reading
/// under, giving up as soon as `superseded` says the result is no longer wanted.
///
/// `root` is the document's own folder, which is what a `[[wikilink]]` in it may reach
/// (`ux_decisions.md`: there is no vault). It is walked here, on the worker, because
/// the pipeline itself opens nothing: what it is given is a list of names.
fn compose(
    source: &str,
    name: &str,
    engine: EngineId,
    plugins: &Plugins,
    root: Option<&Path>,
    superseded: &dyn Fn() -> bool,
) -> Option<Rendered> {
    if superseded() {
        return None;
    }
    let parsed = engine_named(engine).parse(source, Extensions::FULL);

    if superseded() {
        return None;
    }
    let beside = beside(root);

    if superseded() {
        return None;
    }
    Some(axiomd_render::render(&parsed, name, plugins, &beside))
}

/// How many documents deep a wikilink may reach, and how many of them there may be.
///
/// A folder is walked afresh for every render, so a reader who drops a note beside
/// their document sees links to it resolve at the next keystroke rather than at the
/// next launch. These bounds are what keep that from ever being expensive: a home
/// directory opened by accident costs a bounded walk instead of an unbounded one.
const REACH: usize = 8;
const MOST_DOCUMENTS: usize = 20_000;

/// The Markdown documents under `root`, each named relative to it.
///
/// Blocking: for a worker, never for the main loop. Hidden directories are skipped —
/// nobody links into `.git` — and directory symlinks are not followed, because a link
/// pointing at its own ancestor is a walk that never ends.
fn beside(root: Option<&Path>) -> axiomd_render::Folder {
    let Some(root) = root else {
        return axiomd_render::Folder::empty();
    };
    let mut documents = Vec::new();
    let mut folders = vec![(root.to_path_buf(), String::new(), 0usize)];
    while let Some((folder, prefix, depth)) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = format!("{prefix}{name}");
            if kind.is_dir() && depth + 1 < REACH {
                folders.push((entry.path(), format!("{path}/"), depth + 1));
            } else if kind.is_file() && is_a_markdown_file(&name) {
                documents.push(path);
                if documents.len() >= MOST_DOCUMENTS {
                    return axiomd_render::Folder::holding(documents);
                }
            }
        }
    }
    axiomd_render::Folder::holding(documents)
}

/// The extensions axiomd claims (`ux_decisions.md`: Markdown files only), in the order
/// it prefers them: the first is the one a document with no name of its own is given.
///
/// This is the decision itself, and everything derived from it is derived here.
pub(crate) const MARKDOWN_EXTENSIONS: [&str; 2] = ["md", "markdown"];

/// Whether `file` is a document axiomd reads — asked of a bare name or of a whole
/// path, and answered the same way for both.
///
/// The one place the question is decided (issue #22). It was decided in two places
/// that happened to agree, which is a coincidence and not a design: the folder a
/// wikilink resolves against and the link the reader clicks are the same question about
/// the same file, and an app that answers them differently shows an outline of a
/// document it will not open.
///
/// It is a question about the *name*, so it says nothing about what is on disk, and
/// nothing about the shape of the path leading to it: a document reached through the
/// desktop's file portal lives under `/run/user/<uid>/doc/<id>/`, on a filesystem of
/// its own, and is exactly as much a document as one in the reader's home.
pub(crate) fn is_a_markdown_file(file: impl AsRef<Path>) -> bool {
    file.as_ref()
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            MARKDOWN_EXTENSIONS
                .iter()
                .any(|claimed| extension.eq_ignore_ascii_case(claimed))
        })
}

/// `name` with a Markdown extension on it, for a document that carries none — what
/// Save As offers a document that has never had a name.
pub(crate) fn as_a_markdown_name(name: &str) -> String {
    match Path::new(name).extension() {
        Some(_) => name.to_owned(),
        None => format!("{name}.{}", MARKDOWN_EXTENSIONS[0]),
    }
}

/// The same document as one file that carries everything it needs — the export path's
/// half of the pipeline.
///
/// It is here rather than beside the exporter so that there is exactly one place that
/// turns a chosen engine and an extension set into a parse: what is exported is what
/// the preview shows, because both are composed by this module from the same buffer
/// with the same engine.
///
/// Slow by nature — it reads every picture the document names — so it is only ever
/// called on a worker (`export.rs`).
pub(crate) fn compose_standalone(
    source: &str,
    name: &str,
    engine: EngineId,
    plugins: &Plugins,
    root: Option<&Path>,
    embed: &dyn Fn(&str) -> Option<axiomd_render::Picture>,
) -> String {
    let parsed = engine_named(engine).parse(source, Extensions::FULL);
    axiomd_render::standalone(&parsed, name, plugins, &beside(root), embed)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::thread::ThreadId;
    use std::time::Duration;

    use super::*;
    use crate::testing::ScratchDir;

    /// How long a render is allowed to take before the test calls it hung. Far above
    /// any real render of a test fixture; it exists so a renderer that never
    /// delivers fails the suite instead of hanging it.
    const WATCHDOG: Duration = Duration::from_secs(30);

    /// A renderer on a main loop of its own.
    ///
    /// Private rather than the process-wide default context, because the test binary
    /// runs tests on several threads at once and a `MainContext` may only be owned by
    /// one of them.
    struct Harness {
        main_loop: glib::MainLoop,
        shown: Rc<RefCell<Vec<(Rendered, ThreadId)>>>,
    }

    impl Harness {
        /// Runs `body` with a renderer whose deliveries are recorded, and returns the
        /// pages that reached the window, in arrival order.
        fn run(body: impl FnOnce(&Harness, &Renderer)) -> Vec<(Rendered, ThreadId)> {
            let context = glib::MainContext::new();
            let shown = Rc::new(RefCell::new(Vec::new()));
            let delivered = shown.clone();
            context
                .with_thread_default(|| {
                    let main_loop = glib::MainLoop::new(Some(&context), false);
                    let stop = main_loop.clone();
                    let recorder = shown.clone();
                    let renderer = Renderer::new(move |page| {
                        recorder
                            .borrow_mut()
                            .push((page, std::thread::current().id()));
                        stop.quit();
                    });
                    let harness = Harness {
                        main_loop,
                        shown: shown.clone(),
                    };
                    body(&harness, &renderer);
                })
                .expect("own a private main context");
            delivered.borrow().clone()
        }

        fn shown(&self) -> Vec<Rendered> {
            self.shown
                .borrow()
                .iter()
                .map(|(page, _)| page.clone())
                .collect()
        }

        /// Drives the loop until a page is delivered, or fails the test.
        fn run_until_a_page_is_shown(&self) {
            let watchdog = self.main_loop.clone();
            glib::spawn_future_local(async move {
                glib::timeout_future(WATCHDOG).await;
                watchdog.quit();
            });
            self.main_loop.run();
            assert!(
                !self.shown.borrow().is_empty(),
                "no page was ever delivered",
            );
        }
    }

    /// What the reader has in the buffer is what gets rendered — never a file.
    #[test]
    fn shows_the_document_the_buffer_holds() {
        let shown = Harness::run(|harness, renderer| {
            renderer.render(
                "# Title\n\nBody text.\n".to_owned(),
                "notes".to_owned(),
                axiomd_engine::engines()[0].id(),
                Plugins::builtin(&[]),
                None,
            );
            harness.run_until_a_page_is_shown();
        });

        assert_eq!(shown.len(), 1);
        let html = shown[0].0.html();
        assert!(
            html.contains("<h1 id=\"title\" data-line=\"1\">Title</h1>"),
            "{html}"
        );
        assert!(html.contains("Body text."), "{html}");
    }

    /// The main loop must be free while a document renders: nothing is delivered
    /// until the loop is driven, and the delivery lands on the thread that owns it.
    #[test]
    fn renders_without_blocking_the_thread_that_asked() {
        let caller = std::thread::current().id();

        let shown = Harness::run(|harness, renderer| {
            renderer.render(
                "# Title\n".to_owned(),
                "notes".to_owned(),
                axiomd_engine::engines()[0].id(),
                Plugins::builtin(&[]),
                None,
            );
            assert!(
                harness.shown().is_empty(),
                "render() produced a page before the loop ran, so it did the work inline",
            );
            harness.run_until_a_page_is_shown();
        });

        assert_eq!(
            shown[0].1, caller,
            "the page was delivered off the main thread"
        );
    }

    /// A window must never flash a document the user has already moved past — which
    /// while the reader is typing is every keystroke but the last.
    #[test]
    fn a_superseded_render_is_never_shown() {
        let shown = Harness::run(|harness, renderer| {
            renderer.render(
                "# Stale\n".to_owned(),
                "notes".to_owned(),
                axiomd_engine::engines()[0].id(),
                Plugins::builtin(&[]),
                None,
            );
            renderer.render(
                "# Wanted\n".to_owned(),
                "notes".to_owned(),
                axiomd_engine::engines()[0].id(),
                Plugins::builtin(&[]),
                None,
            );
            harness.run_until_a_page_is_shown();
        });

        assert_eq!(shown.len(), 1, "more than the last request was shown");
        let html = shown[0].0.html();
        assert!(html.contains("Wanted"), "{html}");
        assert!(!html.contains("Stale"), "{html}");
    }

    /// The rule the whole application asks, over the names it is asked about — a
    /// document the reader has, one written under either extension, one shouted, and
    /// the things beside a document that are not documents.
    ///
    /// It answers about a name, so a path of any shape carrying that name gets the same
    /// answer: the document portal's `/run/user/1000/doc/<id>/notes.md` is a document,
    /// and the `<id>` directory above it — which is a name with no extension at all,
    /// and the shape issue #22 suspected of demoting documents — is not one.
    #[test]
    fn what_axiomd_reads_is_decided_by_the_name_and_never_by_the_path() {
        for name in [
            "notes.md",
            "README.markdown",
            "NOTES.MD",
            "Guide.Markdown",
            "/home/reader/documents/notes.md",
            "/run/user/1000/doc/6a6290b7/article.medium.md",
            "./sub/../notes.md",
        ] {
            assert!(
                is_a_markdown_file(name),
                "{name} is a document axiomd reads"
            );
        }
        for name in [
            "notes.txt",
            "diagram.png",
            "notes",
            "notes.mdx",
            "/run/user/1000/doc/6a6290b7",
            ".md",
        ] {
            assert!(
                !is_a_markdown_file(name),
                "{name} is not a document axiomd reads",
            );
        }
    }

    /// Windows are deduplicated by this identity, so spelling must not matter.
    #[test]
    fn one_file_has_one_identity_however_it_is_named() {
        let scratch = ScratchDir::new("file-id");
        let file = scratch.write("notes.md", "# Notes\n");
        let link = scratch.path().join("link.md");
        std::os::unix::fs::symlink(&file, &link).expect("create symlink");
        let indirect = scratch.path().join("./sub/../notes.md");
        std::fs::create_dir_all(scratch.path().join("sub")).expect("create subdirectory");
        let other = scratch.write("other.md", "# Other\n");

        let identity = FileId::of(&file).expect("the file exists");

        assert_eq!(
            FileId::of(&link),
            Some(identity),
            "a symlink is the same file"
        );
        assert_eq!(
            FileId::of(&indirect),
            Some(identity),
            "a roundabout path is the same file",
        );
        assert_ne!(
            FileId::of(&other),
            Some(identity),
            "a different file is a different document",
        );
        assert_eq!(FileId::of(&scratch.path().join("gone.md")), None);
    }
}
