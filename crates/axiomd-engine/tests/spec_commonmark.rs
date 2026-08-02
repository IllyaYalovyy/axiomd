//! CommonMark 0.31.2 conformance.
//!
//! Every example in the vendored `spec.txt` is parsed through the engine boundary
//! and serialised back to HTML; the result must equal the specification's own
//! expected output, byte for byte. There are no exclusions: rendering correctness
//! is the product (VISION principle 1) and the spec is the authority.

mod support;

use axiomd_engine::{ComrakEngine, Extensions, MarkdownEngine};
use support::{HtmlFlavor, describe, load_examples, run_suite};

/// The example count published with CommonMark 0.31.2. Pinned so that swapping the
/// fixture for another revision cannot silently shrink the suite.
const COMMONMARK_EXAMPLES: usize = 652;

#[test]
fn commonmark_0_31_2_suite_passes_completely() {
    let examples = load_examples("commonmark-0.31.2.spec.txt");
    assert_eq!(
        examples.len(),
        COMMONMARK_EXAMPLES,
        "vendored CommonMark fixture changed size"
    );

    let engine = ComrakEngine::new();
    let failures = run_suite(
        &examples,
        |md| engine.parse(md, Extensions::COMMONMARK),
        HtmlFlavor::CommonMark,
    );

    assert!(
        failures.is_empty(),
        "{} of {} CommonMark examples do not match the spec:{}",
        failures.len(),
        examples.len(),
        describe(&failures)
    );
}

/// Strict CommonMark must stay strict: a document using GFM syntax parses as plain
/// markdown when no extensions are requested.
#[test]
fn commonmark_mode_leaves_gfm_syntax_unparsed() {
    let engine = ComrakEngine::new();
    let parsed = engine.parse("~~gone~~\n", Extensions::COMMONMARK);
    let html = support::to_html(parsed.events(), HtmlFlavor::CommonMark);
    assert_eq!(html, "<p>~~gone~~</p>\n");
}
