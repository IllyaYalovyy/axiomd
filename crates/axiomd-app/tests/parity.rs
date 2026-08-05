//! What the package costs over the build (issue #36).
//!
//! The flatpak is a supported distribution and the owner reported it feeling slower
//! than the native build. This file is the answer as numbers: every metric below is
//! measured on both forms in the same run and printed as one line, and the packaged
//! form is held to a ceiling of its own that only ever comes down. The committed
//! evidence is `designs/flatpak-parity.md`, and [`the_committed_table_is_what_this_run_pins`]
//! is why it cannot go stale.
//!
//! # Reading the output
//!
//! ```text
//! parity: cold start to a typical document served... native 697.0 ms  flatpak 863.0 ms  ×1.24  (ceiling 1200.0 ms; parity 700.0 ms; 5 samples each)
//! ```
//!
//! Three numbers: what the build costs, what the package costs, and the multiple
//! between them. The **ceiling** is what this run is held to; **parity** is the native
//! form's own figure, which is what the package is being walked towards. Only the
//! packaged number is enforced — the native budgets are pinned in `perf.rs` and pinning
//! them twice would fail a packaging run for a reason that has nothing to do with the
//! package.
//!
//! Run them with the gate's hook, which needs the flatpak built and installed first:
//!
//! ```text
//! ./scripts/quality.d/40-flatpak.sh        # builds and installs it
//! ./scripts/quality.d/50-flatpak-perf.sh   # measures it against the build
//! ```
//!
//! # Why they are `#[ignore]`
//!
//! Two reasons at once, and each alone would be enough. They need a flatpak installed
//! on the machine, which not every developer has; and like every budget in this project
//! they mean nothing in a debug build, so the hook above runs this target in release,
//! one test at a time.

use std::time::Instant;

use axiomd_e2e::budget::Budget;
use axiomd_e2e::parity::{Form, Metric};
use axiomd_e2e::{App, Fixture, corpus, parity};

/// Where the committed evidence lives.
const EVIDENCE: &str = "designs/flatpak-parity.md";

/// The generated part of it, between these markers.
const BEGIN: &str = "<!-- pinned:begin -->";
const END: &str = "<!-- pinned:end -->";

/// Every metric the parity table covers: the scenarios issue #36 names, each measured
/// on both forms.
///
/// The ceilings were measured on 2026-08-05 and are about 1.35× what was seen, so a
/// busy machine is not a failing gate — the same headroom the native budgets carry. What
/// each aims at is the native form's own measured figure: parity, literally. The machine
/// and the numbers are in that task's report and in the evidence file.
const METRICS: &[Metric] = &[
    Metric {
        what: "cold start to a typical document served",
        measures: "the process starting to the document on screen, sandbox and all",
        packaged: Budget::millis(1_200, 700),
    },
    Metric {
        what: "the application's own share of a cold start",
        measures: "the same launch with everything before axiomd's first instruction \
                   taken off it, ending when the document's bytes leave the handler",
        packaged: Budget::millis(750, 430),
    },
    Metric {
        what: "a document opened from the desktop",
        measures: "a launch with the document handed over the way Files hands it — \
                   through the document portal for the package",
        packaged: Budget::millis(1_200, 700),
    },
    Metric {
        what: "a second document opened into a running application",
        measures: "a window and its document with the application already up — a \
                   launch with no launch in it",
        packaged: Budget::millis(540, 430),
    },
    Metric {
        what: "a changed typical file on screen",
        measures: "a file changing on disk to the reader seeing it, including the \
                   150 ms a burst of writes is coalesced over",
        packaged: Budget::millis(320, 240),
    },
    Metric {
        what: "one window on a typical document",
        measures: "every process the launch is made of, resident",
        packaged: Budget::megabytes(600, 570),
    },
];

