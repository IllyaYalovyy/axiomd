//! The reader's recent documents: what the desktop is told (issue #52), and what
//! axiomd offers them back (issue #54).
//!
//! Every test here drives the shipped binary on a headless compositor and then reads
//! the recent list of *that launch's* desktop — the `recently-used.xbel` GTK writes
//! under the data directory the harness pinned for it. So what is asserted is what the
//! reader would find in Files → Recent and in the file chooser's Recent view, and not
//! that some call was made.
//!
//! The tests for the in-app list read the primary menu of the running window, opened
//! the way the reader opens it, and press what they find there. One list stands behind
//! both halves: the entries offered in the menu are the entries the desktop was told
//! about, and nothing else is written down anywhere (issue #54).
//!
//! The desktop is the launch's own and dies with it (issue #44): a suite that wrote
//! into the developer's real recent list would fill it with fixtures that no longer
//! exist. `containment.rs` holds every launch to that, from the application's own
//! answer about where it writes.

use std::path::Path;

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

// ---------------------------------------------------------------------------------
// Issue #54: the same list, offered back to the reader inside axiomd.
// ---------------------------------------------------------------------------------

/// What the menu items of recent documents fire. Spelled out here rather than read from
/// the application, so that an action renamed underneath the menu fails a test instead
/// of quietly matching nothing.
const OPEN_RECENT: &str = "app.open-recent";

/// Opens the primary menu of the window the commands are addressing, the way a reader
/// with no pointer opens it.
///
/// Once per window: the list is worked out as the menu opens, and this harness cannot
/// close a menu the way a reader does — so a test that needs a second look at a
/// freshly-built list opens a second window and looks at that one's menu.
fn open_the_menu(app: &App) {
    assert!(app.press_key("F10"), "F10 is on nothing in this window");
    app.wait_for("the main menu to open", || app.menu().open);
}

/// The documents the open menu offers a way back to, in the order it offers them, by
/// the words the reader reads on each.
fn offered(app: &App) -> Vec<String> {
    app.menu()
        .items
        .into_iter()
        .filter(|(_, action)| action.starts_with(OPEN_RECENT))
        .map(|(label, _)| label)
        .collect()
}

/// What pressing the entry labelled `label` would fire, target and all — the whole of
/// what the reader's press does. Fails when the menu is offering no such document,
/// because a test that pressed nothing would pass without pressing anything.
fn the_way_back_to(app: &App, label: &str) -> String {
    app.menu()
        .items
        .into_iter()
        .find(|(shown, action)| shown == label && action.starts_with(OPEN_RECENT))
        .unwrap_or_else(|| {
            panic!(
                "the menu offers no way back to {label:?}, only {:?}",
                offered(app),
            )
        })
        .1
}

/// Reads `document` in the window the reader is in, and waits until the desktop has
/// been told — which is what settles the order the menu lists them in.
fn read(app: &App, document: &Path) {
    app.open_here(document);
    app.wait_until_recent(document);
}

