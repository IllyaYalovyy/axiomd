//! Which engines this build has, and which one a name means.
//!
//! Engine selection happens on three levels — the reader's preference, a per-document
//! override, and the `--engine` flag a test launches with (issue #17) — and all three
//! carry an [`EngineId`] rather than an engine. This is the one place that turns a
//! name back into something that parses, so no caller has to know what engines exist
//! in order to offer them or to honour a choice.

use crate::boundary::MarkdownEngine;
use crate::comrak_engine::ComrakEngine;
use crate::pulldown_engine::PulldownEngine;

const COMRAK: ComrakEngine = ComrakEngine::new();
const PULLDOWN: PulldownEngine = PulldownEngine::new();

/// Every engine this build has, in the order a chooser should offer them.
///
/// The first is the engine a document is read with when nothing has chosen otherwise;
/// which one that is remains comrak until the owner rules on D5
/// (`design_decisions.md`). The list is never empty.
pub fn engines() -> &'static [&'static dyn MarkdownEngine] {
    &[&COMRAK, &PULLDOWN]
}

/// The engine `name` names, or `None` when this build has no such engine.
///
/// A stored preference or a command line may name an engine that has been renamed or
/// removed; answering `None` lets the caller say so rather than silently reading the
/// document with something the reader did not choose.
pub fn engine(name: &str) -> Option<&'static dyn MarkdownEngine> {
    engines()
        .iter()
        .copied()
        .find(|engine| engine.id().as_str() == name)
}
