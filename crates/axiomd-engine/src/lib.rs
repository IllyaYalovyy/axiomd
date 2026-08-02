//! Markdown engine boundary.
//!
//! Owns the parser-agnostic contract described in `designs/RFC-001-mvp-architecture.md`:
//! a document source goes in, typed events carrying source spans come out. Engine
//! implementations (comrak first, pulldown-cmark second) live behind that boundary and
//! no engine-specific type may appear in a public signature.
//!
//! The boundary trait and the first engine land with issue #2; this crate is a
//! placeholder until then.
