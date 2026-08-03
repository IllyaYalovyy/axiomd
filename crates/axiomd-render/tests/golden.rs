//! The rendered document, pinned byte for byte.
//!
//! Each `tests/golden/*.md` fixture is rendered and compared with its reviewed
//! `.html` counterpart. Between them the fixtures cover every block and inline
//! construct the engine boundary can emit, so any change to markup, escaping,
//! sanitisation or anchoring shows up here as an exact diff rather than as a
//! surprise in the app.

mod support;

use std::fs;

use support::{fixtures, folder_for, golden_dir, render, render_beside};

#[test]
fn every_fixture_renders_to_its_golden_document() {
    for (name, source) in fixtures() {
        let path = golden_dir().join(name.replace(".md", ".html"));
        let golden = fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("{name}: no golden document at {}", path.display()));
        let rendered = render_beside(&source, &folder_for(&name));
        assert_eq!(
            rendered.html(),
            golden,
            "{name} no longer renders to its golden document{}",
            first_difference(rendered.html(), &golden)
        );
    }
}

/// Rendering is a pure function of the source, and must stay one across processes:
/// a document that renders differently from run to run cannot be golden-tested, and
/// would make live reload patch the DOM for no reason.
///
/// The failure this guards against is hash-map iteration order inside the sanitiser
/// leaking into attribute order, which differs per collection instance — so the
/// document is rendered many times, each with a fresh sanitiser.
#[test]
fn rendering_the_same_source_twenty_times_gives_the_same_document() {
    let source = "- [x] done\n- [ ] not done\n\n<input type=\"text\" value=\"x\" disabled>\n\n\
                  | a | b |\n| - | - |\n| 1 | 2 |\n";
    let first = render(source);
    for attempt in 1..20 {
        let again = render(source);
        assert_eq!(
            again.html(),
            first.html(),
            "render {attempt} differs from the first{}",
            first_difference(again.html(), first.html())
        );
    }
}

/// The first differing line, with its neighbourhood, because whole-document
/// `assert_eq!` output is unreadable.
fn first_difference(actual: &str, expected: &str) -> String {
    let actual: Vec<&str> = actual.lines().collect();
    let expected: Vec<&str> = expected.lines().collect();
    let at = (0..actual.len().max(expected.len()))
        .find(|&i| actual.get(i) != expected.get(i))
        .unwrap_or(0);
    let line = |lines: &[&str], i: usize| {
        lines
            .get(i)
            .copied()
            .unwrap_or("<end of document>")
            .to_string()
    };
    format!(
        "\n  first difference at line {}:\n    expected: {}\n    actual:   {}",
        at + 1,
        line(&expected, at),
        line(&actual, at)
    )
}
