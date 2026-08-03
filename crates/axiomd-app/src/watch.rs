//! Watching the file a window is showing, and saying so once the writing stops.
//!
//! One save is rarely one write. An editor truncates and rewrites; a formatter runs
//! after it; a linter touches the file again; some editors write a new file beside the
//! old one and rename it over the top, which is a deletion and a creation. Reporting
//! each of those separately would put the reader through a re-render per write, so a
//! [`FileWatch`] reports a change only once the file has been quiet for [`QUIET`] —
//! and reports it again for the next burst, however that burst is spelled.
//!
//! # What is watched is the path, not the file
//!
//! The whole point of an editor's write-and-rename is that the file the window opened
//! stops existing. GLib's file monitor watches the name inside its directory rather
//! than the inode behind it, so the replacement, and every save after it, still
//! arrives here — verified by `a_file_replaced_by_a_rename_is_reported_and_watched_on`
//! below, which replaces a watched file and then saves over it again.
//!
//! # Nothing outlives the window
//!
//! Dropping the watch cancels the monitor and abandons a report that has not fired
//! yet, so a closed window cannot be woken by its old document (invariant 7).

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

/// How long the file must be quiet before a change is reported. RFC-001's debounce,
/// and the same number the editor's own preview will use.
const QUIET: Duration = Duration::from_millis(150);

/// Watches one file for as long as it exists.
pub(crate) struct FileWatch {
    /// `None` when the file could not be watched at all, which costs the window its
    /// live reload and nothing else.
    monitor: Option<gio::FileMonitor>,
}

impl FileWatch {
    /// Watches `file`, calling `changed` on the main loop once it has settled after a
    /// change — however many writes that change took.
    pub(crate) fn new(file: &Path, changed: impl Fn() + 'static) -> Self {
        let debounce = Rc::new(Debounce::new(changed));
        let monitor = gio::File::for_path(file)
            .monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE);
        let monitor = match monitor {
            Ok(monitor) => {
                monitor.connect_changed(move |_, _, _, event| {
                    if may_change_the_document(event) {
                        debounce.nudge();
                    }
                });
                Some(monitor)
            }
            Err(error) => {
                // Explicit rather than silent: the document is still readable, but it
                // has stopped following the file and the reader would never know.
                eprintln!(
                    "axiomd: {} will not reload when it changes: {error}",
                    file.display(),
                );
                None
            }
        };
        Self { monitor }
    }
}

impl Drop for FileWatch {
    fn drop(&mut self) {
        if let Some(monitor) = self.monitor.take() {
            monitor.cancel();
        }
    }
}

/// Whether an event can mean the document's text is now different.
///
/// Everything is taken to mean it except the events that cannot: a permission or
/// timestamp change is not a save, and an unmount takes the file away without
/// rewriting it. An event this build has never heard of counts as a change, because
/// re-reading a file that did not change costs a render and missing one that did costs
/// the reader their document.
fn may_change_the_document(event: gio::FileMonitorEvent) -> bool {
    !matches!(
        event,
        gio::FileMonitorEvent::AttributeChanged
            | gio::FileMonitorEvent::PreUnmount
            | gio::FileMonitorEvent::Unmounted
    )
}

/// Turns a burst of events into one report, once the burst is over.
struct Debounce {
    /// Which nudge is the current one. Every nudge supersedes the ones before it, the
    /// same way a new render supersedes the parse in flight.
    generation: Cell<u64>,
    report: Box<dyn Fn()>,
}

impl Debounce {
    fn new(report: impl Fn() + 'static) -> Self {
        Self {
            generation: Cell::new(0),
            report: Box::new(report),
        }
    }

