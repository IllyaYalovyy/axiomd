//! The same document, read by every engine, through the whole pipeline (issue #17).
//!
//! `axiomd-render` is the first thing past the engine boundary, and it is where a
//! leaked engine would show. It takes `Parsed` and nothing else, so these tests are
//! written the way the boundary says the pipeline works: pick an engine by name, hand
//! its parse to the renderer, and read the page.
//!
//! What they hold to:
//!
//! * every registered engine renders every golden fixture into a real document —
//!   nothing panics, nothing comes out empty, and the anchors outline and scroll sync
//!   ride on are there whichever engine produced the events;
//! * a capability an engine does not have shows up as the *document without that
//!   feature*, not as a failure — `Extension::Autolinks` is the live case;
//! * the pipeline holds no opinion about which engine it was handed.

mod support;

use axiomd_engine::{Extension, Extensions, MarkdownEngine};
use axiomd_render::{Folder, Plugins, Rendered};

/// Every engine this build has.
fn engines() -> &'static [&'static dyn MarkdownEngine] {
    let engines = axiomd_engine::engines();
    assert!(
        engines.len() >= 2,
        "the pipeline is only being proved against one engine (issue #17)",
    );
    engines
}

/// Renders `source` the way the app does, with the engine the reader chose.
fn render_with(engine: &dyn MarkdownEngine, source: &str) -> Rendered {
    let parsed = engine.parse(source, Extensions::FULL);
    axiomd_render::render(&parsed, "fixture", &Plugins::builtin(&[]), &Folder::empty())
}

/// Every golden fixture, through every engine, to a page a reader could read.
///
/// The fixtures are the corpus precisely because they are the documents that exercise
/// every construct the boundary can emit; running them through a second engine is what
/// says the pipeline consumes the boundary rather than one parser's habits.
#[test]
fn every_engine_renders_every_fixture_into_a_document() {
    for engine in engines() {
        for (name, source) in support::fixtures() {
            let page = render_with(*engine, &source);
            let html = page.html();

            assert!(
                html.contains("<article class=\"markdown\""),
                "{}: {name} did not render into a document: {html}",
                engine.id(),
            );
            assert!(
                html.len() > 200,
                "{}: {name} rendered into {} bytes, which is not a document",
                engine.id(),
                html.len(),
            );
            // Anchors are what outline navigation, scroll sync and live reload map
            // through (invariant 3). A page without them is a page the reader cannot
            // be kept in place in.
            assert!(
                html.contains("data-line=\""),
                "{}: {name} rendered without a single source anchor",
                engine.id(),
            );
        }
    }
}

/// Two engines, the same document, the same heading map.
///
/// The outline is the page's own heading map, and picking a section takes the reader
/// to the line it names. If two engines disagreed about either, switching engines
/// would move the reader.
#[test]
fn every_engine_produces_the_same_outline_for_the_same_document() {
    let source =
        "# Title\n\nProse.\n\n## Getting started\n\nMore.\n\n### Details\n\nEnd.\n\n## Later\n";
    let mut agreed: Option<Vec<(u8, String, u32)>> = None;

    for engine in engines() {
        let page = render_with(*engine, source);
        let outline: Vec<(u8, String, u32)> = page
            .outline()
            .iter()
            .map(|heading| (heading.level, heading.text.clone(), heading.line))
            .collect();

        assert_eq!(
            outline.len(),
            4,
            "{}: {outline:?} is not this document's outline",
            engine.id(),
        );
        match &agreed {
            None => agreed = Some(outline),
            Some(first) => assert_eq!(
                &outline,
                first,
                "{} disagrees with {} about where this document's sections are",
                engine.id(),
                engines()[0].id(),
            ),
        }
    }
}

/// A capability an engine does not have is the document without that feature — never a
/// crash, never an empty page, and never a promise the renderer cannot keep.
///
/// pulldown-cmark has no GFM extended autolinks, so this is a live case rather than a
/// hypothetical: the same paragraph is a link in one engine and prose in the other, and
/// both are documents the reader can read. What decides which is the engine's own
/// capability report, not a list kept here.
#[test]
fn a_capability_an_engine_lacks_degrades_to_the_document_without_it() {
    let source = "Visit www.example.com for more.\n";
    let mut linked = 0usize;
    let mut plain = 0usize;

    for engine in engines() {
        let html = render_with(*engine, source).html().to_owned();

        // The prose is there either way. That is what "degraded" means: less markup,
        // never less document.
        assert!(
            html.contains("www.example.com"),
            "{}: the address itself is gone from the page: {html}",
            engine.id(),
        );

        let is_link = html.contains("href=\"http://www.example.com\"");
        assert_eq!(
            is_link,
            engine.capabilities().contains(Extension::Autolinks),
            "{}: the page and the engine's capability report disagree about extended \
             autolinks",
            engine.id(),
        );
        match is_link {
            true => linked += 1,
            false => plain += 1,
        }
    }

    assert!(
        linked > 0 && plain > 0,
        "this build's engines all agree about extended autolinks, so the degradation \
         path is not being exercised ({linked} link it, {plain} do not)",
    );
}

/// The pipeline cannot tell which engine produced a document.
///
/// Not a spelling check on the output: the engine ids are asked of the registry, so an
/// engine renamed or added is covered without this test being touched.
#[test]
fn no_engine_names_itself_in_the_page() {
    let source = "# Title\n\n> [!NOTE] Careful\n> body\n\n| a |\n| - |\n| 1 |\n\n- [x] done\n";
    for engine in engines() {
        let html = render_with(*engine, source).html().to_owned();
        for named in engines() {
            assert!(
                !html.contains(named.id().as_str()),
                "{} put the name {} in the page it rendered",
                engine.id(),
                named.id(),
            );
        }
    }
}
