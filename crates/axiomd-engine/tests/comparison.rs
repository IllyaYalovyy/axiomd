//! The engine comparison harness: the evidence the D5 default-engine ruling is made
//! on (issue #17).
//!
//! Every registered engine is run through four things and the results are written into
//! `designs/engine-comparison.md`, which is committed as evidence:
//!
//! * **capabilities** — what each engine says it can parse, which
//!   `boundary.rs::engine_parses_exactly_the_extensions_it_advertises` has already
//!   pinned to what it observably does, so the matrix is a measurement rather than a
//!   claim;
//! * **conformance** — the vendored CommonMark 0.31.2 and GFM extension suites;
//! * **the golden corpus** — every fixture the render pipeline is pinned against,
//!   parsed by each engine and compared with the shipping default's parse;
//! * **span quality** — the four properties `spans.rs` asserts, plus how finely each
//!   engine anchors a document, which is what scroll sync and the outline ride on.
//!
//! Throughput is the fifth, and it is `#[ignore]`d for the same reason the app's perf
//! budgets are: `cargo test` builds debug, where a parser is an order of magnitude off
//! what anybody ships. `scripts/quality.d/30-engines.sh` runs it in release.
//!
//! # Why this file cannot go stale
//!
//! [`the_committed_report_is_what_this_run_measures`] regenerates the deterministic
//! part of the report and compares it with the committed file byte for byte, printing
//! the fresh text when they differ. An engine added to the registry, or a conversion
//! that changes what an engine produces, fails the gate until the evidence is updated.
//!
//! **The report recommends; it does not decide.** Which engine is the default is D5,
//! and D5 is the project owner's (`design_decisions.md`, `AGENTS.md`).

mod support;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use axiomd_engine::{Event, Extension, Extensions, MarkdownEngine, Parsed, Tag, TagEnd};
use support::{HtmlFlavor, load_examples, to_html};

/// Where the committed evidence lives.
const REPORT: &str = "designs/engine-comparison.md";

/// The generated part of it, between these markers.
const BEGIN: &str = "<!-- measured:begin -->";
const END: &str = "<!-- measured:end -->";

fn engines() -> &'static [&'static dyn MarkdownEngine] {
    let engines = axiomd_engine::engines();
    assert!(
        engines.len() >= 2,
        "a comparison of {} engine(s) is not a comparison (issue #17)",
        engines.len(),
    );
    engines
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the workspace root above crates/axiomd-engine")
        .to_path_buf()
}

// ---------------------------------------------------------------------------
// The four deterministic measurements
// ---------------------------------------------------------------------------

/// How many of a suite's examples an engine renders exactly as the specification says.
fn conformance(
    engine: &dyn MarkdownEngine,
    examples: &[support::Example],
    set: Extensions,
) -> usize {
    let flavor = match set == Extensions::GFM {
        true => HtmlFlavor::Gfm,
        false => HtmlFlavor::CommonMark,
    };
    examples
        .iter()
        .filter(|example| {
            let parsed = engine.parse(&example.markdown, set);
            to_html(parsed.events(), flavor) == example.html
        })
        .count()
}

/// Every golden fixture the render pipeline is pinned against, as `(name, markdown)`.
///
/// Read from the render crate, because that is where the corpus lives and duplicating
/// it would give the comparison a second, quietly diverging set of documents.
fn golden_corpus() -> Vec<(String, String)> {
    let dir = repo_root().join("crates/axiomd-render/tests/golden");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("reading the golden corpus at {}: {e}", dir.display()))
        .map(|entry| entry.expect("directory entry").path())
        .filter(|path| path.extension().is_some_and(|e| e == "md"))
        .collect();
    paths.sort();
    assert!(
        paths.len() >= 6,
        "only {} golden fixtures found at {}; the corpus walk is broken",
        paths.len(),
        dir.display(),
    );
    paths
        .into_iter()
        .map(|path| {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .expect("fixture name")
                .to_owned();
            (
                name,
                std::fs::read_to_string(&path).expect("reading a fixture"),
            )
        })
        .collect()
}

/// Every spec document, which is the span corpus.
fn span_corpus() -> Vec<String> {
    let mut docs = Vec::new();
    for file in ["commonmark-0.31.2.spec.txt", "gfm-0.29.spec.txt"] {
        docs.extend(load_examples(file).into_iter().map(|e| e.markdown));
    }
    docs
}

