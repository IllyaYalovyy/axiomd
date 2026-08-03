//! The plugin layer, asserted as a plugin author and a reader see it.
//!
//! Every hook is exercised through the public API — a plugin written here, registered
//! here, and read back out of the document it produced — because that API is the
//! contract #11 and #13 build on. The three things the layer promises beyond "the hook
//! runs" are what most of this file is about: a switched-off plugin costs nothing, a
//! failing one loses only its own block, and neither one can take a source anchor with
//! it.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axiomd_render::{Anchor, Asset, Manifest, PLUGIN_API, Plugin, Plugins};
use support::parse;

/// A document with a claimed fence in it, between two ordinary blocks.
const WITH_A_DIAGRAM: &str = "# Notes\n\nBefore.\n\n```diagram\nA -> B\n```\n\nAfter.\n";

/// Renders `source` with `plugins`, as a window does.
fn render(source: &str, plugins: &Plugins) -> axiomd_render::Rendered {
    axiomd_render::render(&parse(source), "fixture", plugins)
}

/// A registry holding one plugin.
fn registry(plugin: impl Plugin + 'static) -> Plugins {
    Plugins::of([Arc::new(plugin) as Arc<dyn Plugin>])
}

/// The manifest of a plugin that claims `diagram` fences and carries one stylesheet.
const DRAWS: Manifest = Manifest {
    api: PLUGIN_API,
    id: "draws",
    name: "Diagrams",
    description: "Draws diagram fences",
    fences: &["diagram"],
    assets: &[Asset {
        name: "draws.css",
        content_type: "text/css",
        bytes: b".markdown .plugin-draws { border: 1px solid; }",
    }],
};

/// A fence handler that draws, and counts how often it was asked to.
struct Draws {
    asked: Arc<AtomicUsize>,
}

impl Plugin for Draws {
    fn manifest(&self) -> &'static Manifest {
        &DRAWS
    }

    fn fence(&self, language: &str, source: &str) -> Result<String, String> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        Ok(format!(
            "<figure class=\"drawn\">{language}: {}</figure>",
            source.trim()
        ))
    }
}

/// The same claim, answered with a reason instead of a drawing.
struct Fails;

impl Plugin for Fails {
    fn manifest(&self) -> &'static Manifest {
        &DRAWS
    }

    fn fence(&self, _language: &str, _source: &str) -> Result<String, String> {
        Err("the diagram has no nodes".to_owned())
    }
}

/// And the same claim, answered by falling over.
struct Panics;

impl Plugin for Panics {
    fn manifest(&self) -> &'static Manifest {
        &DRAWS
    }

    fn fence(&self, _language: &str, _source: &str) -> Result<String, String> {
        panic!("a plugin bug nobody caught before shipping");
    }
}

/// A fence handler is the only thing between a language and the highlighter: what it
/// draws is what the reader sees, in the block's own place and carrying its line.
#[test]
fn a_fence_handler_draws_the_language_it_claimed() {
    let asked = Arc::new(AtomicUsize::new(0));
    let plugins = registry(Draws {
        asked: asked.clone(),
    });

    let rendered = render(WITH_A_DIAGRAM, &plugins);
    let html = rendered.html();

    assert!(
        html.contains("<figure class=\"drawn\">diagram: A -&gt; B</figure>"),
        "the plugin's drawing is not in the document: {html}",
    );
    assert!(
        !html.contains("class=\"language-diagram\""),
        "the claimed fence was also highlighted as code: {html}",
    );
    assert_eq!(asked.load(Ordering::SeqCst), 1, "the fence was drawn twice");

    // The document around it is untouched, and the drawing stands where the fence
    // was — anchored to the line the fence opened on (invariant 3).
    assert!(html.contains("<p data-line=\"3\">Before.</p>"), "{html}");
    assert!(html.contains("<p data-line=\"9\">After.</p>"), "{html}");
    assert!(
        html.contains("<div class=\"plugin plugin-draws\" data-line=\"5\">"),
        "the drawing does not carry the fence's line: {html}",
    );
    assert_eq!(
        rendered
            .anchors()
            .iter()
            .map(|a| a.line)
            .collect::<Vec<_>>(),
        vec![1, 3, 5, 9],
    );
}

