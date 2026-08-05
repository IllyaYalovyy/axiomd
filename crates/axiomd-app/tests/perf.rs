//! The performance budgets, as tests (issue #9, cross-cutting invariant 8).
//!
//! VISION states axiomd's speed as numbers — cold start under 300 ms on a typical
//! file, a 10 MB document usable, ten windows without cross-window slowdown, memory
//! per window bounded and freed. This file is those numbers, measured on the real
//! application and printed whether they pass or fail. A change that breaks one of them
//! is not done, whatever else it fixes.
//!
//! # Reading the output
//!
//! ```text
//! perf: cold start to a typical document served.... 411.1 ms  (ceiling 560.0 ms; target 300.0 ms; …)
//! ```
//!
//! Two numbers, because they are not the same number. The **ceiling** is what the gate
//! enforces: today's honest measurement with headroom, and it only ever comes down.
//! The **target** is VISION's own figure. Where they differ, every run says how far
//! there is left to go — see `axiomd_e2e::budget`.
//!
//! Run them with the gate's perf hook, which builds release and shows the numbers:
//!
//! ```text
//! ./scripts/quality.d/20-perf.sh          # the quick ones, which the gate runs
//! AXIOMD_PERF_SOAK=1 ./scripts/quality.d/20-perf.sh   # and the ten-megabyte ones
//! ```
//!
//! # Why they are `#[ignore]`
//!
//! `cargo test` builds debug, where comrak, syntect and ammonia are an order of
//! magnitude off what anybody ships — a budget measured there measures nothing. The
//! hook above runs this target in release with `--ignored`, one test at a time, so no
//! two budgets are ever measured while competing for the same machine. Measuring one
//! in a debug build fails rather than reporting a meaningless number.
//!
//! # Raising a ceiling is not something this file may do
//!
//! Lowering one is ordinary work. Raising one means axiomd got slower and somebody
//! accepted that, which is the project owner's decision and nobody else's (issue #9).
//! Every ceiling below was measured on 2026-08-03; the machine and the numbers are in
//! that task's report.

use std::time::{Duration, Instant};

use axiomd_e2e::budget::Budget;
use axiomd_e2e::{Fixture, budget, corpus, launch};

/// The ceilings, each with the figure it is being walked towards, and each about
/// 1.35× what was measured on 2026-08-03 so that a busy machine is not a failing gate.
/// Down only — see the module note above.
mod budgets {
    use super::Budget;

    /// VISION's own number: cold start to a rendered typical file, under 300 ms.
    ///
    /// Measured at 411 ms. Roughly 60 ms of it is GTK starting, 135 ms is WebKit
    /// building a web context, 100 ms is the render — most of that the syntax
    /// highlighter deserialising its grammars on first use — and the remaining 200 ms
    /// is WebKit launching the web process that then asks for the page.
    pub(super) const COLD_START: Budget = Budget::millis(560, 300);

    /// Parsing and rendering ten megabytes, off the main thread.
    ///
    /// Measured at 9.1 s, of which 0.4 s is the parse and 8.3 s is the syntax
    /// highlighter: RFC-001's block cache is a contingency for re-parsing, and
    /// re-parsing is not what this costs (see the task report for issue #9).
    pub(super) const TEN_MEGABYTE_RENDER: Budget = Budget::millis(12_500, 1_000);

    /// Block structure nested 200 deep. Not a speed budget so much as a guard: a
    /// parser or renderer that goes quadratic in nesting depth shows it here. Measured
    /// at 1.6 ms, which is where it should be.
    pub(super) const PATHOLOGICAL_RENDER: Budget = Budget::millis(100, 100);

    /// A file changing on disk to the reader seeing it, for a typical document.
    /// Includes the deliberate 150 ms quiet period a burst of writes is coalesced
    /// over (`watch.rs`), so the target is that quiet period plus RFC-001's 250 ms for
    /// the work itself. Measured at 215 ms — already inside its target.
    pub(super) const RELOAD: Budget = Budget::millis(400, 400);

    /// The same for a ten megabyte document, measured at 17.6 s. Nowhere near its
    /// target: a re-render today is a whole-document render *and* a whole-document
    /// patch, and the second half is as expensive as the first.
    pub(super) const HEAVY_RELOAD: Budget = Budget::millis(23_000, 400);