    /// Notes that something happened, and reports it once nothing else has.
    fn nudge(self: &Rc<Self>) {
        let mine = self.generation.get() + 1;
        self.generation.set(mine);

        let debounce = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            glib::timeout_future(QUIET).await;
            // Gone with its window, or superseded by a later nudge: either way this
            // one has nothing left to report.
            if let Some(debounce) = debounce.upgrade()
                && debounce.generation.get() == mine
            {
                (debounce.report)();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Instant;

    use super::*;
    use crate::testing::ScratchDir;

    /// How long a change is allowed to take to arrive before the test calls it lost.
    /// Far above any real notification; it exists so a watch that never reports fails
    /// the suite instead of hanging it.
    const WATCHDOG: Duration = Duration::from_secs(30);

    /// How long the loop runs between checks while waiting for a report.
    const TICK: Duration = Duration::from_millis(250);

    /// A watch on a real file, on a main loop of its own.
    ///
    /// Private rather than the process-wide default context, because the test binary
    /// runs tests on several threads at once and a `MainContext` may only be owned by
    /// one of them.
    struct Trial {
        main_loop: glib::MainLoop,
    }

    /// What a watch reported, as the loop that drove it saw it.
    type Reports = Rc<Cell<u32>>;

    impl Trial {
        fn run(body: impl FnOnce(&Trial)) {
            let context = glib::MainContext::new();
            context
                .with_thread_default(|| {
                    let trial = Trial {
                        main_loop: glib::MainLoop::new(Some(&context), false),
                    };
                    body(&trial);
                })
                .expect("own a private main context");
        }

        /// Watches `file`, counting what it reports and stopping the loop at each
        /// report so the test regains control the moment something happens.
        fn watch(&self, file: &Path) -> (FileWatch, Reports) {
            let reports = Rc::new(Cell::new(0));
            let counted = reports.clone();
            let stop = self.main_loop.clone();
            let watch = FileWatch::new(file, move || {
                counted.set(counted.get() + 1);
                stop.quit();
            });
            (watch, reports)
        }

        /// Drives the loop until `reports` reaches `wanted`, or fails the test.
        fn run_until_reported(&self, reports: &Reports, wanted: u32) {
            let deadline = Instant::now() + WATCHDOG;
            while reports.get() < wanted {
                assert!(
                    Instant::now() < deadline,
                    "waited {WATCHDOG:?} for {wanted} changes and only {} arrived",
                    reports.get(),
                );
                let stop = self.main_loop.clone();
                glib::spawn_future_local(async move {
                    glib::timeout_future(TICK).await;
                    stop.quit();
                });
                self.main_loop.run();
            }
        }

        /// Drives the loop for one tick, so that anything the watch was going to
        /// report has had its chance to arrive.
        fn settle(&self) {
            let stop = self.main_loop.clone();
            glib::spawn_future_local(async move {
                glib::timeout_future(TICK).await;
                stop.quit();
            });
            self.main_loop.run();
        }
    }

    fn save(file: &PathBuf, contents: &str) {
        std::fs::write(file, contents).expect("save the file");
    }

    /// A save that arrives as five separate writes — an editor, then a formatter — is
    /// one change to the reader.
    ///
    /// Deterministic rather than timed: the loop cannot turn between the nudges, so
    /// there is no scheduling under which the burst could be reported twice.
    #[test]
    fn a_burst_of_changes_is_reported_once() {
        let reported = Rc::new(Cell::new(0u32));
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                let main_loop = glib::MainLoop::new(Some(&context), false);
                let counted = reported.clone();
                let debounce = Rc::new(Debounce::new(move || counted.set(counted.get() + 1)));

                for _ in 0..5 {
                    debounce.nudge();
                }
                run_past_the_quiet_period(&main_loop);
                assert_eq!(
                    reported.get(),
                    1,
                    "a burst of five writes was not coalesced"
                );

                // And the next burst is a change of its own: a debounce that reported
                // once and then went quiet forever would strand the reader on the
                // version they happened to have.
                debounce.nudge();
                debounce.nudge();
                run_past_the_quiet_period(&main_loop);
                assert_eq!(reported.get(), 2, "the second burst was never reported");
            })
            .expect("own a private main context");
    }

    fn run_past_the_quiet_period(main_loop: &glib::MainLoop) {
        let stop = main_loop.clone();
        glib::spawn_future_local(async move {
            glib::timeout_future(QUIET * 4).await;
            stop.quit();
        });
        main_loop.run();
    }

    #[test]
    fn a_file_saved_in_place_is_reported() {
        let scratch = ScratchDir::new("watch-save");
        let file = scratch.write("notes.md", "# One\n");

        Trial::run(|trial| {
            let (_watch, reports) = trial.watch(&file);

            save(&file, "# Two\n");

            trial.run_until_reported(&reports, 1);
        });
    }

    /// How vim, emacs and VS Code save: the file the watch was given stops existing
    /// and another takes its name. The reader must get that save, and the one after
    /// it — a watch that followed the old inode would go silent here.
    #[test]
    fn a_file_replaced_by_a_rename_is_reported_and_watched_on() {
        let scratch = ScratchDir::new("watch-rename");
        let file = scratch.write("notes.md", "# One\n");
        let replacement = scratch.write("notes.md.new", "# Two\n");

        Trial::run(|trial| {
            let (_watch, reports) = trial.watch(&file);

            std::fs::rename(&replacement, &file).expect("rename the new file over the old");
            trial.run_until_reported(&reports, 1);

            save(&file, "# Three\n");
            trial.run_until_reported(&reports, 2);
        });
    }

    /// A file that goes away is a change like any other: the window has to hear about
    /// it to say so beside the document it is still showing.
    #[test]
    fn a_deleted_file_is_reported() {
        let scratch = ScratchDir::new("watch-delete");
        let file = scratch.write("notes.md", "# One\n");

        Trial::run(|trial| {
            let (_watch, reports) = trial.watch(&file);

            std::fs::remove_file(&file).expect("delete the file");

            trial.run_until_reported(&reports, 1);
        });
    }

    /// Closing a window frees what it held (invariant 7): its watch stops watching,
    /// and a file that changes afterwards reports to nobody.
    ///
    /// The second file is the synchronisation point, so this proves a negative without
    /// waiting on a clock: both watches share one notification channel and both files
    /// are written in order, so a report for the second file means anything the first
    /// would have produced has already been and gone.
    #[test]
    fn a_dropped_watch_reports_nothing() {
        let scratch = ScratchDir::new("watch-dropped");
        let closed = scratch.write("closed.md", "# One\n");
        let open = scratch.write("open.md", "# One\n");

        Trial::run(|trial| {
            let (dropped, unwanted) = trial.watch(&closed);
            let (_watch, reports) = trial.watch(&open);

            drop(dropped);
            save(&closed, "# Two\n");
            save(&open, "# Two\n");

            trial.run_until_reported(&reports, 1);
            trial.settle();

            assert_eq!(
                unwanted.get(),
                0,
                "a dropped watch reported a change to the window that let go of it",
            );
        });
    }
}