fn is_block(tag: &Tag<'_>) -> bool {
    matches!(
        tag,
        Tag::Paragraph
            | Tag::Heading { .. }
            | Tag::BlockQuote { .. }
            | Tag::CodeBlock { .. }
            | Tag::List { .. }
            | Tag::Item { .. }
            | Tag::FootnoteDefinition { .. }
            | Tag::Table { .. }
            | Tag::TableHead
            | Tag::TableRow
            | Tag::TableCell { .. }
    )
}

fn is_block_end(end: &TagEnd) -> bool {
    matches!(
        end,
        TagEnd::Paragraph
            | TagEnd::Heading(_)
            | TagEnd::BlockQuote
            | TagEnd::CodeBlock
            | TagEnd::List { .. }
            | TagEnd::Item
            | TagEnd::FootnoteDefinition
            | TagEnd::Table
            | TagEnd::TableHead
            | TagEnd::TableRow
            | TagEnd::TableCell
    )
}

fn block_shape(events: &[axiomd_engine::SpannedEvent<'_>]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match &e.event {
            Event::Start(tag) if is_block(tag) => Some(format!("{tag:?}")),
            _ => None,
        })
        .collect()
}

/// What the span corpus says about one engine's spans.
struct Spans {
    events: usize,
    nested: usize,
    nested_inside_parent: usize,
    blocks: usize,
    blocks_that_reparse: usize,
    lines: usize,
    anchored_lines: usize,
}

fn span_quality(engine: &dyn MarkdownEngine, corpus: &[String]) -> Spans {
    let mut measured = Spans {
        events: 0,
        nested: 0,
        nested_inside_parent: 0,
        blocks: 0,
        blocks_that_reparse: 0,
        lines: 0,
        anchored_lines: 0,
    };

    for source in corpus {
        let parsed = engine.parse(source, Extensions::FULL);
        let events = parsed.events();
        measured.events += events.len();

        let mut stack: Vec<std::ops::Range<usize>> = Vec::new();
        let mut anchored: Vec<u32> = Vec::new();
        let mut depth = 0usize;
        let mut top_level: Option<usize> = None;

        for (index, spanned) in events.iter().enumerate() {
            match &spanned.event {
                Event::Start(tag) if is_block(tag) => {
                    anchored.push(spanned.span.line);
                    if let Some(parent) = stack.last() {
                        measured.nested += 1;
                        if parent.start <= spanned.span.range.start
                            && spanned.span.range.end <= parent.end
                        {
                            measured.nested_inside_parent += 1;
                        }
                    }
                    stack.push(spanned.span.range.clone());
                    if depth == 0 {
                        top_level = Some(index);
                    }
                    depth += 1;
                }
                Event::End(end) if is_block_end(end) => {
                    stack.pop();
                    depth = depth.saturating_sub(1);
                    if depth == 0
                        && let Some(start) = top_level.take()
                    {
                        measured.blocks += 1;
                        let slice = &source[events[start].span.range.clone()];
                        let reparsed = engine.parse(slice, Extensions::FULL);
                        if block_shape(reparsed.events()) == block_shape(&events[start..=index]) {
                            measured.blocks_that_reparse += 1;
                        }
                    }
                }
                _ => {}
            }
        }

        anchored.sort_unstable();
        anchored.dedup();
        let non_blank: Vec<u32> = source
            .lines()
            .enumerate()
            .filter(|(_, line)| !line.trim().is_empty())
            .map(|(at, _)| at as u32 + 1)
            .collect();
        measured.lines += non_blank.len();
        measured.anchored_lines += non_blank
            .iter()
            .filter(|line| anchored.binary_search(line).is_ok())
            .count();
    }

    measured
}

// ---------------------------------------------------------------------------
// The report
// ---------------------------------------------------------------------------

fn percent(part: usize, whole: usize) -> String {
    match whole {
        0 => "n/a".to_owned(),
        _ => format!("{:.1}%", part as f64 * 100.0 / whole as f64),
    }
}

fn row(cells: &[String]) -> String {
    format!("| {} |\n", cells.join(" | "))
}

fn header(first: &str) -> String {
    let mut names = vec![first.to_owned()];
    names.extend(engines().iter().map(|engine| engine.id().to_string()));
    let mut out = row(&names);
    out.push_str(&row(&vec!["---".to_owned(); names.len()]));
    out
}

