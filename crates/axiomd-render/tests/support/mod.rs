//! Shared fixture plumbing for the render tests.
//!
//! Each test binary uses the part of this it needs, so not every helper is live in
//! every one of them.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use axiomd_engine::{ComrakEngine, Extensions, MarkdownEngine, Parsed};
use axiomd_render::{Folder, Plugins, Rendered};

/// Every golden fixture, as `(name, markdown)`, in a stable order.
pub fn fixtures() -> Vec<(String, String)> {
    let mut paths: Vec<PathBuf> = fs::read_dir(golden_dir())
        .expect("reading the golden fixture directory")
        .map(|entry| entry.expect("directory entry").path())
        .filter(|path| path.extension().is_some_and(|e| e == "md"))
        .collect();
    paths.sort();
    assert!(
        paths.len() >= 6,
        "only {} fixtures found; the fixture walk is broken",
        paths.len()
    );
    paths
        .into_iter()
        .map(|path| {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .expect("fixture name")
                .to_string();
            let source = fs::read_to_string(&path).expect("reading a fixture");
            (name, source)
        })
        .collect()
}

/// The directory holding `*.md` fixtures and their pinned `*.html`.
pub fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

/// Parses with every extension the boundary knows, as the app does.
pub fn parse(source: &str) -> Parsed<'_> {
    ComrakEngine::new().parse(source, Extensions::FULL)
}

/// Parses and renders in one step, with the plugins a first run reads under — which is
/// what the app hands the pipeline, and with nothing beside the document.
pub fn render(source: &str) -> Rendered {
    render_beside(source, &Folder::empty())
}

/// The same, for a document that has documents beside it — which is the whole of what
/// a `[[wikilink]]` in it can resolve to.
pub fn render_beside(source: &str, beside: &Folder) -> Rendered {
    axiomd_render::render(&parse(source), "fixture", &Plugins::builtin(&[]), beside)
}

/// What lies beside the fixture called `name`: the documents listed in its `.folder`
/// file, or nothing when it has none.
///
/// A fixture's folder is written down beside it rather than built from a scratch
/// directory, because resolution is a rule about a list of names and the pipeline never
/// opens anything (`wikilink.rs`).
pub fn folder_for(name: &str) -> Folder {
    let listing = golden_dir().join(name.replace(".md", ".folder"));
    match fs::read_to_string(listing) {
        Ok(listed) => Folder::holding(
            listed
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned),
        ),
        Err(_) => Folder::empty(),
    }
}
