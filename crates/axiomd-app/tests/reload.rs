//! UT-004: the file changes under the reader, and the document follows it.
//!
//! Every test here drives the shipped binary on a headless compositor, edits the
//! document the way another program would — a plain save, a burst of saves, an
//! editor's write-and-rename, a deletion — and reads the result out of the view the
//! user is looking at.
//!
//! Two numbers recur, and they are the point of the feature. The navigation count
//! must not move: a re-render that navigates the view is the full-page reload that
//! flashes the window and loses the reader's place (`design_decisions.md`). And the
//! reader's place, measured as the position on screen of the content they were
//! looking at, must survive an edit that moves that content up or down the document.

use std::path::Path;

use axiomd_e2e::{App, Fixture};

const V1: &str = "\
# Release Notes

The first version of the document.

## Details

A second section.
";

const V2: &str = "\
# Release Notes

The second version of the document.

## Details

A second section.
";

/// Saves `contents` over `document` the way a plain editor does: in place.
fn save(document: &Path, contents: &str) {
    std::fs::write(document, contents).unwrap_or_else(|error| panic!("save {document:?}: {error}"));
}

/// Saves `contents` the way vim, emacs and VS Code do: a new file beside the old one,
/// renamed over the top. The document the window opened no longer exists afterwards.
fn save_by_replacing(document: &Path, contents: &str) {
    let written = document.with_extension("md.new");
    std::fs::write(&written, contents).unwrap_or_else(|error| panic!("write {written:?}: {error}"));
    std::fs::rename(&written, document)
        .unwrap_or_else(|error| panic!("rename {written:?} over {document:?}: {error}"));
}

/// A document long enough to scroll, with a paragraph the tests can steer by.
///
/// Paragraph `n` is on source line `2n + 1`, so a test can name a block by its anchor
/// as well as by its text.
fn long_document(inserted: usize) -> String {
    let mut source = String::from("# Notes\n\n");
    for extra in 1..=inserted {
        source.push_str(&format!("Inserted paragraph {extra}.\n\n"));
    }
    for paragraph in 1..=120 {
        source.push_str(&format!("Paragraph {paragraph}.\n\n"));
    }
    source
}

/// Where the `selector` element reading `text` sits on screen, rounded to whole pixels.
fn screen_position_of(app: &App, selector: &str, text: &str) -> i32 {
    let script = format!(
        "Math.round(Array.from(document.querySelectorAll({selector:?})) \
         .find(block => block.textContent === {text:?}) \
         .getBoundingClientRect().top)"
    );
    app.dom(&script)
        .parse()
        .unwrap_or_else(|_| panic!("{text:?} is not a {selector} of the document on screen"))
}

fn scroll_offset(app: &App) -> i32 {
    app.dom("Math.round(document.scrollingElement.scrollTop)")
        .parse()
        .expect("a scroll offset")
}

