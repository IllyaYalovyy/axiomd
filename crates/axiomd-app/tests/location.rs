//! Issue #24: where the window says the reader's document is.
//!
//! The defect was a packaged axiomd putting `/run/user/1000/doc/d8ded700` under a
//! document the reader keeps in `~/SynologyDrive/AiBlog`. That path is the desktop's
//! document portal talking to the sandbox, and it is not something a reader has ever
//! seen or could navigate to.
//!
//! The resolution itself — the portal's `GetHostPaths`, and every way it can answer —
//! is settled in `axiomd_doc::Home` and probed against the running desktop in
//! `packaging.rs`. What is asserted here is the half a reader actually meets: that the
//! header bar of a running axiomd says where the document is, in the words they know,
//! and keeps saying so as the document moves.

use axiomd_e2e::Fixture;

const NOTES: &str = "# Release Notes\n\nThe first paragraph.\n";

/// A document in the reader's own documents folder reads as `~/Documents` and not as
/// the path it happens to have — the shortening every desktop shows.
#[test]
fn a_document_in_the_readers_home_is_placed_with_their_home_shortened() {
    let app = axiomd_e2e::launch_without_document();

    let document = app.documents_dir().join("notes.md");
    std::fs::write(&document, NOTES).expect("write into the launch's documents folder");
    app.open_here(&document);

    let header = app.header();
    assert_eq!(header.title, "notes.md");
    assert_eq!(
        header.where_it_lives, "~/Documents",
        "the header does not say where the reader keeps the document",
    );
    assert_eq!(
        header.in_full,
        document.display().to_string(),
        "hovering the title does not give the reader the path in full",
    );

    assert!(app.close().is_empty(), "the launch left something running");
}

/// A document outside it is placed in full, because there is nothing to shorten.
#[test]
fn a_document_outside_the_readers_home_is_placed_in_full() {
    let fixture = Fixture::new("placed-outside");
    let document = fixture.write("notes.md", NOTES);
    let app = axiomd_e2e::launch(&document);

    let header = app.header();
    assert_eq!(
        header.where_it_lives,
        document
            .parent()
            .expect("the document has a folder")
            .display()
            .to_string(),
    );
    assert_eq!(header.in_full, document.display().to_string());

    assert!(app.close().is_empty(), "the launch left something running");
}

/// A document that has never been anywhere says so, rather than saying nothing or
/// inventing a folder for it.
#[test]
fn a_document_that_has_never_been_saved_says_it_has_nowhere_to_be() {
    let app = axiomd_e2e::launch_without_document();

    let header = app.header();
    assert_eq!(header.title, "Untitled");
    assert_eq!(header.where_it_lives, "Not saved yet");
    assert_eq!(header.in_full, "Not saved yet");

    assert!(app.close().is_empty(), "the launch left something running");
}

/// And it is placed the moment the reader gives it somewhere to be — the same one
/// answer, retaken, rather than a second one written beside the save.
#[test]
fn a_document_given_a_name_is_placed_where_the_reader_put_it() {
    let fixture = Fixture::new("placed-on-save");
    let app = axiomd_e2e::launch_without_document();

    app.type_text("# Saved\n");
    let chosen = fixture.write("draft.md", "");
    app.save_as(&chosen);
    app.wait_until_saved();

    let header = app.header();
    assert_eq!(header.title, "draft.md");
    assert_eq!(
        header.where_it_lives,
        chosen
            .parent()
            .expect("the document has a folder")
            .display()
            .to_string(),
        "the window still says the document is nowhere",
    );
    assert_eq!(header.in_full, chosen.display().to_string());

    assert!(app.close().is_empty(), "the launch left something running");
}

/// A file that is not there is still a place the reader asked about: the window names
/// it and says where it was looked for, instead of leaving the header blank.
#[test]
fn a_document_that_is_not_there_is_still_named_and_placed() {
    let fixture = Fixture::new("placed-missing");
    // Written and removed rather than never written, so the folder exists and the
    // failure is the file's own.
    let document = fixture.write("gone.md", NOTES);
    std::fs::remove_file(&document).expect("remove the document");

    let app = axiomd_e2e::launch_without_document();
    app.open(&document);

    let header = app.header();
    assert_eq!(header.title, "gone.md");
    assert_eq!(
        header.where_it_lives,
        document
            .parent()
            .expect("the document has a folder")
            .display()
            .to_string(),
    );
    assert_eq!(header.in_full, document.display().to_string());

    assert!(app.close().is_empty(), "the launch left something running");
}

/// Issue #49: the place the window says the document is, is the place the reader is
/// put when they ask for the next one — not wherever the desktop last left a chooser.
#[test]
fn the_chooser_for_the_next_document_opens_where_this_one_is_kept() {
    let fixture = Fixture::new("opening-here");
    let document = fixture.write("notes.md", NOTES);
    let app = axiomd_e2e::launch(&document);

    assert_eq!(
        app.where_the_open_chooser_starts(),
        document
            .parent()
            .expect("the document has a folder")
            .display()
            .to_string(),
        "Ctrl+O does not open where the reader keeps the document they are reading",
    );

    assert!(app.close().is_empty(), "the launch left something running");
}

/// And a document with nowhere to be gives the chooser nowhere to start: it opens on
/// its own default — wherever the reader was last — rather than on a folder invented
/// for a document that has never been anywhere.
#[test]
fn the_chooser_starts_nowhere_in_particular_for_a_document_that_is_nowhere() {
    let app = axiomd_e2e::launch_without_document();

    assert_eq!(
        app.where_the_open_chooser_starts(),
        "",
        "an untitled window sends the reader to a folder it made up",
    );

    assert!(app.close().is_empty(), "the launch left something running");
}