    /// A key press reaching the buffer of a ten megabyte document. Typing must cost a
    /// keystroke, never a parse (`window.rs`). Measured at 1.4 ms.
    pub(super) const KEYSTROKE: Budget = Budget::millis(50, 50);

    /// The longest the application may take to answer anything while an edit to a ten
    /// megabyte document is on its way to the screen. Invariant 4 as a number: the
    /// main loop does not block, whatever is being parsed. Measured at 2.7 s — it does
    /// block, and the ceiling says by how much until it stops.
    pub(super) const STALL_WHILE_RENDERING: Budget = Budget::millis(4_000, 250);

    /// Cold start to a document of a thousand sections served — the outline's own
    /// document (issue #35). Not a startup budget so much as a guard on the sidebar:
    /// a panel that builds its tree in anything worse than a pass over the headings
    /// shows it here, against the same launch measured for a typical file. Measured at
    /// 409-423 ms on 2026-08-04, against 411 ms for a typical document: a thousand rows
    /// cost about a hundredth of a launch.
    pub(super) const THOUSAND_HEADING_START: Budget = Budget::millis(580, 300);

    /// Folding a thousand-section outline down to its top row and opening it again —
    /// nine hundred and ninety-nine rows out of the list and back in, which is the
    /// worst a chevron can cost. Two frames at 60 Hz is what a reader would call
    /// instant, and it is what this is aimed at. Measured at 32-35 ms for the pair on
    /// 2026-08-04 — a frame each way, including the round trip that asks for it.
    pub(super) const OUTLINE_FOLD: Budget = Budget::millis(50, 33);

    /// One window showing a typical document — the application, the web process its
    /// document is rendered in, and the network process beside them. Measured at
    /// 544 MB; WebKit's own floor dominates it (RFC-001 measured 200–300 MB for a web
    /// process).
    pub(super) const ONE_WINDOW: Budget = Budget::megabytes(720, 720);

    /// What each further window adds, averaged over nine of them, measured at 277 MB.
    /// This is the number "ten windows, bounded memory per window" is about.
    pub(super) const EACH_FURTHER_WINDOW: Budget = Budget::megabytes(360, 360);

    /// What is left after nine of ten windows are closed, measured at 639 MB. Closing
    /// a window frees what it owned (invariant 7), so the figure this is aimed at is
    /// one window's worth again — the difference being what a process that has been
    /// busy does not hand back to the kernel.
    pub(super) const AFTER_CLOSING_NINE: Budget = Budget::megabytes(850, 720);
}

/// How many windows the multi-window budgets open. VISION's number.
const MANY_WINDOWS: usize = 10;

/// Comfortably longer than an edit to a ten megabyte document takes to reach the
/// screen, which the reload budget beside it measures at about 18 seconds. It is how
/// long the stall budget watches the application for after an edit.
const A_WHOLE_HEAVY_RE_RENDER: Duration = Duration::from_secs(25);

/// How many sections the outline budgets are measured on. Issue #35's number, and far
/// past anything but a generated document.
const MANY_HEADINGS: usize = 1_000;

/// The document the 10 MB budgets are measured on.
fn ten_megabytes() -> String {
    corpus::of_size(10 * 1024 * 1024)
}

/// Cold start: from the application building itself to a typical document's own bytes
/// leaving the `axiomd://` handler.
///
/// VISION's headline number and the one a reader feels: `xdg-open README.md` and the
/// document is there.
#[test]
#[ignore = "a perf budget; run ./scripts/quality.d/20-perf.sh, which builds release"]
fn a_typical_document_is_served_within_the_cold_start_budget() {
    let fixture = Fixture::new("perf-cold-start");
    let document = fixture.write("notes.md", &corpus::typical());

    budget::time(
        "cold start to a typical document served",
        budgets::COLD_START,
        || {
            let app = launch(&document);
            let took = app.startup();
            assert!(
                app.dom_text("h1").contains("Performance corpus"),
                "the launch that was timed had not rendered the document",
            );
            took
        },
    );
}

/// A thousand sections in the sidebar, from a standing start (issue #35).
///
/// The outline is rebuilt on every render, so the shape of that rebuild is on the path
/// of every keystroke in edit mode as well as of the launch measured here.
#[test]
#[ignore = "a perf budget; run ./scripts/quality.d/20-perf.sh, which builds release"]
fn a_thousand_section_outline_is_built_within_its_budget() {
    let fixture = Fixture::new("perf-outline-build");
    let document = fixture.write("sections.md", &corpus::with_headings(MANY_HEADINGS));

    budget::time(
        "cold start to a 1000-section document served",
        budgets::THOUSAND_HEADING_START,
        || {
            let app = launch(&document);
            let took = app.startup();
            assert_eq!(
                app.outline().rows.len(),
                MANY_HEADINGS,
                "the launch that was timed had not listed the document's sections",
            );
            took
        },
    );
}

