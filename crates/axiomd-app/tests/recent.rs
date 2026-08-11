//! Issue #52: what the reader opens in axiomd is in their desktop's recent documents.
//!
//! Every test here drives the shipped binary on a headless compositor and then reads
//! the recent list of *that launch's* desktop — the `recently-used.xbel` GTK writes
//! under the data directory the harness pinned for it. So what is asserted is what the
//! reader would find in Files → Recent and in the file chooser's Recent view, and not
//! that some call was made.
//!
//! The desktop is the launch's own and dies with it (issue #44): a suite that wrote
//! into the developer's real recent list would fill it with fixtures that no longer
//! exist. `containment.rs` holds every launch to that, from the application's own
//! answer about where it writes.

use axiomd_e2e::{App, Fixture, RecentDocument};

/// The name axiomd registers itself under — the application id, which is also the base
/// name of its desktop entry, so a desktop reading the entry back knows what opened the
/// document.
const APP_ID: &str = "io.github.etf.axiomd";

/// What a Markdown document is, told to the desktop so that its icon and its Open With
/// are the ones a Markdown document gets.
const MARKDOWN: &str = "text/markdown";

/// Fails unless `entry` is a document axiomd registered as Markdown.
fn is_axiomds_markdown(entry: &RecentDocument) {
    assert_eq!(
        entry.kind,
        MARKDOWN,
        "the desktop was told {} is something other than a Markdown document",
        entry.document.display(),
    );
    assert!(
        entry.applications.iter().any(|name| name == APP_ID),
        "no application registered {} — Open With has nothing to offer for it: {:?}",
        entry.document.display(),
        entry.applications,
    );
}

/// The documents a launch's desktop has in its recent list, as paths, in a settled
/// order — the order the file keeps them in is the desktop's business, and what is
/// asserted here is which documents are in it.
fn listed(app: &App) -> Vec<String> {
    let mut documents: Vec<String> = app
        .recent_documents()
        .iter()
        .map(|entry| entry.document.display().to_string())
        .collect();
    documents.sort();
    documents
}

/// The whole of the feature as the reader meets it: they open a document in axiomd, and
/// it is in the desktop's recent documents — under the path they keep it at, as a
/// Markdown document, with axiomd registered against it.
#[test]
fn a_document_the_reader_opens_is_in_the_desktops_recent_documents() {
    let fixture = Fixture::new("recent-open");
    let notes = fixture.write("notes.md", "# Notes\n\nA paragraph.\n");

    let app = axiomd_e2e::launch(&notes);

    let entry = app.wait_until_recent(&notes);
    is_axiomds_markdown(&entry);
    assert_eq!(
        listed(&app),
        [notes.display().to_string()],
        "the desktop's recent documents are not the one document that was opened",
    );
}

/// Following a link is opening a document, and counts as one: a reader who arrived at a
/// note through a `[[wikilink]]` — or through an ordinary relative link — finds it in
/// their recent documents beside the one they came from.
#[test]
fn documents_reached_by_following_links_are_in_the_list_too() {
    let fixture = Fixture::new("recent-links");
    let notes = fixture.write(
        "notes.md",
        "# Notes\n\nSee [[guide]], and the [appendix](appendix.md).\n",
    );
    let guide = fixture.write("guide.md", "# Guide\n\nThe document a wikilink led to.\n");
    let appendix = fixture.write(
        "appendix.md",
        "# Appendix\n\nThe document a relative link led to.\n",
    );

    let app = axiomd_e2e::launch(&notes);
    app.wait_until_recent(&notes);

    app.click("a.wikilink[href=\"guide.md\"]");
    app.wait_until("document.querySelector('h1').textContent === 'Guide'");
    is_axiomds_markdown(&app.wait_until_recent(&guide));

    app.activate("win.back");
    app.wait_until("document.querySelector('h1').textContent === 'Notes'");
    app.click("a[href=\"appendix.md\"]");
    app.wait_until("document.querySelector('h1').textContent === 'Appendix'");
    is_axiomds_markdown(&app.wait_until_recent(&appendix));

    assert_eq!(
        listed(&app).len(),
        3,
        "the three documents the reader read are not the three in the list: {:?}",
        listed(&app),
    );
}

/// A document nobody has named is nowhere: an untitled page is not a document the
/// desktop can offer to open again, and it joins the recent list at the moment the
/// reader gives it a name and not before.
#[test]
fn an_untitled_document_joins_the_list_when_it_is_first_given_a_name() {
    let fixture = Fixture::new("recent-untitled");
    // A name with a space in it, which is percent-encoded in the URI the desktop is
    // given: a document the reader could not find again would look exactly like one
    // they could.
    let saved = fixture.write("first draft.md", "");

    let app = axiomd_e2e::launch_without_document();
    app.type_text("# First Draft\n\nThe opening words.\n");
    assert_eq!(
        listed(&app),
        Vec::<String>::new(),
        "an unnamed document is in the desktop's recent documents, where nothing can \
         open it",
    );

    app.save_as(&saved);
    app.wait_until_saved();

    is_axiomds_markdown(&app.wait_until_recent(&saved));
    assert_eq!(
        listed(&app),
        [saved.display().to_string()],
        "naming the document put something other than the document in the list",
    );
}

/// An export is a copy for somebody else, not a document the reader is reading: the PDF
/// they exported is never in their recent documents, however many they make.
#[test]
fn an_exported_file_is_never_in_the_list() {
    let fixture = Fixture::new("recent-export");
    let notes = fixture.write("notes.md", "# Notes\n\nA paragraph to export.\n");
    let guide = fixture.write("guide.md", "# Guide\n\nA second document.\n");
    let exported = notes.with_file_name("notes.pdf");

    let app = axiomd_e2e::launch(&notes);
    app.wait_until_recent(&notes);

    app.export_to(&exported);
    assert_eq!(
        app.wait_for_banner("Exported"),
        "Exported notes.pdf",
        "the window did not say the document was exported",
    );

    // A later open, waited for, so that what is asserted below is a list the desktop
    // has written *since* the export — never one the test read before the export could
    // have reached it.
    app.open(&guide);
    app.wait_until_recent(&guide);

    // The toolkit registers what it wrote a moment after the export reports done and
    // axiomd takes that entry back (`recent.rs`), so "the export is not in the list" is
    // a condition with a deadline like every other wait here. A desktop that was never
    // told about the export at all satisfies it immediately, which is the same
    // requirement met a shorter way.
    app.wait_for(
        "the exported file to be out of the reader's recent documents",
        || !listed(&app).contains(&exported.display().to_string()),
    );

    let mut read = [guide.display().to_string(), notes.display().to_string()];
    read.sort();
    assert_eq!(
        listed(&app),
        read,
        "the reader's recent documents are not the two documents they read: {:?}",
        app.recent_documents(),
    );
}
