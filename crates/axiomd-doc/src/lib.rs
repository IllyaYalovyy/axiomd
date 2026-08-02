//! Editable document model.
//!
//! While a window owns a file the buffer here is the source of truth: rendering,
//! outline, search and export consume this model, never the file on disk.
//!
//! The model lands with the editor work; this crate is a placeholder until then.