/// Folding that outline away and opening it again, which is the sidebar's own
/// interaction and the one that moves the most rows at once (issue #35).
///
/// Measured as the pair, so every sample is the same work: the fold takes 999 rows out
/// of the list and the unfold puts them back. The number includes the round trip over
/// the control channel, which is what a press costs on top of the work itself.
#[test]
#[ignore = "a perf budget; run ./scripts/quality.d/20-perf.sh, which builds release"]
fn folding_a_thousand_section_outline_stays_within_a_frame_or_two() {
    let fixture = Fixture::new("perf-outline-fold");
    let document = fixture.write("sections.md", &corpus::with_headings(MANY_HEADINGS));
    let app = launch(&document);
    assert_eq!(app.outline().rows.len(), MANY_HEADINGS);

    budget::time(
        "a 1000-section outline folded away and opened again",
        budgets::OUTLINE_FOLD,
        || {
            let began = Instant::now();
            app.toggle_section("Heading corpus");
            app.toggle_section("Heading corpus");
            let took = began.elapsed();
            assert_eq!(
                app.outline().rows.len(),
                MANY_HEADINGS,
                "the outline did not come back from being folded away",
            );
            took
        },
    );
}

/// A typical file changing on disk to the reader seeing the change — and seeing it in
/// the page they were already looking at.
#[test]
#[ignore = "a perf budget; run ./scripts/quality.d/20-perf.sh, which builds release"]
fn a_changed_typical_file_reaches_the_reader_within_the_reload_budget() {
    let fixture = Fixture::new("perf-reload");
    let mut document = corpus::typical();
    let path = fixture.write("notes.md", &document);
    let app = launch(&path);
    let loads = app.navigation_count();

    let mut change = 0;
    budget::time("a changed typical file on screen", budgets::RELOAD, || {
        change += 1;
        let marker = format!("Revision {change} of this document");
        document.push_str(&format!("\n{marker}\n"));
        let began = Instant::now();
        std::fs::write(&path, &document).expect("write the changed document");
        app.wait_until(&format!("document.body.textContent.includes({marker:?})"));
        began.elapsed()
    });

    assert_eq!(
        app.navigation_count(),
        loads,
        "the changed document was shown by reloading the page, not by patching it",
    );
}

/// A reader ticking an item off a long tracker, from the press to the box showing it
/// (issue #38).
///
/// Held to the reload budget beside it, because it is the same work — a document
/// re-rendered and patched into the page the reader is already on — and that budget is
/// the ceiling the project already keeps it under. It has room to spare in it: a
/// reload includes the 150 ms quiet period a burst of saves is coalesced over, and a
/// press has no such period to wait out.
///
/// What it guards is the patch. A tracker's whole list is one block, so a press is the
/// case where the patch walks *into* a block instead of replacing it, item by item,
/// which is how the reader is left where they were pressing. That walk is work the
/// replacement never did, and this is where it would show.
#[test]
#[ignore = "a perf budget; run ./scripts/quality.d/20-perf.sh, which builds release"]
fn pressing_a_task_box_settles_within_the_re_render_budget() {
    let fixture = Fixture::new("perf-task-toggle");
    let path = fixture.write("tracker.md", &long_tracker());
    let app = launch(&path);
    let loads = app.navigation_count();

    let mut item = 0;
    budget::time("a pressed task box on screen", budgets::RELOAD, || {
        item += 1;
        let began = Instant::now();
        app.click(&format!(
            "li.task-list-item:nth-of-type({item}) a.task-toggle"
        ));
        app.wait_until(&format!(
            "document.querySelectorAll('li.task-list-item input')[{}].checked",
            item - 1
        ));
        began.elapsed()
    });

    assert_eq!(
        app.navigation_count(),
        loads,
        "pressing a box reloaded the page instead of patching it",
    );
}