/// The failure the reader is allowed to see: their own block, and a badge saying who
/// could not draw it. Never an empty space, never a dialog, never a lost line.
#[test]
fn a_failing_fence_handler_leaves_the_block_as_source_with_a_badge() {
    let rendered = render(WITH_A_DIAGRAM, &registry(Fails));
    let html = rendered.html();

    assert!(
        html.contains("A -&gt; B"),
        "the reader lost the source of a block a plugin could not draw: {html}",
    );
    assert!(
        html.contains(
            "<p class=\"plugin-badge\">Diagrams could not draw this block: \
             the diagram has no nodes</p>"
        ),
        "the block degraded without saying why: {html}",
    );
    // The rest of the document is exactly what it is without the plugin at all.
    assert!(html.contains("<p data-line=\"3\">Before.</p>"), "{html}");
    assert!(html.contains("<p data-line=\"9\">After.</p>"), "{html}");
    assert!(
        html.contains("<div class=\"plugin-failure\" data-line=\"5\">"),
        "the degraded block lost the fence's line: {html}",
    );
    assert_eq!(
        rendered
            .anchors()
            .iter()
            .map(|a| a.line)
            .collect::<Vec<_>>(),
        vec![1, 3, 5, 9],
    );
    // A plugin that drew nothing needs no styling for what it did not draw.
    assert!(
        !html.contains("draws.css"),
        "a failed plugin still put its stylesheet in the document: {html}",
    );
}

/// The failure the reader is not allowed to see: a plugin falling over takes its own
/// block and nothing else — not the document, not the application (invariant 13).
#[test]
fn a_plugin_that_panics_costs_its_own_block_and_no_more() {
    let rendered = render(WITH_A_DIAGRAM, &registry(Panics));
    let html = rendered.html();

    assert!(html.contains("A -&gt; B"), "{html}");
    assert!(
        html.contains("<p class=\"plugin-badge\">Diagrams could not draw this block:"),
        "a plugin that fell over left no badge: {html}",
    );
    assert!(
        html.contains("<h1 id=\"notes\" data-line=\"1\">Notes</h1>"),
        "{html}"
    );
    assert!(html.contains("<p data-line=\"9\">After.</p>"), "{html}");
}

/// What a switched-off plugin costs: nothing at all. Not a call, not a stylesheet, not
/// a byte of difference in the document.
#[test]
fn a_plugin_that_is_not_registered_is_never_called_and_changes_nothing() {
    let asked = Arc::new(AtomicUsize::new(0));
    let with = registry(Draws {
        asked: asked.clone(),
    });
    let without = Plugins::of([]);

    let switched_on = render(WITH_A_DIAGRAM, &with).html().to_owned();
    asked.store(0, Ordering::SeqCst);
    let switched_off = render(WITH_A_DIAGRAM, &without).html().to_owned();

    assert_eq!(
        asked.load(Ordering::SeqCst),
        0,
        "a plugin the reader switched off was still asked to draw",
    );
    assert!(
        !switched_off.contains("draws.css"),
        "a switched-off plugin's stylesheet reached the document: {switched_off}",
    );
    assert!(
        switched_off
            .contains("<pre class=\"sy-code\" data-line=\"5\"><code class=\"language-diagram\">"),
        "the fence is not the plain code block it is without the plugin: {switched_off}",
    );
    assert_ne!(
        switched_on, switched_off,
        "the plugin was registered and drew nothing",
    );
}