/// Regenerates the deterministic part of the committed report.
fn measured_report() -> String {
    let commonmark = load_examples("commonmark-0.31.2.spec.txt");
    let gfm: Vec<support::Example> = load_examples("gfm-0.29.spec.txt")
        .into_iter()
        .filter(|e| e.section.contains("(extension)"))
        .collect();
    let goldens = golden_corpus();
    let corpus = span_corpus();
    let reference = engines()[0];

    let mut out = String::new();

    out.push_str("### Capabilities\n\nWhat each engine reports it can parse, held to what it\n");
    out.push_str("observably does by `boundary.rs`.\n\n");
    out.push_str(&header("Extension"));
    for extension in Extension::ALL {
        let mut cells = vec![format!("{extension:?}")];
        cells.extend(engines().iter().map(
            |engine| match engine.capabilities().contains(extension) {
                true => "yes".to_owned(),
                false => "no".to_owned(),
            },
        ));
        out.push_str(&row(&cells));
    }

    out.push_str("\n### Conformance\n\nExamples whose serialised HTML is byte-for-byte what the\n");
    out.push_str("specification prints.\n\n");
    out.push_str(&header("Suite"));
    for (name, examples, set) in [
        (
            format!("CommonMark 0.31.2 ({} examples)", commonmark.len()),
            &commonmark,
            Extensions::COMMONMARK,
        ),
        (
            format!("GFM extensions ({} examples)", gfm.len()),
            &gfm,
            Extensions::GFM,
        ),
    ] {
        let mut cells = vec![name];
        cells.extend(engines().iter().map(|engine| {
            let passed = conformance(*engine, examples, set);
            format!(
                "{passed}/{} ({})",
                examples.len(),
                percent(passed, examples.len())
            )
        }));
        out.push_str(&row(&cells));
    }

    let _ = write!(
        out,
        "\n### Golden corpus ({} fixtures)\n\nEvery document the render pipeline is pinned \
         against, parsed by each\nengine and serialised the same way. `{}` is the shipping \
         default, so what\nthe others are compared against is its parse.\n\n",
        goldens.len(),
        reference.id(),
    );
    out.push_str("| Engine | agrees with the default | differs on |\n| --- | --- | --- |\n");
    for engine in engines() {
        let mut differing = Vec::new();
        for (name, source) in &goldens {
            let theirs = to_html(
                engine.parse(source, Extensions::FULL).events(),
                HtmlFlavor::CommonMark,
            );
            let ours = to_html(
                reference.parse(source, Extensions::FULL).events(),
                HtmlFlavor::CommonMark,
            );
            if theirs != ours {
                differing.push(name.clone());
            }
        }
        out.push_str(&row(&[
            engine.id().to_string(),
            format!("{}/{}", goldens.len() - differing.len(), goldens.len()),
            match differing.is_empty() {
                true => "—".to_owned(),
                false => differing.join(", "),
            },
        ]));
    }

    let _ = write!(
        out,
        "\n### Span quality ({} spec documents)\n\nSpans are load-bearing: outline, scroll \
         sync, search and live-reload\nanchoring all map through them (invariant 3). The \
         first three rows are\nproperties `spans.rs` asserts; the last is how finely an \
         engine anchors a\ndocument, which is what scroll-sync granularity is.\n\n",
        corpus.len(),
    );
    out.push_str(&header("Measure"));
    let quality: Vec<Spans> = engines()
        .iter()
        .map(|engine| span_quality(*engine, &corpus))
        .collect();
    let mut cells = vec!["spanned events".to_owned()];
    cells.extend(quality.iter().map(|q| q.events.to_string()));
    out.push_str(&row(&cells));
    let mut cells = vec!["block spans inside their parent".to_owned()];
    cells.extend(
        quality
            .iter()
            .map(|q| percent(q.nested_inside_parent, q.nested)),
    );
    out.push_str(&row(&cells));
    let mut cells = vec!["top-level blocks whose span re-parses to itself".to_owned()];
    cells.extend(
        quality
            .iter()
            .map(|q| percent(q.blocks_that_reparse, q.blocks)),
    );
    out.push_str(&row(&cells));
    let mut cells = vec!["non-blank lines carrying a block anchor".to_owned()];
    cells.extend(quality.iter().map(|q| percent(q.anchored_lines, q.lines)));
    out.push_str(&row(&cells));

    out
}

/// The committed report, and the generated block inside it.
fn committed() -> (PathBuf, String) {
    let path = repo_root().join(REPORT);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "the comparison report is the evidence issue #17 exists to produce and \
             {} could not be read: {e}",
            path.display()
        )
    });
    (path, text)
}