/// The tracker the toggle budget is measured on: one list, long enough that replacing
/// it wholesale would be the expensive thing the patch must not do.
fn long_tracker() -> String {
    let mut source = String::from("# Tracker\n\n");
    for item in 1..=TRACKED {
        source.push_str(&format!(
            "- [ ] item {item}, and what is left to do about it\n\n"
        ));
    }
    source
}

/// How many items that tracker has. Far past a person's list, which is the point.
const TRACKED: usize = 500;

/// The pathological case: block structure nested far past anything a person writes.
///
/// The budget is small on purpose. It is not about the 200 lines of input — it is
/// about a parser or renderer whose cost is quadratic in depth, which passes every
/// other test in this project and hangs the app on one hostile document.
#[test]
#[ignore = "a perf budget; run ./scripts/quality.d/20-perf.sh, which builds release"]
fn deeply_nested_blocks_do_not_take_time_out_of_proportion_to_their_size() {
    use axiomd_engine::{ComrakEngine, Extensions, MarkdownEngine};

    let source = corpus::deeply_nested();
    let plugins = axiomd_render::Plugins::builtin(&[]);

    budget::time(
        "parse and render of 200-deep nesting",
        budgets::PATHOLOGICAL_RENDER,
        || {
            let began = Instant::now();
            let parsed = ComrakEngine::new().parse(&source, Extensions::FULL);
            let rendered =
                axiomd_render::render(&parsed, "nested", &plugins, &axiomd_render::Folder::empty());
            let took = began.elapsed();
            assert!(rendered.html().contains("Bottom."), "the document was lost");
            took
        },
    );
}

/// What one window costs, what each further window costs, and what closing them gives
/// back.
///
/// One launch for all three because they are one measurement: the interesting numbers
/// are differences, and a difference between two launches is a difference between two
/// machines' moods as much as between one window and ten.
#[test]
#[ignore = "a perf budget; run ./scripts/quality.d/20-perf.sh, which builds release"]
fn ten_windows_cost_bounded_memory_and_give_it_back_when_they_close() {
    let fixture = Fixture::new("perf-windows");
    let documents: Vec<_> = (0..MANY_WINDOWS)
        .map(|nth| fixture.write(&format!("notes-{nth}.md"), &corpus::typical()))
        .collect();

    let app = launch(&documents[0]);
    let one_window = app.footprint();
    budget::memory(
        "one window on a typical document",
        budgets::ONE_WINDOW,
        one_window.bytes,
    );

    for document in &documents[1..] {
        app.open(document);
    }
    app.wait_until_windows(MANY_WINDOWS);
    for nth in 0..MANY_WINDOWS {
        app.select_window(nth);
        assert!(
            app.showing_document(),
            "window {nth} of {MANY_WINDOWS} is not showing its document",
        );
    }
    let many = app.footprint();
    let per_window = (many.bytes - one_window.bytes) / (MANY_WINDOWS as u64 - 1);
    budget::memory(
        "each further window, over ten of them",
        budgets::EACH_FURTHER_WINDOW,
        per_window,
    );

    for _ in 1..MANY_WINDOWS {
        app.select_window(app.window_count() - 1);
        app.close_window();
    }
    app.wait_until_windows(1);
    // Closing a window frees what it owned, and its web process is most of what it
    // owned. That process leaves on its own time, so this waits for it rather than
    // reading memory that is about to be given back.
    app.wait_for("every closed window's process to go", || {
        app.footprint().processes <= one_window.processes
    });
    budget::memory(
        "what is left after nine of ten windows close",
        budgets::AFTER_CLOSING_NINE,
        app.footprint().bytes,
    );

    assert_eq!(app.window_count(), 1);
    assert!(app.close().is_empty(), "something outlived the application");
}

/// Ten megabytes through the whole pipeline — parse, render, sanitize — which is what
/// a window's worker does before a page reaches the reader.
///
/// Measured on the pipeline rather than through a window because this is the number
/// RFC-001's block cache would be built to move, and a window would fold WebKit's
/// layout of a 10 MB page into it.
#[test]
#[ignore = "a perf budget; run ./scripts/quality.d/20-perf.sh, which builds release"]
fn a_ten_megabyte_document_is_parsed_and_rendered_within_its_budget() {
    use axiomd_engine::{ComrakEngine, Extensions, MarkdownEngine};

    let what = "parse and render of a 10 MB document";
    if !budget::measuring_the_long_ones(what) {
        return;
    }
    let source = ten_megabytes();
    let plugins = axiomd_render::Plugins::builtin(&[]);

    budget::time(what, budgets::TEN_MEGABYTE_RENDER, || {
        let began = Instant::now();
        let parsed = ComrakEngine::new().parse(&source, Extensions::FULL);
        let rendered =
            axiomd_render::render(&parsed, "corpus", &plugins, &axiomd_render::Folder::empty());
        let took = began.elapsed();
        assert!(
            rendered.anchors().len() > 10_000,
            "only {} blocks came out of a 10 MB document",
            rendered.anchors().len(),
        );
        took
    });
}

