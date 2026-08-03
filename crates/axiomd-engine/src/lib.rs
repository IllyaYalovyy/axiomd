//! Markdown engine boundary.
//!
//! Owns the parser-agnostic contract described in `designs/RFC-001-mvp-architecture.md`:
//! a document source goes in, typed events carrying source spans come out. Engine
//! implementations live behind that boundary and no engine-specific type may appear
//! in a public signature — the view layer must never be able to tell which parser
//! produced a document.
//!
//! ```
//! use axiomd_engine::{ComrakEngine, Event, Extensions, MarkdownEngine, Tag};
//!
//! let engine = ComrakEngine::new();
//! let parsed = engine.parse("# Title\n", Extensions::FULL);
//! assert!(matches!(
//!     parsed.events()[0].event,
//!     Event::Start(Tag::Heading { level: 1 })
//! ));
//! // Spans slice straight back into the source: outline, scroll sync, search
//! // and live-reload anchoring all ride on this.
//! assert_eq!(&"# Title\n"[parsed.events()[0].span.range.clone()], "# Title");
//! ```
//!
//! # Shape notes
//!
//! RFC-001 sketches `parse` as returning `Box<dyn DocumentEvents<'a> + 'a>` and a
//! separate `ParseOptions` struct. Both are collapsed here under the deep-modules
//! mandate: [`Parsed`] is a concrete, object-safe return type (every engine
//! materialises an AST anyway, and the renderer needs random access for block-level
//! caching), and [`Extensions`] is the axiomd-owned options type itself rather than a
//! struct that would forward to a single field.

#![deny(missing_docs)]

mod boundary;
mod comrak_engine;
mod obsidian;

pub use boundary::{
    Alignment, Callout, EngineId, Event, Extension, Extensions, MarkdownEngine, Parsed, Span,
    SpannedEvent, Tag, TagEnd, Task,
};
pub use comrak_engine::ComrakEngine;
