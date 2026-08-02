//! Rendering pipeline.
//!
//! Turns engine events into sanitized HTML with `data-line` anchors derived from
//! source spans, and hosts the optional plugin layer (math, diagrams). Core
//! rendering — CommonMark/GFM including tables and images — is never a plugin.
//!
//! The pipeline lands with issue #3; this crate is a placeholder until then.
