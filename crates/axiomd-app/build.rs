//! Compiles axiomd's settings schema, so a copy that was built but never installed
//! still has one.
//!
//! An installed axiomd reads its schema from the system's compiled schemas, where
//! packaging puts `data/io.github.etf.axiomd.gschema.xml`. Everything else — `cargo
//! run`, the test suite, the e2e harness driving the binary beside it — runs from the
//! build tree, where nothing has been installed anywhere. So the schema is compiled
//! here as well and the resulting directory is handed to the crate as `AXIOMD_SCHEMAS`
//! for `settings.rs` to fall back to. Nothing about the user's machine is baked in
//! beyond this build's own output directory.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The schema, in the repository's data directory beside the desktop file.
const SCHEMA: &str = "io.github.etf.axiomd.gschema.xml";

fn main() {
    let manifest = PathBuf::from(env("CARGO_MANIFEST_DIR"));
    let source = manifest.join("../../data").join(SCHEMA);
    let schemas = PathBuf::from(env("OUT_DIR")).join("schemas");

    println!("cargo::rerun-if-changed={}", source.display());

    std::fs::create_dir_all(&schemas)
        .unwrap_or_else(|error| panic!("create {}: {error}", schemas.display()));
    std::fs::copy(&source, schemas.join(SCHEMA))
        .unwrap_or_else(|error| panic!("copy {}: {error}", source.display()));
    compile(&schemas);

    println!("cargo::rustc-env=AXIOMD_SCHEMAS={}", schemas.display());
}

/// Turns the schema into the `gschemas.compiled` GLib can read.
///
/// A missing compiler is a hard failure rather than a warning: an axiomd built
/// without a schema starts and then dies the moment it reads a setting, which is a
/// far worse way to find out.
fn compile(schemas: &Path) {
    let compiler = "glib-compile-schemas";
    let compiled = match Command::new(compiler).arg(schemas).status() {
        Ok(status) => status,
        Err(error) => panic!(
            "axiomd needs {compiler} to build its settings schema and could not run it: \
             {error}. Install the GLib development tools (`sudo dnf install glib2-devel`)."
        ),
    };
    assert!(
        compiled.success(),
        "{compiler} rejected {SCHEMA}: {compiled}",
    );
}

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|error| panic!("{name}: {error}"))
}
