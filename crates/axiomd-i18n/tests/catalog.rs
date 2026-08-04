//! What a translator gets, held to what the application actually says.
//!
//! Localisation fails quietly. A screen of words added without a line in
//! `po/POTFILES.in` is not a build error, not a wrong pixel and not a failing
//! assertion anywhere else — it is simply a part of axiomd that no translator can ever
//! see, discovered by a reader in another language months later. So the list is not
//! trusted: `xgettext` is run over *every* file that could carry a word for the reader,
//! and what it finds has to be exactly what the list says.
//!
//! Both directions matter. A file carrying words and missing from the list is a screen
//! that cannot be translated; a file on the list that carries none is a path that has
//! moved or a message that is gone, and a translator's tooling stops at it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The repository this crate lives in.
fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repository this crate is in")
}

/// A scratch directory of this test's own, removed when it is dropped.
struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let path = std::env::temp_dir().join(format!("axiomd-po-{label}-{}", std::process::id()));
        std::fs::create_dir_all(&path).expect("make a scratch directory");
        Scratch { path }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Runs `scripts/generate-pot.sh` and answers the template it wrote.
///
/// The script rather than a second copy of its flags: which calls carry a translatable
/// word and how the source is read are one answer, and a test asking a different
/// `xgettext` a different question would be checking nothing about what translators
/// really get.
fn extracted(files_from: Option<&Path>, into: &Path) -> String {
    let mut generate = Command::new(repository().join("scripts/generate-pot.sh"));
    generate.arg("--output").arg(into);
    if let Some(files_from) = files_from {
        generate.arg("--files-from").arg(files_from);
    }
    let run = generate.output().expect(
        "run scripts/generate-pot.sh \
         (it needs xgettext — Fedora: sudo dnf install gettext, \
          Debian: sudo apt install gettext)",
    );
    assert!(
        run.status.success(),
        "scripts/generate-pot.sh failed:\n{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    std::fs::read_to_string(into).expect("read the template that was just written")
}

/// Every file the template says a message came out of.
fn sources_of(template: &str) -> BTreeSet<String> {
    template
        .lines()
        .filter_map(|line| line.strip_prefix("#: "))
        .flat_map(str::split_whitespace)
        // `path:line` — and no path in this repository has a colon in it.
        .filter_map(|reference| reference.rsplit_once(':'))
        .map(|(file, _)| file.to_owned())
        .collect()
}

/// Every file that could carry a word for the reader: the source of every crate, and
/// the two files the desktop reads about axiomd.
///
/// Deliberately not a list of files to check. A sweep that named the files it expected
/// would pass over exactly the file nobody remembered to add.
fn everything_a_word_could_be_written_in() -> Vec<String> {
    let repository = repository();
    let mut found = Vec::new();
    walk(&repository.join("crates"), &mut found);
    found.retain(|path| {
        // Sources only: a crate's `tests` and `benches` say nothing to a reader, and a
        // build script's words are for whoever is building.
        let relative = path.strip_prefix(&repository).unwrap_or(path);
        let parts: Vec<_> = relative.iter().map(|part| part.to_string_lossy()).collect();
        path.extension().is_some_and(|kind| kind == "rs")
            && parts.get(2).is_some_and(|d| d == "src")
    });
    found.push(repository.join("data/io.github.etf.axiomd.desktop"));
    found.push(repository.join("data/io.github.etf.axiomd.metainfo.xml"));
    found.sort();

    found
        .iter()
        .map(|path| {
            path.strip_prefix(&repository)
                .expect("a path inside the repository")
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

fn walk(directory: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, into);
        } else {
            into.push(path);
        }
    }
}

/// The list `po/POTFILES.in` holds, without its comments and blank lines.
fn listed_for_translators() -> BTreeSet<String> {
    let path = repository().join("po/POTFILES.in");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

/// The drift check: the list translators work from is exactly the set of files that
/// really carry a word for the reader — no more, and above all no less.
#[test]
fn every_file_with_a_word_for_the_reader_is_listed_for_translators() {
    let scratch = Scratch::new("drift");
    let everything = scratch.path.join("everything.in");
    std::fs::write(
        &everything,
        everything_a_word_could_be_written_in().join("\n"),
    )
    .expect("write the list of every file to sweep");

    let swept = sources_of(&extracted(
        Some(&everything),
        &scratch.path.join("swept.pot"),
    ));
    let listed = listed_for_translators();

    let missing: Vec<_> = swept.difference(&listed).collect();
    assert!(
        missing.is_empty(),
        "these files say something to the reader and no translator can see it; \
         add them to po/POTFILES.in: {missing:#?}",
    );

    let stale: Vec<_> = listed.difference(&swept).collect();
    assert!(
        stale.is_empty(),
        "po/POTFILES.in lists these and there is nothing to translate in them; \
         remove them: {stale:#?}",
    );
}

/// The template itself: it is generated from the committed list without complaint, and
/// what comes out is the words a reader really reads — from the window, from a rendered
/// document, and from what the desktop shows about axiomd before it is even started.
#[test]
fn the_template_translators_work_from_holds_the_words_the_reader_reads() {
    let scratch = Scratch::new("template");
    let template = extracted(None, &scratch.path.join("axiomd.pot"));

    for message in [
        // The window.
        "msgid \"Open Document\"",
        "msgid \"No document open\"",
        "msgid \"If you don't save, your changes will be lost.\"",
        // The preferences dialog, and a plugin named in it.
        "msgid \"Preferences\"",
        "msgid \"Mermaid Diagrams\"",
        // A rendered document's own chrome.
        "msgid \"Load image\"",
        // The document model's troubles.
        "msgid \"Could not save {document}\"",
        // The desktop entry and the AppStream metainfo.
        "msgid \"Markdown Viewer\"",
        "msgid \"Read and write Markdown documents\"",
    ] {
        assert!(
            template.contains(message),
            "the template has no {message} in it, so nobody can translate it",
        );
    }

    // A plural is a plural in the template, not two unrelated messages: a language with
    // more than two forms has nowhere to put them otherwise.
    assert!(
        template.contains("msgid \"{n} heading\"\nmsgid_plural \"{n} headings\""),
        "the outline's count did not reach the template as a plural",
    );

    // A person's name is not a word for anyone to translate, and the AppStream metainfo
    // says so with `its:translate="no"` rather than by trusting nobody to try.
    assert!(
        !template.contains("msgid \"Illya Yalovyy\""),
        "the developer's own name is offered to translators as a word to change",
    );

    // The words this crate's own tests translate are its own: it is the machinery, not
    // a screen, and a template carrying its fixtures would be a template of invented
    // words. `asked()` in `src/lib.rs` is what keeps them out.
    assert!(
        !template.contains("crates/axiomd-i18n/src/lib.rs"),
        "the translation machinery has put its own test fixtures in front of translators",
    );
}

/// Every message reaches the template through a call `xgettext` can see.
///
/// `xgettext` 0.25.1 reads a Rust source by scanning rather than by parsing, and it
/// misses a module-qualified call in some positions that it finds in others — probed
/// here: `println!("{}", axiomd_i18n::gettext("…"))` is extracted and
/// `let x = axiomd_i18n::gettext("…");` is not. A word written the second way is
/// translated at runtime and offered to nobody, which is the one failure this whole
/// file exists to prevent, and it would leave no other trace.
///
/// So the calls are imported and made bare, everywhere, and this is what says so. Only
/// the four that carry a message: `axiomd_i18n::setup()` names no words and is written
/// qualified on purpose, once, in `main`.
#[test]
fn no_message_is_written_where_the_extractor_cannot_see_it() {
    let repository = repository();
    let mut hidden = Vec::new();
    for file in everything_a_word_could_be_written_in() {
        let Ok(source) = std::fs::read_to_string(repository.join(&file)) else {
            continue;
        };
        for call in ["gettext(", "ngettext(", "pgettext(", "gettext_noop("] {
            if source.contains(&format!("axiomd_i18n::{call}")) {
                hidden.push(format!("{file}: axiomd_i18n::{call}…)"));
            }
        }
    }

    assert!(
        hidden.is_empty(),
        "these calls are written module-qualified, where xgettext may not see them; \
         import the call and write it bare: {hidden:#?}",
    );
}