/// The condition on an asset is the document, not only the switch: styling for a
/// capability a document never used is styling it never loads.
#[test]
fn a_plugins_stylesheet_reaches_only_the_documents_that_used_it() {
    let plugins = registry(Draws {
        asked: Arc::new(AtomicUsize::new(0)),
    });

    let used = render(WITH_A_DIAGRAM, &plugins).html().to_owned();
    let unused = render("# Notes\n\nNo diagram here.\n", &plugins)
        .html()
        .to_owned();

    assert!(
        used.contains("<link rel=\"stylesheet\" href=\"axiomd://assets/plugin/draws/draws.css\">"),
        "the document used the plugin and does not carry its styling: {used}",
    );
    assert!(
        !unused.contains("draws.css"),
        "a document with no diagram in it loaded the diagram styling: {unused}",
    );
}

/// The bytes behind that link, which the app serves and nothing else can reach.
#[test]
fn a_built_in_plugins_asset_is_served_by_name_and_nothing_else_is() {
    let asset = Plugins::asset("/plugin/emoji/emoji.css").expect("the emoji stylesheet");

    assert_eq!(asset.content_type, "text/css");
    assert!(
        String::from_utf8_lossy(asset.bytes).contains(".markdown .emoji"),
        "the served bytes are not the plugin's stylesheet",
    );
    assert_eq!(Plugins::asset("/plugin/emoji/other.css"), None);
    assert_eq!(Plugins::asset("/plugin/nobody/emoji.css"), None);
    assert_eq!(Plugins::asset("/plugin/emoji/../../axiomd.css"), None);
    assert_eq!(Plugins::asset("/axiomd.css"), None);
}

/// A plugin is no more trusted than the document: whatever it draws goes through the
/// same sanitiser, so it cannot put in a page what a document cannot.
#[test]
fn what_a_plugin_draws_is_cleaned_like_everything_else() {
    struct Hostile;
    static HOSTILE: Manifest = Manifest {
        fences: &["diagram"],
        assets: &[],
        ..DRAWS
    };
    impl Plugin for Hostile {
        fn manifest(&self) -> &'static Manifest {
            &HOSTILE
        }
        fn fence(&self, _language: &str, _source: &str) -> Result<String, String> {
            Ok(
                "<script>alert(1)</script><img src=\"https://example.com/x.png\">\
                <figure class=\"drawn\">ok</figure>"
                    .to_owned(),
            )
        }
    }

    let html = render(WITH_A_DIAGRAM, &registry(Hostile)).html().to_owned();

    assert!(
        html.contains("<figure class=\"drawn\">ok</figure>"),
        "{html}"
    );
    assert!(
        !html.contains("<script"),
        "a plugin got a script into the page: {html}"
    );
    assert!(
        !html.contains("example.com"),
        "a plugin got a remote image into the page: {html}",
    );
}

/// A post-render hook sees the finished body and the source map beside it, and what it
/// answers with is what the reader gets.
#[test]
fn a_post_render_hook_decorates_the_finished_document() {
    struct Decorates;
    static DECORATES: Manifest = Manifest {
        id: "decorates",
        name: "Decorations",
        fences: &[],
        ..DRAWS
    };
    impl Plugin for Decorates {
        fn manifest(&self) -> &'static Manifest {
            &DECORATES
        }
        fn decorate(&self, html: &str, anchors: &[Anchor]) -> Option<String> {
            Some(format!(
                "{html}<p class=\"counted\">{} blocks</p>",
                anchors.len()
            ))
        }
    }

    let html = render("# Notes\n\nOne.\n\nTwo.\n", &registry(Decorates))
        .html()
        .to_owned();

    assert!(
        html.contains("<p class=\"counted\">3 blocks</p>"),
        "the hook did not see the document and its anchors: {html}",
    );
    assert!(
        html.contains(
            "<link rel=\"stylesheet\" href=\"axiomd://assets/plugin/decorates/draws.css\">"
        ),
        "the plugin decorated the document and its styling stayed behind: {html}",
    );
}