fn generated_block(text: &str, path: &Path) -> String {
    let start = text
        .find(BEGIN)
        .unwrap_or_else(|| panic!("{}: no {BEGIN} marker", path.display()))
        + BEGIN.len();
    let end = text
        .find(END)
        .unwrap_or_else(|| panic!("{}: no {END} marker", path.display()));
    assert!(
        end > start,
        "{}: the markers are out of order",
        path.display()
    );
    text[start..end].trim_matches('\n').to_owned()
}

/// The committed evidence is what this run measures.
///
/// This is what stops the report going stale: a new engine, or a conversion that
/// changes what an engine produces, fails here until the evidence is rewritten. The
/// fresh text is printed, so updating it is reading the failure rather than re-running
/// something by hand.
#[test]
fn the_committed_report_is_what_this_run_measures() {
    let (path, text) = committed();
    let measured = measured_report();
    let measured = measured.trim_matches('\n');
    let committed = generated_block(&text, &path);

    assert_eq!(
        committed,
        measured,
        "\n{} is no longer what the engines measure. The measured evidence is:\n\
         \n{BEGIN}\n\n{measured}\n\n{END}\n",
        path.display(),
    );
}

/// Every registered engine appears in every part of the report, including the parts a
/// machine does not generate.
///
/// Adding an engine without comparison data is exactly the failure this catches: the
/// throughput section and the recommendation are written by a person, and an engine
/// missing from them is an engine the D5 ruling would be made without.
#[test]
fn the_report_covers_every_registered_engine() {
    let (path, text) = committed();
    let (_, after) = text
        .split_once("## Throughput")
        .unwrap_or_else(|| panic!("{}: no throughput section", path.display()));
    let (throughput, _) = after
        .split_once("\n## ")
        .unwrap_or_else(|| panic!("{}: the throughput section never ends", path.display()));
    let (_, recommendation) = text
        .split_once("## Recommendation")
        .unwrap_or_else(|| panic!("{}: no recommendation", path.display()));

    for engine in engines() {
        let id = engine.id().as_str();
        assert!(
            throughput.contains(id),
            "{}: {id} has no measured throughput",
            path.display(),
        );
        assert!(
            recommendation.contains(id),
            "{}: the recommendation does not mention {id}",
            path.display(),
        );
    }
}

// ---------------------------------------------------------------------------
// Throughput
// ---------------------------------------------------------------------------

/// The documents the perf budgets are measured on, and their sizes.
fn throughput_fixtures() -> Vec<(&'static str, String)> {
    vec![
        ("typical (50 KB)", axiomd_e2e::corpus::typical()),
        ("10 MB", axiomd_e2e::corpus::of_size(10 * 1024 * 1024)),
        (
            "pathological (200 deep)",
            axiomd_e2e::corpus::deeply_nested(),
        ),
    ]
}

/// Parse throughput on the perf fixtures, for every engine.
///
/// `#[ignore]`d and run in release by `scripts/quality.d/30-engines.sh`, for the reason
/// the app's perf budgets are: a parser measured in a debug build is an order of
/// magnitude off what anybody ships, and a number that means nothing is worse in a
/// report than no number at all.
///
/// It asserts nothing about the time — a wall clock on a shared machine is not a thing
/// to gate on — only that every engine finishes every fixture with a document in hand.
/// The numbers are printed for the report.
#[test]
#[ignore = "measured in release by scripts/quality.d/30-engines.sh"]
fn measures_parse_throughput_on_the_perf_fixtures() {
    if cfg!(debug_assertions) {
        panic!(
            "a parser measured in a debug build measures the debug build. Run \
             ./scripts/quality.d/30-engines.sh"
        );
    }

    for (name, document) in throughput_fixtures() {
        for engine in engines() {
            // One warm run so the measurement is of parsing rather than of first-touch
            // page faults on a freshly allocated ten megabytes.
            let warm: Parsed<'_> = engine.parse(&document, Extensions::FULL);
            assert!(!warm.events().is_empty());

            let started = Instant::now();
            let parsed = engine.parse(&document, Extensions::FULL);
            let took = started.elapsed();
            let events = parsed.events().len();

            let megabytes = document.len() as f64 / (1024.0 * 1024.0);
            println!(
                "engines: {:<24} {:<16} {:>9.1} ms  ({:>6.1} MB/s, {events} events)",
                name,
                engine.id().to_string(),
                took.as_secs_f64() * 1000.0,
                megabytes / took.as_secs_f64(),
            );
        }
    }
}