/// The metric called `what`, so a measurement can only ever be of a metric the
/// committed table pins.
fn metric(what: &'static str) -> &'static Metric {
    METRICS
        .iter()
        .find(|metric| metric.what == what)
        .unwrap_or_else(|| panic!("{what:?} is not one of the parity metrics"))
}

// ---------------------------------------------------------------------------
// The measurements
// ---------------------------------------------------------------------------

/// A launch, from the process starting to the document being on screen.
///
/// Timed from outside rather than from the application's own clock, because the sandbox
/// is built before there is an application to ask: `App::startup` cannot see the part of
/// a packaged launch this metric exists to measure.
#[test]
#[ignore = "measures the installed flatpak; run ./scripts/quality.d/50-flatpak-perf.sh"]
fn a_typical_document_is_served_by_the_package_as_it_is_by_the_build() {
    let fixture = Fixture::new("parity-cold-start");
    let document = fixture.write("notes.md", &corpus::typical());

    parity::time(metric("cold start to a typical document served"), |form| {
        let app = form.launch(&document);
        let took = app.launched_in();
        assert!(
            app.dom_text("h1").contains("Performance corpus"),
            "the launch that was timed had not rendered the document",
        );
        took
    });
}

/// The same launch, with everything before axiomd's first instruction taken off.
///
/// Measured beside the whole launch on purpose: the difference between the two metrics
/// is, per form, everything that is not axiomd's own code — `execve`, the dynamic
/// loader, and for the package the sandbox. Comparing those two differences is what says
/// whether the package's overhead is the sandbox being built or axiomd being slower
/// inside it, which is the question issue #36 asks and the one a guess would answer
/// wrong.
#[test]
#[ignore = "measures the installed flatpak; run ./scripts/quality.d/50-flatpak-perf.sh"]
fn the_application_does_its_own_share_of_a_launch_at_the_same_speed_in_both_forms() {
    let fixture = Fixture::new("parity-own-share");
    let document = fixture.write("notes.md", &corpus::typical());

    parity::time(
        metric("the application's own share of a cold start"),
        |form| {
            let app = form.launch(&document);
            let took = app.startup();
            assert!(
                app.dom_text("h1").contains("Performance corpus"),
                "the launch that was timed had not rendered the document",
            );
            took
        },
    );
}

/// The same launch as the desktop makes it: a double-click in Files.
///
/// The route differs by form and the difference is the point — the package is handed a
/// name the document portal invented, after a round trip to that portal, while the build
/// is handed the reader's own path (issue #22).
#[test]
#[ignore = "measures the installed flatpak; run ./scripts/quality.d/50-flatpak-perf.sh"]
fn a_document_opened_from_the_desktop_arrives_by_either_route() {
    let fixture = Fixture::new("parity-portal");
    let document = fixture.write("notes.md", &corpus::typical());

    parity::time(metric("a document opened from the desktop"), |form| {
        let app = form.from_the_desktop(&document);
        let took = app.launched_in();
        assert!(
            app.dom_text("h1").contains("Performance corpus"),
            "the launch that was timed had not rendered the document",
        );
        took
    });
}

/// A second document opened into an application that is already running — the warm
/// start, where nothing of the sandbox is built and nothing of the runtime is loaded.
///
/// What is left is a window and a web process, which is what says whether the packaged
/// form's overhead is the sandbox or everything it does afterwards.
#[test]
#[ignore = "measures the installed flatpak; run ./scripts/quality.d/50-flatpak-perf.sh"]
fn a_second_document_opens_into_a_running_application_in_either_form() {
    let fixture = Fixture::new("parity-warm-open");
    let document = fixture.write("notes.md", &corpus::typical());
    let mut running = Running::none();
    // A document of its own for every sample: axiomd shows a document it already has
    // open by presenting the window it is in, so opening the same file twice measures a
    // window being raised rather than a document arriving.
    let mut opened = 0;

    parity::time(
        metric("a second document opened into a running application"),
        |form| {
            let app = running.of(form, || form.launch(&document));
            opened += 1;
            let another = fixture.write(&format!("more-notes-{opened}.md"), &corpus::typical());
            let windows = app.window_count();
            let began = Instant::now();
            app.open(&another);
            let took = began.elapsed();
            assert_eq!(
                app.window_count(),
                windows + 1,
                "the open that was timed did not put a window up",
            );
            took
        },
    );
}

/// A file changing on disk to the reader seeing the change, in both forms.
///
/// The watch is inotify on a directory the sandbox was granted, so this is also where a
/// bind mount that does not deliver events at the same speed would show.
#[test]
#[ignore = "measures the installed flatpak; run ./scripts/quality.d/50-flatpak-perf.sh"]
fn a_changed_file_reaches_the_reader_in_either_form() {
    let fixture = Fixture::new("parity-reload");
    // A document per form: both are running at once, and a file one of them is watching
    // must not be rewritten by the other's samples.
    let mut documents = [corpus::typical(), corpus::typical()];
    let paths = [
        fixture.write("native.md", &documents[0]),
        fixture.write("packaged.md", &documents[1]),
    ];
    let mut running = Running::none();
    let mut change = 0;

    parity::time(metric("a changed typical file on screen"), |form| {
        let nth = usize::from(form == Form::Packaged);
        let app = running.of(form, || form.launch(&paths[nth]));
        let loads = app.navigation_count();
        change += 1;
        let marker = format!("Revision {change} of this document");
        documents[nth].push_str(&format!("\n{marker}\n"));
        let began = Instant::now();
        std::fs::write(&paths[nth], &documents[nth]).expect("write the changed document");
        app.wait_until(&format!("document.body.textContent.includes({marker:?})"));
        let took = began.elapsed();
        assert_eq!(
            app.navigation_count(),
            loads,
            "the changed document was shown by reloading the page, not by patching it",
        );
        took
    });
}

/// What one window on a typical document costs, in both forms: the application, the web
/// process its document is rendered in, and the network process beside them.
#[test]
#[ignore = "measures the installed flatpak; run ./scripts/quality.d/50-flatpak-perf.sh"]
fn one_window_costs_the_package_what_it_costs_the_build() {
    let fixture = Fixture::new("parity-memory");
    let document = fixture.write("notes.md", &corpus::typical());

    parity::memory(metric("one window on a typical document"), |form| {
        let app = form.launch(&document);
        assert!(
            app.showing_document(),
            "the window measured has no document"
        );
        app.footprint().bytes
    });
}

/// The applications a metric about a *running* application measures against — one per
/// form, started on the first sample and kept for the whole of it.
struct Running {
    native: Option<App>,
    packaged: Option<App>,
}

impl Running {
    fn none() -> Running {
        Running {
            native: None,
            packaged: None,
        }
    }

    /// The application of `form`, started with `start` if this is the first sample.
    fn of(&mut self, form: Form, start: impl FnOnce() -> App) -> &App {
        let slot = match form {
            Form::Native => &mut self.native,
            Form::Packaged => &mut self.packaged,
        };
        slot.get_or_insert_with(start)
    }
}

// ---------------------------------------------------------------------------
// The committed evidence
// ---------------------------------------------------------------------------

/// The table in `designs/flatpak-parity.md` is the table this harness pins.
///
/// A ceiling lowered, a metric added or a metric renamed without the evidence moving
/// with it fails here, printing the text the file should hold — the same contract the
/// engine comparison keeps (`designs/engine-comparison.md`). Evidence that can drift
/// from the thing it is evidence of is not evidence.
#[test]
fn the_committed_table_is_what_this_run_pins() {
    let (path, text) = evidence();
    let pinned = parity::table(METRICS);
    let pinned = pinned.trim_matches('\n');
    let committed = generated_block(&text, &path);

    assert_eq!(
        committed,
        pinned,
        "\n{} no longer says what the parity harness pins. It should say:\n\
         \n{BEGIN}\n\n{pinned}\n\n{END}\n",
        path.display(),
    );
}

/// Every metric the table pins has a measured number and an explanation behind it.
///
/// The generated block is only the ceilings; what a run actually measured, and what the
/// difference between the forms was found to be made of, are written by a person. A
/// metric added without either is a ceiling nobody has evidence for.
#[test]
fn every_pinned_metric_has_its_measurement_and_its_explanation() {
    let (path, text) = evidence();
    for section in ["## Measured", "## Where the overhead goes"] {
        let (_, after) = text
            .split_once(section)
            .unwrap_or_else(|| panic!("{}: there is no {section} section", path.display()));
        let body = after.split("\n## ").next().unwrap_or(after);
        for metric in METRICS {
            assert!(
                body.contains(metric.what),
                "{}: {section} says nothing about {:?}",
                path.display(),
                metric.what,
            );
        }
    }
}

/// Every metric is measured on both forms, and only on metrics the table knows.
///
/// The names below are the ones the measurements above ask for; a metric renamed in the
/// table without its test following fails here rather than in a run nobody made.
#[test]
fn every_metric_the_table_pins_is_one_a_test_measures() {
    let measured = [
        "cold start to a typical document served",
        "the application's own share of a cold start",
        "a document opened from the desktop",
        "a second document opened into a running application",
        "a changed typical file on screen",
        "one window on a typical document",
    ];
    for metric in METRICS {
        assert!(
            measured.contains(&metric.what),
            "no test measures {:?}, so its ceiling is a number nobody checks",
            metric.what,
        );
    }
    for what in measured {
        metric(what);
    }
}

fn evidence() -> (std::path::PathBuf, String) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("the workspace root above crates/axiomd-app")
        .join(EVIDENCE);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    (path, text)
}

fn generated_block<'a>(text: &'a str, path: &std::path::Path) -> &'a str {
    let (_, after) = text
        .split_once(BEGIN)
        .unwrap_or_else(|| panic!("{}: no {BEGIN} marker", path.display()));
    let (block, _) = after
        .split_once(END)
        .unwrap_or_else(|| panic!("{}: no {END} marker", path.display()));
    block.trim_matches('\n')
}