/// The one thing a post-render hook may not do, enforced rather than asked for: the
/// source map outlives the decoration. Outline, scroll sync, search and live reload
/// all read it (invariant 3).
#[test]
fn a_post_render_hook_that_loses_an_anchor_is_refused() {
    struct Loses;
    static LOSES: Manifest = Manifest {
        id: "loses",
        name: "Loses anchors",
        fences: &[],
        assets: &[],
        ..DRAWS
    };
    impl Plugin for Loses {
        fn manifest(&self) -> &'static Manifest {
            &LOSES
        }
        fn decorate(&self, html: &str, _anchors: &[Anchor]) -> Option<String> {
            Some(html.replace(" data-line=\"3\"", ""))
        }
    }

    let source = "# Notes\n\nOne.\n\nTwo.\n";
    let refused = render(source, &registry(Loses)).html().to_owned();

    assert_eq!(
        refused,
        render(source, &Plugins::of([])).html(),
        "a decoration that dropped an anchor was kept",
    );
}

/// A plugin written against another version of this contract is left out, exactly as a
/// switched-off one is: it is not called through a contract that has changed under it.
#[test]
fn a_plugin_from_another_api_version_is_not_run() {
    struct FromTheFuture;
    static FUTURE: Manifest = Manifest {
        api: PLUGIN_API + 1,
        id: "future",
        ..DRAWS
    };
    impl Plugin for FromTheFuture {
        fn manifest(&self) -> &'static Manifest {
            &FUTURE
        }
        fn fence(&self, _language: &str, _source: &str) -> Result<String, String> {
            Ok("<figure class=\"drawn\">from the future</figure>".to_owned())
        }
    }

    let plugins = registry(FromTheFuture);

    assert_eq!(plugins.manifests().count(), 0, "it was registered anyway");
    let html = render(WITH_A_DIAGRAM, &plugins).html().to_owned();
    assert!(!html.contains("from the future"), "{html}");
    assert!(html.contains("<code class=\"language-diagram\">"), "{html}");
}

/// Two plugins cannot be the same plugin: the first registration of an id is the one
/// that answers for it, so a document cannot be drawn twice by the same name.
#[test]
fn an_id_belongs_to_the_first_plugin_that_registered_it() {
    let plugins = Plugins::of([
        Arc::new(Draws {
            asked: Arc::new(AtomicUsize::new(0)),
        }) as Arc<dyn Plugin>,
        Arc::new(Fails) as Arc<dyn Plugin>,
    ]);

    assert_eq!(plugins.manifests().count(), 1);
    let html = render(WITH_A_DIAGRAM, &plugins).html().to_owned();
    assert!(html.contains("<figure class=\"drawn\">"), "{html}");
    assert!(!html.contains("plugin-badge"), "{html}");
}

/// The built-in list is what preferences offers, and every entry of it is a plugin the
/// reader can name in their settings.
#[test]
fn the_built_ins_are_switched_off_by_name() {
    let all: Vec<&Manifest> = Plugins::builtin(&[]).manifests().collect();
    assert!(
        all.iter().any(|manifest| manifest.id == "emoji"),
        "the emoji plugin is not built in",
    );
    for manifest in &all {
        assert_eq!(manifest.api, PLUGIN_API);
        assert!(!manifest.name.is_empty() && !manifest.description.is_empty());
    }

    let off = Plugins::builtin(&["emoji".to_owned()]);
    assert_eq!(off.manifests().count(), all.len() - 1);

    // A plugin this build has never heard of is not an error: an id left in the
    // settings by a version that had it stays there in case it comes back.
    let unknown = Plugins::builtin(&["not-a-plugin".to_owned()]);
    assert_eq!(unknown.manifests().count(), all.len());
}