/// The reload budget for ten megabytes: the document RFC-001's block cache exists for,
/// and the one whose re-render must still be a patch rather than a reload.
#[test]
#[ignore = "a perf budget; run ./scripts/quality.d/20-perf.sh, which builds release"]
fn a_changed_ten_megabyte_file_reaches_the_reader_within_the_reload_budget() {
    let what = "a changed 10 MB file on screen";
    if !budget::measuring_the_long_ones(what) {
        return;
    }
    let fixture = Fixture::new("perf-heavy-reload");
    let mut document = ten_megabytes();
    let path = fixture.write("huge.md", &document);
    let app = launch(&path);
    let loads = app.navigation_count();

    let mut change = 0;
    budget::time(what, budgets::HEAVY_RELOAD, || {
        change += 1;
        let marker = format!("Revision {change} of this document");
        document.push_str(&format!("\n{marker}\n"));
        let began = Instant::now();
        std::fs::write(&path, &document).expect("write the changed document");
        app.wait_until(&format!("document.body.textContent.includes({marker:?})"));
        began.elapsed()
    });

    assert_eq!(
        app.navigation_count(),
        loads,
        "a 10 MB re-render reloaded the page instead of patching it",
    );
}

/// Typing in a ten megabyte document: a key press costs a key press.
///
/// Two budgets in one launch because they are two halves of invariant 4. The first is
/// what a key press itself costs — it must reach the buffer without waiting for a
/// parse. The second is the worst the application answers *anything* while the render
/// that key press started is running: the parse is on a worker (`document.rs`), so the
/// main loop should keep turning, and this is the number that says whether it does.
#[test]
#[ignore = "a perf budget; run ./scripts/quality.d/20-perf.sh, which builds release"]
fn typing_in_a_ten_megabyte_document_never_waits_for_it_to_render() {
    let what = "a key press in a 10 MB document";
    if !budget::measuring_the_long_ones(what) {
        return;
    }
    let fixture = Fixture::new("perf-typing");
    let path = fixture.write("huge.md", &ten_megabytes());
    let app = launch(&path);
    app.activate("win.mode");
    app.wait_until_mode("edit");

    budget::time(what, budgets::KEYSTROKE, || {
        let began = Instant::now();
        app.type_text("x");
        began.elapsed()
    });

    let mut edit = 0;
    budget::time(
        "the worst answer while a 10 MB edit reaches the screen",
        budgets::STALL_WHILE_RENDERING,
        || {
            // One edit, typed once, and then nothing else touches the buffer: the
            // debounce runs out, the document is parsed and rendered on a worker, and
            // the page it produced is patched in. The sample runs until the words the
            // reader just typed are on screen, because the patch is on the main thread
            // and stopping at the end of the render would measure the half of the
            // cycle that is not.
            edit += 1;
            app.place_caret(1);
            app.type_text(&format!("Edit number {edit} of this document\n\n"));

            // Then the application is asked the cheapest thing it answers, over and
            // over, for longer than the whole cycle takes. The longest of those
            // answers is the longest the main loop was not turning — which is the
            // whole of what invariant 4 promises.
            //
            // A fixed window rather than a wait for the edit to appear, which is what
            // every other test here would do: asking the page what it is showing is
            // itself an answer the application cannot give while it is stalled, and a
            // loop that asks takes the harness past its own 30-second deadline before
            // the edit lands. What is measured has to be something the stall cannot
            // swallow, so the sample is bounded by the clock instead. `window count`
            // is answered on the main loop and touches nothing else, which is exactly
            // the property being measured.
            let until = Instant::now() + A_WHOLE_HEAVY_RE_RENDER;
            let mut worst = Duration::ZERO;
            while Instant::now() < until {
                let asked = Instant::now();
                app.window_count();
                worst = worst.max(asked.elapsed());
            }
            worst
        },
    );
}
