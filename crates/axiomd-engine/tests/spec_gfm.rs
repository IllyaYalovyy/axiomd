//! GFM extension conformance.
//!
//! # Which examples this suite runs, and why
//!
//! The GFM specification is a verbatim copy of the CommonMark 0.29 spec with five
//! extension sections spliced in. This suite runs **exactly** the examples in those
//! extension sections — the ones GFM actually defines. The other 648 are CommonMark
//! 0.29 cases, superseded by `spec_commonmark.rs`, which runs all 652 examples of
//! CommonMark 0.31.2 with no exclusions.
//!
//! Running them here would be worse than redundant, because GFM 0.29 and CommonMark
//! 0.31.2 disagree, and the GFM document even disagrees with itself:
//!
//! * The "HTML blocks" section (examples 140, 141, 142, 145, 147) expects `<script>`
//!   and `<style>` blocks to pass through untouched, while GFM's own "Disallowed Raw
//!   HTML (extension)" section requires exactly those tags to be escaped.
//! * The "Autolinks" section (examples 610, 616, 619, 620) expects bare URLs to stay
//!   literal text, while "Autolinks (extension)" requires them to become links.
//! * Nine "Emphasis and strong emphasis" cases expect GFM's nested-`<strong>`
//!   collapsing quirk, which CommonMark 0.31.2 does not.
//!
//! Where the two specs disagree, CommonMark 0.31.2 wins: it is the newer spec, and
//! it is the one VISION pins the product to.

mod support;

use axiomd_engine::{ComrakEngine, Extensions, MarkdownEngine};
use support::{Example, HtmlFlavor, describe, load_examples, run_suite};

/// The number of examples inside the GFM spec's `(extension)` sections. Pinned so a
/// fixture update that drops cases fails loudly instead of passing a smaller suite.
const GFM_EXTENSION_EXAMPLES: usize = 24;

/// The five extension sections the GFM spec defines, by heading text.
const EXTENSION_SECTIONS: [&str; 5] = [
    "Tables (extension)",
    "Task list items (extension)",
    "Strikethrough (extension)",
    "Autolinks (extension)",
    "Disallowed Raw HTML (extension)",
];

fn extension_examples() -> Vec<Example> {
    load_examples("gfm-0.29.spec.txt")
        .into_iter()
        .filter(|e| e.section.contains("(extension)"))
        .collect()
}

#[test]
fn gfm_extension_suite_passes_completely() {
    let examples = extension_examples();
    assert_eq!(
        examples.len(),
        GFM_EXTENSION_EXAMPLES,
        "vendored GFM fixture changed size"
    );

    let engine = ComrakEngine::new();
    let failures = run_suite(
        &examples,
        |md| engine.parse(md, Extensions::GFM),
        HtmlFlavor::Gfm,
    );

    assert!(
        failures.is_empty(),
        "{} of {} GFM extension examples do not match the spec:{}",
        failures.len(),
        examples.len(),
        describe(&failures)
    );
}

/// Guards the filter above: every extension section the spec defines must actually
/// contribute examples, so a renamed or removed section cannot silently reduce the
/// suite to a subset that still passes.
#[test]
fn every_gfm_extension_section_contributes_examples() {
    let examples = extension_examples();
    for section in EXTENSION_SECTIONS {
        assert!(
            examples.iter().any(|e| e.section == section),
            "no examples found for GFM section {section:?}"
        );
    }
    let unexpected: Vec<_> = examples
        .iter()
        .filter(|e| !EXTENSION_SECTIONS.contains(&e.section.as_str()))
        .map(|e| e.section.clone())
        .collect();
    assert!(
        unexpected.is_empty(),
        "unrecognised GFM extension sections: {unexpected:?}"
    );
}
