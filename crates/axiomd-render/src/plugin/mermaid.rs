//! Mermaid diagrams: a ```` ```mermaid ```` fence read as a picture (issue #13).
//!
//! The plugin that proves the two halves of the API a stylesheet alone could not: a
//! fence handler, and an asset that is *code*. What this module does is small — it
//! turns a fence into an anchored block that holds the diagram's source — because the
//! drawing itself cannot happen here. A diagram's size is a question about text
//! metrics, fonts and a viewport, and none of those exist until the document is on
//! screen. So the block travels with the source in it, and the page draws it
//! ([`mermaid-view.js`](../../assets/plugin/mermaid-view.js)).
//!
//! # What the reader sees while that has not happened yet
//!
//! The source they wrote, as a code block like any other. That is the whole of the
//! not-yet-drawn state, and it is also the failed state and the switched-off state: a
//! reader never gets a blank box, and never loses what the document says (invariant
//! 12). A diagram the page cannot parse keeps the source and gains one line saying
//! why, in the same words a plugin that fails in the pipeline gets.
//!
//! # The library
//!
//! `@mermaid-js/tiny` 11.16.0, MIT, vendored whole beside this file with the licence
//! it is used under (`assets/plugin/mermaid.LICENSE`). It is compiled into the
//! application: no network, ever, and nothing to install (`design_decisions.md`).
//!
//! Its "tiny" build covers the diagram types documents actually use — flowchart,
//! sequence, class, state, entity-relationship, gantt, pie, user journey and
//! gitgraph, all verified drawing against the running application. What it leaves out
//! is the types that carry their own extra bundles: **mindmap**, **quadrant**,
//! **requirement**, **timeline**, **sankey**, **xychart**, **block**, **packet**,
//! **architecture**, **kanban**, **radar**, **treemap** and **zenuml**. A fence in
//! one of those is not a blank box either: it fails the way an unparseable diagram
//! fails, keeping its source and saying that this build does not know the type.

use super::{Asset, Manifest, PLUGIN_API, Plugin, SCRIPT, STYLESHEET};
use crate::body::escape_text;

/// The library that draws, run by the app and never by the document.
const LIBRARY: Asset = Asset {
    name: "mermaid.js",
    content_type: SCRIPT,
    bytes: include_bytes!("../../assets/plugin/mermaid.js"),
};

/// The plugin's own view code: what to draw, when to draw it, and what to do when it
/// cannot be drawn.
///
/// Ahead of [`LIBRARY`] in the manifest, and that order is load-bearing rather than
/// alphabetical. A document is displayed with scripting off, and WebKitGTK runs no
/// timer at all under that setting (probed on 2.52.5, and already relied on by
/// `track.js`) — so this file puts a timer back, built out of animation frames, which
/// do run. The library reads those functions off the global object as it loads, so
/// they have to be there before it does.
const VIEW: Asset = Asset {
    name: "view.js",
    content_type: SCRIPT,
    bytes: include_bytes!("../../assets/plugin/mermaid-view.js"),
};

/// How a diagram sits in the document — the block, and the source inside it while it
/// is still a source.
const STYLE: Asset = Asset {
    name: "mermaid.css",
    content_type: STYLESHEET,
    bytes: include_bytes!("../../assets/plugin/mermaid.css"),
};

const MANIFEST: Manifest = Manifest {
    api: PLUGIN_API,
    id: "mermaid",
    name: "Mermaid Diagrams",
    description: "Draw ```mermaid fences as diagrams, bundled and offline.",
    fences: &["mermaid"],
    assets: &[VIEW, LIBRARY, STYLE],
};

/// The plugin itself, which holds nothing: everything it needs is compiled in.
pub(super) struct Mermaid;

impl Plugin for Mermaid {
    fn manifest(&self) -> &'static Manifest {
        &MANIFEST
    }

    /// The block a diagram fence becomes: its own source, as a code block.
    ///
    /// Deliberately the same markup an unclaimed fence in this language would have
    /// produced. It is what the reader reads until the page has drawn the diagram, it
    /// is what they keep if it cannot be drawn, and it is what the page reads the
    /// diagram *out of* — so the source is in the document exactly once, as text,
    /// rather than repeated into an attribute nobody can see.
    fn fence(&self, language: &str, source: &str) -> Result<String, String> {
        let _ = language;
        Ok(format!(
            "<pre class=\"sy-code\"><code class=\"language-mermaid\">{}</code></pre>\n",
            escape_text(source),
        ))
    }
}