/// The whole of the feature: the documents the reader has been reading are two presses
/// away in any window, newest first.
#[test]
fn the_documents_the_reader_has_read_are_offered_newest_first() {
    let fixture = Fixture::new("recent-menu-order");
    let first = fixture.write("first.md", "# First\n");
    let second = fixture.write("second.md", "# Second\n");
    let third = fixture.write("third.md", "# Third\n");

    let app = axiomd_e2e::launch(&first);
    app.wait_until_recent(&first);
    read(&app, &second);
    read(&app, &third);

    open_the_menu(&app);
    assert!(
        app.menu().offers().contains(&("Open Recent", "")),
        "the menu offers no way back to a document the reader has read: {:?}",
        app.menu().offers(),
    );
    assert_eq!(
        offered(&app),
        ["third.md", "second.md", "first.md"],
        "the reader's documents are not offered with the one they last read at the top",
    );

    // The rest of the menu is the menu it always was. The zoom row is a widget the
    // popover holds rather than an item of the model, so a section rebuilt underneath it
    // as the menu opens is exactly what could leave a gap where it sits.
    assert_eq!(
        app.menu().zoom,
        "100%",
        "working the recent documents out took the zoom row out of the menu",
    );

    // The list is the desktop's own, in the world this launch lives in (issue #44): what
    // it offers is these three fixtures and nothing of whoever is running the suite.
    assert_eq!(
        app.recent_documents().len(),
        3,
        "this launch's desktop knows documents nobody in this test opened: {:?}",
        app.recent_documents(),
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The list is a way back to what the reader was just reading, not an archive: past ten
/// documents the oldest drops off the end.
#[test]
fn the_reader_is_offered_the_last_ten_documents_and_no_more() {
    let fixture = Fixture::new("recent-menu-cap");
    let documents: Vec<_> = (0..11)
        .map(|at| {
            fixture.write(
                &format!("document-{at:02}.md"),
                &format!("# Document {at}\n"),
            )
        })
        .collect();

    let app = axiomd_e2e::launch(&documents[0]);
    app.wait_until_recent(&documents[0]);
    for document in &documents[1..] {
        read(&app, document);
    }

    open_the_menu(&app);
    let offered = offered(&app);
    assert_eq!(
        offered,
        (1..11)
            .rev()
            .map(|at| format!("document-{at:02}.md"))
            .collect::<Vec<_>>(),
        "the menu is not the last ten documents, newest first",
    );
    assert!(
        !offered.contains(&"document-00.md".to_owned()),
        "the eleventh document back is still being offered",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A document that has been deleted is not offered: an entry that cannot open anything
/// is worse than no entry at all.
#[test]
fn a_document_that_is_no_longer_there_is_not_offered() {
    let fixture = Fixture::new("recent-menu-gone");
    let kept = fixture.write("kept.md", "# Kept\n");
    let deleted = fixture.write("deleted.md", "# Deleted\n");
    let reading = fixture.write("reading.md", "# Reading\n");

    let app = axiomd_e2e::launch(&kept);
    app.wait_until_recent(&kept);
    read(&app, &deleted);
    read(&app, &reading);

    std::fs::remove_file(&deleted).expect("delete a document");

    open_the_menu(&app);
    assert_eq!(
        offered(&app),
        ["reading.md", "kept.md"],
        "the menu is offering a document that is not on the disk any more",
    );
    // The desktop's own list still has it — ageing that list is the desktop's business
    // and not axiomd's (issue #52). What changed is what axiomd offers.
    assert_eq!(
        app.recent_documents().len(),
        3,
        "axiomd pruned the desktop's list, which is not its to prune",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Pressing one opens the document, where the reader is, with nothing asked of them —
/// the same path the file chooser's answer takes (invariant 12).
#[test]
fn pressing_a_recent_document_opens_it_where_the_reader_is() {
    let fixture = Fixture::new("recent-menu-open");
    let guide = fixture.write("guide.md", "# Guide\n\nThe document to come back to.\n");
    let notes = fixture.write("notes.md", "# Notes\n");

    let app = axiomd_e2e::launch(&guide);
    app.wait_until_recent(&guide);
    read(&app, &notes);
    assert_eq!(app.dom_text("h1"), "Notes");

    open_the_menu(&app);
    app.activate(&the_way_back_to(&app, "guide.md"));

    app.wait_until("document.querySelector('h1').textContent === 'Guide'");
    assert_eq!(
        app.window_title(),
        "guide.md",
        "the window the reader was in is not the window the document opened in",
    );
    assert_eq!(app.window_count(), 1, "the document opened in a new window");
    assert_eq!(
        app.visible_dialog(),
        "",
        "opening a recent document asked the reader a question",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// And one that goes away between the menu opening and the reader pressing it — the
/// only way a listed document can be missing — is handled exactly as any other document
/// that cannot be opened is (`window.rs`): the reader keeps the document they are
/// reading, is told beside it, and is asked nothing. The entry is gone the next time the
/// menu is built.
#[test]
fn a_document_that_went_away_under_the_open_menu_says_so_and_drops_out() {
    let fixture = Fixture::new("recent-menu-vanished");
    let vanishing = fixture.write("vanishing.md", "# Vanishing\n");
    let reading = fixture.write("reading.md", "# Reading\n");

    let app = axiomd_e2e::launch(&vanishing);
    app.wait_until_recent(&vanishing);
    read(&app, &reading);

    open_the_menu(&app);
    let press = the_way_back_to(&app, "vanishing.md");
    std::fs::remove_file(&vanishing).expect("delete a document");
    app.activate(&press);

    assert_eq!(
        app.wait_for_banner("Could not open"),
        "Could not open vanishing.md — showing the last version read",
        "the window did not say why the document the reader pressed did not arrive",
    );
    assert_eq!(
        app.dom_text("h1"),
        "Reading",
        "a document that could not be opened took the reader's own off the screen",
    );
    assert_eq!(
        app.visible_dialog(),
        "",
        "a document that is not there raised a dialog on the open path",
    );

    // The next menu the reader opens no longer offers it. A new window, because this
    // harness cannot shut a menu the way a reader does — its menu is built at the
    // moment it opens, exactly as the one before it was.
    app.activate("app.new");
    app.wait_until_windows(2);
    open_the_menu(&app);
    assert_eq!(
        offered(&app),
        ["reading.md"],
        "a document that could not be opened is still being offered",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Two documents of the same name are told apart by where they are kept, so the reader
/// is never choosing between two identical words.
#[test]
fn documents_that_share_a_name_are_told_apart_in_the_menu() {
    let fixture = Fixture::new("recent-menu-same-name");
    let wayfinder = fixture.write("wayfinder/notes.md", "# Wayfinder\n");
    let atlas = fixture.write("atlas/notes.md", "# Atlas\n");
    let guide = fixture.write("atlas/guide.md", "# Guide\n");

    let app = axiomd_e2e::launch(&wayfinder);
    app.wait_until_recent(&wayfinder);
    read(&app, &atlas);
    read(&app, &guide);

    open_the_menu(&app);
    assert_eq!(
        offered(&app),
        ["guide.md", "notes.md (atlas)", "notes.md (wayfinder)"],
        "the reader is being offered the same word for two different documents",
    );
    // And the two of them really are the two documents, not one offered twice.
    assert_ne!(
        the_way_back_to(&app, "notes.md (atlas)"),
        the_way_back_to(&app, "notes.md (wayfinder)"),
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A reader with nothing to come back to — a first run, or a desktop whose File History
/// is off — is offered nothing, and told nothing: an empty way back is not an error
/// (issue #54, `ux_decisions.md`).
#[test]
fn a_reader_with_no_recent_documents_is_offered_none_and_told_nothing() {
    let app = axiomd_e2e::launch_without_document();

    open_the_menu(&app);
    let menu = app.menu();
    assert_eq!(
        offered(&app),
        Vec::<String>::new(),
        "a desktop with no recent documents offered some anyway",
    );
    assert!(
        !menu
            .offers()
            .iter()
            .any(|(label, _)| *label == "Open Recent"),
        "the menu offers a way back to nothing at all: {:?}",
        menu.offers(),
    );
    assert_eq!(app.visible_dialog(), "", "an empty list raised a dialog");
    assert_eq!(app.banner(), "", "an empty list said something went wrong");

    assert!(app.close().is_empty(), "the launch left processes behind");
}
