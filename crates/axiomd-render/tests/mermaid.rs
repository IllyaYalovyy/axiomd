//! The Mermaid plugin, as the pipeline produces it (issue #13).
//!
//! What is asserted here is everything about a diagram that is decided before the page
//! exists: the block a fence becomes, the source it keeps, the files it asks the app
//! for, and — the half that matters most — the files a document without a diagram in
//! it does *not* ask for. Drawing itself happens in the page and is asserted against
//! the running application (`axiomd-app/tests/mermaid.rs`).

mod support;

use axiomd_render::{Plugins, Rendered};
use support::parse;

/// A document with one diagram between two ordinary blocks.
const WITH_A_DIAGRAM: &str =
    "# Notes\n\nBefore.\n\n```mermaid\nflowchart TD\n  A --> B\n```\n\nAfter.\n";

/// The same document with no diagram in it.
const WITHOUT_A_DIAGRAM: &str = "# Notes\n\nBefore.\n\n```rust\nfn main() {}\n```\n\nAfter.\n";

/// Renders as a window does, with the plugins a first run reads under.
fn render(source: &str) -> Rendered {
    axiomd_render::render(&parse(source), "fixture", &Plugins::builtin(&[]))
}

/// The fence becomes an anchored block that still holds the diagram's source, so the
/// reader sees what they wrote until the page draws it — and keeps it if it never can.
#[test]
fn a_mermaid_fence_becomes_a_diagram_block_holding_its_own_source() {
    let rendered = render(WITH_A_DIAGRAM);

    assert!(
        rendered
            .body()
            .contains("<div class=\"plugin plugin-mermaid\" data-line=\"5\">"),
        "no anchored diagram block in {}",
        rendered.body(),
    );
    assert!(
        rendered
            .body()
            .contains("<code class=\"language-mermaid\">flowchart TD\n  A --&gt; B\n</code>"),
        "the diagram's source is not in {}",
        rendered.body(),
    );
    // And the block is where the fence was: the anchor map is what outline navigation,
    // search and live reload all read.
    assert_eq!(
        rendered
            .anchors()
            .iter()
            .map(|anchor| anchor.line)
            .collect::<Vec<_>>(),
        vec![1, 3, 5, 10],
    );
}

/// The library reaches a document that has a diagram in it, and it is named as
/// something the app's own origin answers for — never anything outside it.
#[test]
fn a_document_with_a_diagram_asks_for_the_bundled_library() {
    let rendered = render(WITH_A_DIAGRAM);

    assert_eq!(
        rendered.scripts(),
        [
            "axiomd://assets/plugin/mermaid/view.js",
            "axiomd://assets/plugin/mermaid/mermaid.js",
        ],
    );
    assert_eq!(
        rendered.stylesheets(),
        ["axiomd://assets/plugin/mermaid/mermaid.css"],
    );
    // Everything the plugin carries is compiled in and served from the app's own
    // scheme, which is the whole of what a document can reach.
    for uri in rendered.scripts() {
        let path = uri.strip_prefix("axiomd://assets").expect("an asset URI");
        let asset = Plugins::asset(path).unwrap_or_else(|| panic!("{uri} is not bundled"));
        assert_eq!(asset.content_type, "text/javascript");
        assert!(!asset.bytes.is_empty());
    }
}

/// The half of "lazily" that is decided before the page exists: a document with no
/// diagram in it costs nothing at all — not the 2.5 MB library, not a stylesheet.
#[test]
fn a_document_without_a_diagram_asks_for_no_script_at_all() {
    let rendered = render(WITHOUT_A_DIAGRAM);

    assert_eq!(rendered.scripts(), [] as [String; 0]);
    assert!(
        !rendered.html().contains("mermaid"),
        "a document with no diagram named the diagram plugin",
    );
}

/// A reader who switched the plugin off gets the source they wrote, highlighted like
/// any other fence — and the library never reaches them.
#[test]
fn switching_the_plugin_off_leaves_the_fence_as_an_ordinary_code_block() {
    let plugins = Plugins::builtin(&["mermaid".to_owned()]);
    let rendered = axiomd_render::render(&parse(WITH_A_DIAGRAM), "fixture", &plugins);

    assert_eq!(rendered.scripts(), [] as [String; 0]);
    assert!(
        rendered
            .body()
            .contains("<pre class=\"sy-code\" data-line=\"5\"><code class=\"language-mermaid\">"),
        "the fence is not an ordinary code block in {}",
        rendered.body(),
    );
    assert!(!rendered.html().contains("plugin-mermaid"));
}

/// An exported document leaves axiomd behind, so nothing that only axiomd could run
/// travels in it: the diagram's source is what the file carries.
#[test]
fn an_exported_document_carries_no_script() {
    let exported = axiomd_render::standalone(
        &parse(WITH_A_DIAGRAM),
        "fixture",
        &Plugins::builtin(&[]),
        &|_| None,
    );

    assert!(
        !exported.contains("<script"),
        "an exported document carried a script",
    );
    assert!(
        !exported.contains("mermaid.js"),
        "an exported document named the diagram library",
    );
    assert!(
        exported.contains("flowchart TD"),
        "an exported document lost the diagram's source",
    );
}

/// A bundled library travels with the licence it is used under, and this is the test
/// that says so out loud rather than a note somebody has to remember.
#[test]
fn the_bundled_library_travels_with_the_licence_it_is_used_under() {
    let licence = include_str!("../assets/plugin/mermaid.LICENSE");

    assert!(licence.contains("MIT License"), "{licence}");
    assert!(licence.contains("Knut Sveidqvist"), "{licence}");
}
