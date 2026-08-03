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

use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axiomd_engine::{ComrakEngine, Extensions, MarkdownEngine};
use axiomd_render::Rendered;
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
    pub(crate) fn render(&self, source: String, name: String) {
        let mine = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let generation = self.generation.clone();
        let show = self.show.clone();

        glib::spawn_future_local(async move {
            let worker = generation.clone();
            let composed = gio::spawn_blocking(move || {
                compose(&source, &name, &|| worker.load(Ordering::SeqCst) != mine)
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

/// The engines a document can be read with, in the order preferences offer them.
///
/// One so far: the boundary exists precisely so that the second (#17) changes this
/// list and nothing else. The reader's chosen engine is a setting already, so an
/// engine that is not here is one the dialog can never show and [`compose`] can never
/// be asked for.
pub(crate) fn engines() -> [axiomd_engine::EngineId; 1] {
    [ComrakEngine::new().id()]
}

/// Parses and renders `source`, giving up as soon as `superseded` says the result is
/// no longer wanted.
fn compose(source: &str, name: &str, superseded: &dyn Fn() -> bool) -> Option<Rendered> {
    if superseded() {
        return None;
    }
    let parsed = ComrakEngine::new().parse(source, Extensions::FULL);

    if superseded() {
        return None;
    }
    Some(axiomd_render::render(&parsed, name))
}

/// The same document as one file that carries everything it needs — the export path's
/// half of the pipeline.
///
/// It is here rather than beside the exporter so that there is exactly one place that
/// decides which engine and which extensions a reader's document is read with: what
/// is exported is what the preview shows, because both are composed by this module
/// from the same buffer.
///
/// Slow by nature — it reads every picture the document names — so it is only ever
/// called on a worker (`export.rs`).
pub(crate) fn compose_standalone(
    source: &str,
    name: &str,
    embed: &dyn Fn(&str) -> Option<axiomd_render::Picture>,
) -> String {
    let parsed = ComrakEngine::new().parse(source, Extensions::FULL);
    axiomd_render::standalone(&parsed, name, embed)
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
            renderer.render("# Title\n\nBody text.\n".to_owned(), "notes".to_owned());
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
            renderer.render("# Title\n".to_owned(), "notes".to_owned());
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
            renderer.render("# Stale\n".to_owned(), "notes".to_owned());
            renderer.render("# Wanted\n".to_owned(), "notes".to_owned());
            harness.run_until_a_page_is_shown();
        });

        assert_eq!(shown.len(), 1, "more than the last request was shown");
        let html = shown[0].0.html();
        assert!(html.contains("Wanted"), "{html}");
        assert!(!html.contains("Stale"), "{html}");
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
