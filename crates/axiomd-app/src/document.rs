//! Files on disk turned into pages a window can show — always off the main loop.
//!
//! Reading, parsing and rendering are the only expensive things a viewer does, and
//! all three happen here, on a worker. The main loop hands over a path and gets a
//! [`Page`] back later; it never waits, so a huge document cannot make the window
//! stop drawing.
//!
//! A [`Renderer`] holds one document's worth of work at a time. Asking for another
//! render supersedes the one in flight: the worker abandons it at the next phase
//! boundary and its result is dropped rather than shown, so a window can never
//! display a document the user has already moved past. This is the machinery live
//! reload and mode switching build on.
//!
//! [`FileId`] is the other half: it answers "is this the file that window already
//! has open?" by identity rather than by spelling, so `./README.md`, an absolute
//! path and a symlink all name the same document.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axiomd_engine::{ComrakEngine, Extensions, MarkdownEngine};
use axiomd_render::Rendered;
use gtk::gio;
use gtk::glib;

/// What a window has to show for a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Page {
    /// The rendered document: the whole page for a view that has not shown this
    /// document yet, and its blocks alone for one that has.
    Rendered(Rendered),
    /// The document cannot be shown, and this is what the window says instead. Both
    /// strings are user-facing; opening never asks the user a question, so this is
    /// the whole of the report.
    Unavailable { title: String, detail: String },
}

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
    show: Rc<dyn Fn(Page)>,
}

impl Renderer {
    /// Builds a renderer that hands every page it finishes to `show`, on the thread
    /// that created it.
    pub(crate) fn new(show: impl Fn(Page) + 'static) -> Self {
        Self {
            generation: Arc::new(AtomicU64::new(0)),
            show: Rc::new(show),
        }
    }

    /// Starts rendering `path`, superseding whatever was in flight.
    ///
    /// Returns at once. The page arrives later through the callback, on the calling
    /// thread; a superseded render never arrives at all.
    pub(crate) fn render(&self, path: PathBuf) {
        let mine = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let generation = self.generation.clone();
        let show = self.show.clone();

        glib::spawn_future_local(async move {
            let worker = generation.clone();
            let composed = gio::spawn_blocking(move || {
                compose(&path, &|| worker.load(Ordering::SeqCst) != mine)
            })
            .await;

            if generation.load(Ordering::SeqCst) != mine {
                return;
            }
            match composed {
                Ok(Some(page)) => show(page),
                Ok(None) => {}
                Err(_) => show(Page::Unavailable {
                    title: "Could not render this document".to_owned(),
                    detail: "The renderer stopped unexpectedly. Try opening the file again."
                        .to_owned(),
                }),
            }
        });
    }
}

/// Reads, parses and renders `path`, giving up as soon as `superseded` says the
/// result is no longer wanted.
fn compose(path: &Path, superseded: &dyn Fn() -> bool) -> Option<Page> {
    let name = path
        .file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned();

    if superseded() {
        return None;
    }
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            return Some(Page::Unavailable {
                title: format!("Could not open {name}"),
                detail: format!("{error}."),
            });
        }
    };
    let source = match String::from_utf8(bytes) {
        Ok(source) => source,
        Err(_) => {
            return Some(Page::Unavailable {
                title: format!("Could not read {name}"),
                detail: "This file is not UTF-8 text, so it is not a Markdown document.".to_owned(),
            });
        }
    };

    if superseded() {
        return None;
    }
    let parsed = ComrakEngine::new().parse(&source, Extensions::FULL);

    if superseded() {
        return None;
    }
    Some(Page::Rendered(axiomd_render::render(&parsed)))
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
        shown: Rc<RefCell<Vec<(Page, ThreadId)>>>,
    }

    impl Harness {
        /// Runs `body` with a renderer whose deliveries are recorded, and returns the
        /// pages that reached the window, in arrival order.
        fn run(body: impl FnOnce(&Harness, &Renderer)) -> Vec<(Page, ThreadId)> {
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

        fn shown(&self) -> Vec<Page> {
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

    fn html_of(page: &Page) -> &str {
        match page {
            Page::Rendered(document) => document.html(),
            other => panic!("expected a rendered page, got {other:?}"),
        }
    }

    #[test]
    fn shows_the_document_the_file_holds() {
        let scratch = ScratchDir::new("render-ok");
        let file = scratch.write("notes.md", "# Title\n\nBody text.\n");

        let shown = Harness::run(|harness, renderer| {
            renderer.render(file.clone());
            harness.run_until_a_page_is_shown();
        });

        assert_eq!(shown.len(), 1);
        let html = html_of(&shown[0].0);
        assert!(html.contains("<h1 data-line=\"1\">Title</h1>"), "{html}");
        assert!(html.contains("Body text."), "{html}");
    }

    /// The main loop must be free while a document renders: nothing is delivered
    /// until the loop is driven, and the delivery lands on the thread that owns it.
    #[test]
    fn renders_without_blocking_the_thread_that_asked() {
        let scratch = ScratchDir::new("render-offthread");
        let file = scratch.write("notes.md", "# Title\n");
        let caller = std::thread::current().id();

        let shown = Harness::run(|harness, renderer| {
            renderer.render(file.clone());
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

    /// A window must never flash a document the user has already moved past.
    #[test]
    fn a_superseded_render_is_never_shown() {
        let scratch = ScratchDir::new("render-supersede");
        let stale = scratch.write("stale.md", "# Stale\n");
        let wanted = scratch.write("wanted.md", "# Wanted\n");

        let shown = Harness::run(|harness, renderer| {
            renderer.render(stale.clone());
            renderer.render(wanted.clone());
            harness.run_until_a_page_is_shown();
        });

        assert_eq!(shown.len(), 1, "more than the last request was shown");
        let html = html_of(&shown[0].0);
        assert!(html.contains("Wanted"), "{html}");
        assert!(!html.contains("Stale"), "{html}");
    }

    #[test]
    fn says_why_a_file_it_cannot_read_is_not_shown() {
        let scratch = ScratchDir::new("render-missing");
        let missing = scratch.path().join("gone.md");

        let shown = Harness::run(|harness, renderer| {
            renderer.render(missing.clone());
            harness.run_until_a_page_is_shown();
        });

        match &shown[0].0 {
            Page::Unavailable { title, detail } => {
                assert!(title.contains("gone.md"), "{title}");
                assert!(!detail.is_empty(), "the reason was left blank");
            }
            other => panic!("expected an unavailable page, got {other:?}"),
        }
    }

    #[test]
    fn says_why_a_file_that_is_not_text_is_not_shown() {
        let scratch = ScratchDir::new("render-binary");
        let file = scratch.write("image.md", [0xff, 0xfe, 0x00, 0x9f]);

        let shown = Harness::run(|harness, renderer| {
            renderer.render(file.clone());
            harness.run_until_a_page_is_shown();
        });

        match &shown[0].0 {
            Page::Unavailable { title, detail } => {
                assert!(title.contains("image.md"), "{title}");
                assert!(detail.contains("UTF-8"), "{detail}");
            }
            other => panic!("expected an unavailable page, got {other:?}"),
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