/// The whole of UT-004's first half: the file changes, the view follows, and the page
/// is never navigated.
#[test]
fn an_external_save_reaches_the_reader_without_reloading_the_page() {
    let fixture = Fixture::new("reload-in-place");
    let document = fixture.write("notes.md", V1);
    let app = axiomd_e2e::launch(&document);
    assert_eq!(app.dom_text("p"), "The first version of the document.");

    // The reader has selected the heading — something they can only still have
    // afterwards if the element carrying it is the same one. A page reload, or a
    // wholesale replacement of the article's contents, builds a new <h1> out of the
    // new markup and takes the selection with the old one.
    app.dom(
        "(() => { const range = document.createRange(); \
         range.selectNodeContents(document.querySelector('h1')); \
         const selected = window.getSelection(); \
         selected.removeAllRanges(); selected.addRange(range); \
         return selected.toString(); })()",
    );
    assert_eq!(
        app.dom("window.getSelection().toString()"),
        "Release Notes",
        "the test could not select the heading to begin with",
    );

    save(&document, V2);

    app.wait_until(
        "document.querySelector('p').textContent === 'The second version of the document.'",
    );
    assert_eq!(
        app.navigation_count(),
        1,
        "the changed document was shown by navigating the view, not by patching it",
    );
    assert_eq!(
        app.dom("window.getSelection().toString()"),
        "Release Notes",
        "the heading was rebuilt although the edit did not touch it, \
         taking the reader's selection with it",
    );
    assert_eq!(app.dom_text("h1"), "Release Notes");
    assert_eq!(app.dom_text("h2"), "Details");
    assert_eq!(app.window_title(), "notes.md");
    assert!(app.showing_document());
    assert!(app.banner().is_empty(), "an ordinary save raised a banner");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Invariant 5, at the layer where it is visible: content appears above what the user
/// is reading, and what they are reading does not move on screen.
///
/// Source lines all shift when text is inserted above them, so this cannot be — and is
/// not — restored by remembering a line number and scrolling back to it.
#[test]
fn an_edit_above_the_reader_leaves_them_looking_at_the_same_paragraph() {
    let fixture = Fixture::new("reload-anchor");
    let document = fixture.write("notes.md", &long_document(0));
    let app = axiomd_e2e::launch(&document);

    // Paragraph 40 is on source line 81, and the reader is looking at it.
    app.dom("document.querySelector('[data-line=\"81\"]').scrollIntoView(true)");
    let before = screen_position_of(&app, "p", "Paragraph 40.");
    let scrolled = scroll_offset(&app);
    assert!(
        scrolled > 0,
        "the document did not scroll, so there is nothing to preserve"
    );

    save(&document, &long_document(5));
    app.wait_until("document.querySelectorAll('p').length === 125");

    let after = screen_position_of(&app, "p", "Paragraph 40.");
    assert!(
        (after - before).abs() <= 2,
        "the reader's paragraph moved from {before}px to {after}px on screen",
    );
    assert!(
        scroll_offset(&app) > scrolled,
        "the view did not scroll to follow content that was inserted above it",
    );
    // The blocks that were kept moved down the file, and the anchor map moved with
    // them. Outline navigation, search and scroll sync all read it: a patched document
    // whose anchors still describe the old source is a broken document.
    assert_eq!(
        app.dom("document.querySelector('[data-line=\"91\"]').textContent"),
        "Paragraph 40.",
        "the anchors of the kept blocks still point at the old source lines",
    );
    assert_eq!(app.navigation_count(), 1);

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The other half of the anchor rule: the block the reader was on is gone, so the
/// nearest surviving one before it takes its place.
#[test]
fn an_edit_that_deletes_the_readers_block_falls_back_to_the_one_before_it() {
    let fixture = Fixture::new("reload-deleted-block");
    let document = fixture.write("notes.md", &long_document(0));
    let app = axiomd_e2e::launch(&document);

    app.dom("document.querySelector('[data-line=\"81\"]').scrollIntoView(true)");
    let before = screen_position_of(&app, "p", "Paragraph 40.");

    // Paragraph 40 is deleted; everything above it is untouched, so paragraph 39 is
    // the nearest surviving predecessor of the block the reader was looking at.
    let without_40 = long_document(0).replace("Paragraph 40.\n\n", "");
    save(&document, &without_40);
    app.wait_until("document.querySelectorAll('p').length === 119");

    let after = screen_position_of(&app, "p", "Paragraph 39.");
    assert!(
        (after - before).abs() <= 2,
        "the paragraph before the deleted one landed at {after}px, not at {before}px",
    );
    assert_eq!(app.navigation_count(), 1);

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A long list is one block, and one item of it changing must not cost the reader the
/// rest of it: the shape issue #38 is about, arriving from the file rather than from a
/// press on a checkbox. Matching blocks by their content alone would find nothing to
/// keep here — the one `<ul>` the whole list is changed — and would rebuild every item
/// in it, moving the reader and taking whatever they had selected with it.
#[test]
fn an_edit_inside_a_long_list_keeps_the_rest_of_the_list() {
    let fixture = Fixture::new("reload-inside-a-list");
    let document = fixture.write("notes.md", &long_list("Item 3."));
    let app = axiomd_e2e::launch(&document);
    app.wait_until("document.querySelectorAll('li').length === 100");

    // The reader is deep in the list, with one of its items selected — something they
    // can only still have afterwards if the element carrying it is the same one.
    app.dom("document.querySelectorAll('li')[59].scrollIntoView(true)");
    let before = screen_position_of(&app, "li", "Item 60.");
    let scrolled = scroll_offset(&app);
    assert!(
        scrolled > 0,
        "the list did not scroll, so there is nothing to preserve"
    );
    app.dom(
        "(() => { const range = document.createRange(); \
         range.selectNodeContents(document.querySelectorAll('li')[59]); \
         const selected = window.getSelection(); \
         selected.removeAllRanges(); selected.addRange(range); \
         return selected.toString(); })()",
    );
    assert_eq!(
        app.dom("window.getSelection().toString()"),
        "Item 60.",
        "the test could not select an item to begin with",
    );

    // One item near the top of the same list is rewritten, on one line as before.
    save(&document, &long_list("Item 3, revised."));
    app.wait_until("document.querySelectorAll('li')[2].textContent === 'Item 3, revised.'");

    assert_eq!(
        scroll_offset(&app),
        scrolled,
        "an edit inside the list moved the reader, who is not reading that part of it",
    );
    assert_eq!(
        screen_position_of(&app, "li", "Item 60."),
        before,
        "the item the reader was on did not stay where it was on screen",
    );
    assert_eq!(
        app.dom("window.getSelection().toString()"),
        "Item 60.",
        "the reader's item was rebuilt although the edit did not touch it, \
         taking their selection with it",
    );
    assert_eq!(app.navigation_count(), 1);

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A hundred items in one list, with the third of them written by the caller.
fn long_list(third: &str) -> String {
    let mut source = String::from("# Notes\n\n");
    for item in 1..=100 {
        match item {
            3 => source.push_str(&format!("- {third}\n")),
            _ => source.push_str(&format!("- Item {item}.\n")),
        }
    }
    source
}

/// A save is not one write: an editor, a formatter and a linter can each touch the
/// file within a few milliseconds. The debounce is what stops that burst from becoming
/// a burst of renders, and the count of finished pages is where it shows.
///
/// The bound is deliberately loose rather than exact: how many change events the
/// kernel and GLib deliver for twenty writes is theirs to decide, and pinning a number
/// on it would be pinning their behaviour, not axiomd's. One page per save would be
/// twenty-one. The exact coalescing is asserted deterministically in the unit test
/// `a_burst_of_changes_is_reported_once`.
#[test]
fn a_burst_of_saves_does_not_become_a_burst_of_renders() {
    let fixture = Fixture::new("reload-burst");
    let document = fixture.write("notes.md", "# Notes\n\nVersion 0.\n");
    let app = axiomd_e2e::launch(&document);
    let before = app.render_count();

    for version in 1..=20 {
        save(&document, &format!("# Notes\n\nVersion {version}.\n"));
    }

    app.wait_until("document.querySelector('p').textContent === 'Version 20.'");
    let rendered = app.render_count() - before;
    assert!(
        rendered <= 3,
        "twenty saves in a burst produced {rendered} renders",
    );
    assert_eq!(app.navigation_count(), 1);

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// How most editors save: write a new file, rename it over the old one. The document
/// the window opened stops existing at that moment, and the view must still follow —
/// twice over, because the watch has to survive the replacement to see the next save.
#[test]
fn an_editor_that_replaces_the_file_keeps_one_window_on_the_document() {
    let fixture = Fixture::new("reload-rename");
    let document = fixture.write("notes.md", V1);
    let app = axiomd_e2e::launch(&document);

    save_by_replacing(&document, V2);
    app.wait_until(
        "document.querySelector('p').textContent === 'The second version of the document.'",
    );
    assert_eq!(app.navigation_count(), 1);
    assert!(app.banner().is_empty(), "a completed save raised a banner");

    // The window holds its document by the file's identity on disk, and the rename
    // gave the path a new one. A window still holding the identity of the replaced
    // file no longer recognises its own document: opening it again opens a second
    // window on the same file.
    app.open(&document);
    assert_eq!(
        app.window_count(),
        1,
        "the replaced document opened a second window",
    );

    // And the watch survived the replacement.
    save(&document, "# Release Notes\n\nA third version.\n");
    app.wait_until("document.querySelector('p').textContent === 'A third version.'");
    assert_eq!(app.navigation_count(), 1);

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A file that goes away does not take the document off the screen with it. The
/// reader keeps the last version they had, and is told beside it — never in a dialog
/// (`ux_decisions.md`), and never by having the view replaced with an error page.
#[test]
fn a_deleted_file_leaves_the_last_version_on_screen_behind_a_banner() {
    let fixture = Fixture::new("reload-deleted");
    let document = fixture.write("notes.md", V1);
    let app = axiomd_e2e::launch(&document);

    std::fs::remove_file(&document).expect("delete the document");

    let banner = app.wait_for_banner("notes.md");
    assert!(
        app.showing_document(),
        "deleting the file took the document off the screen: {banner}",
    );
    assert_eq!(app.dom_text("p"), "The first version of the document.");
    assert_eq!(app.navigation_count(), 1);

    // The file comes back — an editor that saves by replacing, or an undo in the
    // file manager. The document follows it and the banner goes.
    save_by_replacing(&document, V2);

    app.wait_until(
        "document.querySelector('p').textContent === 'The second version of the document.'",
    );
    app.wait_until_no_banner();
    assert_eq!(app.navigation_count(), 1);

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Windows share nothing (invariant 7): a save reaches the window whose document it
/// is and no other. Apostrophe's cross-window leaks are the named anti-pattern.
#[test]
fn a_save_reaches_the_window_holding_that_document_and_no_other() {
    let fixture = Fixture::new("reload-two-windows");
    let first = fixture.write("first.md", "# First\n\nThe first document.\n");
    let second = fixture.write("second.md", "# Second\n\nThe second document.\n");

    let app = axiomd_e2e::launch(&first);
    app.open(&second);
    assert_eq!(app.window_count(), 2);

    save(&first, "# First\n\nThe first document, edited.\n");

    app.select_window(0);
    app.wait_until("document.querySelector('p').textContent === 'The first document, edited.'");
    assert_eq!(app.navigation_count(), 1);

    app.select_window(1);
    assert_eq!(app.dom_text("p"), "The second document.");
    assert_eq!(app.dom_text("h1"), "Second");
    assert_eq!(app.navigation_count(), 1);

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A window given another document follows the new file, not the old one: opening is
/// one load per document, and the document that arrives is the one that reloads.
#[test]
fn a_window_given_another_document_follows_that_one() {
    let fixture = Fixture::new("reload-reopened");
    let first = fixture.write("first.md", "# First\n\nThe first document.\n");
    let second = fixture.write("second.md", "# Second\n\nThe second document.\n");

    let app = axiomd_e2e::launch(&first);
    app.open_here(&second);
    assert_eq!(
        app.window_count(),
        1,
        "the document did not take over the window"
    );
    assert_eq!(app.window_title(), "second.md");

    save(&second, "# Second\n\nThe second document, edited.\n");

    app.wait_until("document.querySelector('p').textContent === 'The second document, edited.'");
    assert_eq!(
        app.navigation_count(),
        2,
        "a document per load and no more: two documents, two loads",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A file the window could not read is a status page, not a dead end: when it becomes
/// a document, the reader gets the document — without reopening anything.
#[test]
fn a_file_that_becomes_readable_appears_without_being_reopened() {
    let fixture = Fixture::new("reload-unreadable");
    let document = fixture.write("notes.md", V1);
    std::fs::write(&document, [0xffu8, 0xfe, 0x00, 0x9f]).expect("write a file that is not text");

    let app = axiomd_e2e::launch_without_document();
    app.open(&document);
    assert!(
        !app.showing_document(),
        "a file that is not text was shown as a document",
    );

    save(&document, V1);

    app.wait_until("document.querySelector('h1') !== null");
    assert_eq!(app.dom_text("p"), "The first version of the document.");
    assert!(app.showing_document());

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Live reload feeds the same pipeline the first paint does, so a document full of
/// the characters that break a naive script — quotes, backslashes, angle brackets,
/// newlines — has to survive the trip into the DOM patch intact.
#[test]
fn a_document_of_hostile_characters_survives_being_patched_in() {
    let fixture = Fixture::new("reload-escaping");
    let document = fixture.write("notes.md", V1);
    let app = axiomd_e2e::launch(&document);

    save(
        &document,
        "# Release Notes\n\n\
         She said \"it's `\\n`\" & <b>meant</b> it — 100% \\ of the time.\n\n\
         <script>document.querySelector('h1').textContent = 'HIJACKED'</script>\n",
    );

    app.wait_until("document.querySelectorAll('p').length === 1");
    // The backticks became a <code> element, so what the reader sees is its contents.
    assert_eq!(
        app.dom_text("p"),
        "She said \"it's \\n\" & meant it — 100% \\ of the time.",
    );
    assert_eq!(
        app.dom_text("h1"),
        "Release Notes",
        "the patched-in script ran"
    );
    assert_eq!(
        app.dom("document.querySelectorAll('script').length"),
        "0",
        "a script survived into the patched document",
    );
    assert_eq!(app.navigation_count(), 1);

    assert!(app.close().is_empty(), "the launch left processes behind");
}