/// The built-in that proves the transform hook, as the reader sees it: what they wrote
/// as a shortcode is an emoji, and the document says so in one span the styling can
/// find.
#[test]
fn emoji_shortcodes_become_emoji() {
    let plugins = Plugins::builtin(&[]);
    let html = render("Shipped :tada: today, *:rocket: fast*.\n", &plugins)
        .html()
        .to_owned();

    assert!(
        html.contains(
            "<p data-line=\"1\">Shipped <span class=\"emoji\">🎉</span> today, \
             <em><span class=\"emoji\">🚀</span> fast</em>.</p>"
        ),
        "{html}",
    );
    assert!(
        html.contains("<link rel=\"stylesheet\" href=\"axiomd://assets/plugin/emoji/emoji.css\">"),
        "the document used the plugin and does not carry its styling: {html}",
    );
}

/// Code is not prose. A shortcode inside a fence or an inline span is what the author
/// typed, because that is the whole point of writing it there.
#[test]
fn a_shortcode_in_code_is_left_exactly_as_it_was_written() {
    let plugins = Plugins::builtin(&[]);
    let html = render(
        "Use `:tada:` in prose.\n\n```text\nprint(\":tada:\")\n```\n\nAnd :not_an_emoji: stays.\n",
        &plugins,
    )
    .html()
    .to_owned();

    assert!(
        html.contains("<code>:tada:</code>"),
        "an inline code span was rewritten: {html}"
    );
    assert!(
        html.contains("print(\":tada:\")"),
        "a code fence was rewritten: {html}",
    );
    assert!(
        html.contains(":not_an_emoji: stays"),
        "an unknown shortcode did not survive: {html}",
    );
    assert!(!html.contains("class=\"emoji\""), "{html}");
    assert!(
        !html.contains("emoji.css"),
        "a document that used no shortcode loaded the shortcode styling: {html}",
    );
}

/// A shortcode in a heading reaches the outline as the emoji, and the heading keeps
/// the anchor id its source gives it — so a link written against the document survives
/// the plugin being switched on or off.
#[test]
fn a_shortcode_in_a_heading_keeps_the_headings_own_link() {
    let rendered = render("# :rocket: Launch\n\nText.\n", &Plugins::builtin(&[]));

    assert_eq!(rendered.outline().len(), 1);
    assert_eq!(rendered.outline()[0].text, "🚀 Launch");
    assert_eq!(rendered.outline()[0].line, 1);
    assert!(
        rendered.html().contains("<h1 id=\"rocket-launch\""),
        "{}",
        rendered.html(),
    );
    assert_eq!(
        rendered
            .anchors()
            .iter()
            .map(|a| a.line)
            .collect::<Vec<_>>(),
        vec![1, 3],
    );
}

/// Switching the built-in off gives back exactly the document a build with no plugin
/// layer produces — byte for byte, which is the only honest measure of "costs
/// nothing".
#[test]
fn a_document_rendered_with_every_plugin_off_is_the_plain_document() {
    let source = "# :rocket: Launch\n\nShipped :tada: today.\n";
    let off = render(source, &Plugins::builtin(&["emoji".to_owned()]))
        .html()
        .to_owned();
    let none = render(source, &Plugins::of([])).html().to_owned();

    assert_eq!(off, none);
    assert!(off.contains(":tada: today"), "{off}");
    assert!(!off.contains("emoji.css"), "{off}");
}

/// An exported file carries what it needs and names nothing: a plugin's styling
/// travels inside it like the document's own.
#[test]
fn an_exported_document_carries_a_plugins_styling_rather_than_linking_it() {
    let parsed = parse("Shipped :tada: today.\n");
    let exported = axiomd_render::standalone(&parsed, "notes", &Plugins::builtin(&[]), &|_| None);

    assert!(exported.contains("🎉"), "{exported}");
    assert!(
        exported.contains(".markdown .emoji"),
        "the exported file does not carry the plugin's styling: {exported}",
    );
    assert!(
        !exported.contains("axiomd://"),
        "an exported file names something only the app can answer for: {exported}",
    );
}
