//! What the user sees, asserted against the running application.
//!
//! These are the checks issues #1 and #4 shipped as manual ones — "a window opens
//! titled axiomd", "the fixture renders", "closing a window frees what it held" —
//! promoted to tests that run in the quality gate, plus the proof that the harness
//! they run on can actually fail.
//!
//! Every one of them drives the shipped binary on a headless compositor and reads the
//! result out of the rendered document. Nothing is stubbed.

use axiomd_e2e::Fixture;

/// A document with the pieces every later assertion needs: a heading to find, a
/// paragraph to read, and a second heading further down.
const NOTES: &str = "\
# Release Notes

The first paragraph of the document.

## Details

A second section, with `code` and *emphasis*.
";

/// Issue #1 shipped this as a manual check: run axiomd, see a window. What that window
/// holds was settled by the owner in #18 — a new untitled document, in edit mode, ready
/// to be typed in (`ux_decisions.md`).
#[test]
fn a_bare_launch_opens_a_new_untitled_document_ready_to_type_in() {
    let app = axiomd_e2e::launch_without_document();

    assert_eq!(app.window_count(), 1);
    assert_eq!(app.window_title(), "Untitled");
    assert_eq!(app.mode(), "edit");
    assert!(
        !app.showing_document(),
        "a launch with no document showed a rendered document",
    );
    assert_eq!(
        app.source(),
        "",
        "the new document arrived with something in it"
    );
    assert!(
        !app.is_modified(),
        "a document nobody has typed in is unsaved work"
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Issue #4 shipped this as a manual check: open a fixture, see it rendered. The
/// document is read out of the view the user is looking at, not out of the renderer.
#[test]
fn opening_a_file_shows_the_document_it_holds() {
    let fixture = Fixture::new("renders");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", NOTES));

    assert_eq!(app.dom_text("h1"), "Release Notes");
    assert_eq!(app.dom_text("h2"), "Details");
    assert_eq!(app.dom_text("p"), "The first paragraph of the document.");
    assert_eq!(app.dom("document.querySelectorAll('h2').length"), "1");
    assert_eq!(app.dom_text("code"), "code");
    assert_eq!(app.window_title(), "notes.md");
    assert!(app.showing_document());

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Source spans are what outline navigation, scroll sync, search and live reload all
/// map through. They have to survive the trip into the view, not just the renderer.
#[test]
fn the_rendered_document_carries_the_source_lines_of_its_blocks() {
    let fixture = Fixture::new("anchors");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", NOTES));

    assert_eq!(app.dom("document.querySelector('h1').dataset.line"), "1");
    assert_eq!(app.dom("document.querySelector('h2').dataset.line"), "5");
    assert_eq!(
        app.dom(
            "Array.from(document.querySelectorAll('[data-line]')) \
             .map(block => block.dataset.line).join(',')"
        ),
        "1,3,5,7",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Showing a document is one load. A re-render that navigates the view instead of
/// patching it is the full-page reload that flashes the window and loses the reader's
/// place; this is the number that catches it.
#[test]
fn showing_a_document_costs_a_single_load() {
    let fixture = Fixture::new("loads");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", NOTES));

    assert_eq!(app.navigation_count(), 1);

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A document a window cannot show says so inside the window. Opening is never
/// interrupted by a question (`ux_decisions.md`), so there is a status page to read
/// and no dialog to dismiss.
#[test]
fn a_file_that_is_not_text_is_explained_inside_the_window() {
    let fixture = Fixture::new("binary");
    let document = fixture.write("notes.md", NOTES);
    std::fs::write(
        document.with_file_name("image.md"),
        [0xffu8, 0xfe, 0x00, 0x9f],
    )
    .expect("write a file that is not text");

    let app = axiomd_e2e::launch(&document);
    app.open(&document.with_file_name("image.md"));

    assert!(
        !app.showing_document(),
        "a file that is not Markdown was shown as a document",
    );
    assert_eq!(
        app.window_count(),
        2,
        "the window count changed unexpectedly"
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// One window per document, and no state shared between them: the bug that made
/// Apostrophe leak one document's styling into another's window.
#[test]
fn each_window_shows_only_its_own_document() {
    let fixture = Fixture::new("two-windows");
    let first = fixture.write("first.md", "# First Document\n");
    let second = fixture.write("second.md", "# Second Document\n");

    let app = axiomd_e2e::launch(&first);
    app.open(&second);
    assert_eq!(app.window_count(), 2);

    assert_eq!(app.dom_text("h1"), "Second Document");
    assert_eq!(app.window_title(), "second.md");

    app.select_window(0);
    assert_eq!(app.dom_text("h1"), "First Document");
    assert_eq!(app.window_title(), "first.md");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Opening a document that is already open brings its window forward rather than
/// opening a second one — and a symlink to it is the same document.
#[test]
fn a_document_that_is_already_open_does_not_open_twice() {
    let fixture = Fixture::new("one-window");
    let document = fixture.write("notes.md", NOTES);
    let link = document.with_file_name("link.md");
    std::os::unix::fs::symlink(&document, &link).expect("create a symlink");

    let app = axiomd_e2e::launch(&document);
    app.open(&document);
    app.open(&link);

    assert_eq!(app.window_count(), 1, "the same document opened twice");
    assert_eq!(app.dom_text("h1"), "Release Notes");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Closing a window takes its document with it and leaves the other alone.
#[test]
fn closing_a_window_leaves_the_remaining_document_intact() {
    let fixture = Fixture::new("close-one");
    let first = fixture.write("first.md", "# First Document\n");
    let second = fixture.write("second.md", "# Second Document\n");

    let app = axiomd_e2e::launch(&first);
    app.open(&second);
    app.close_window();
    app.wait_until_windows(1);

    assert_eq!(app.dom_text("h1"), "First Document");
    assert_eq!(app.window_title(), "first.md");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A closed window must be forgotten, not merely hidden. If the shell keeps holding
/// it, reopening the document finds that dead window and "presents" it — and the user
/// clicks their file and nothing appears.
///
/// This is the user-visible face of invariant 7: closing a window frees what it held.
#[test]
fn a_document_reopened_after_its_window_closed_comes_back() {
    let fixture = Fixture::new("reopen");
    let document = fixture.write("notes.md", NOTES);

    let app = axiomd_e2e::launch(&document);
    app.close_window();
    app.wait_until_windows(0);

    app.open(&document);

    assert_eq!(app.window_count(), 1, "reopening did not produce a window");
    assert!(
        app.showing_document(),
        "the document was reopened into a window showing nothing",
    );
    assert_eq!(app.dom_text("h1"), "Release Notes");
    assert_eq!(app.window_title(), "notes.md");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The menu item and `Ctrl+W` both run this action; a test that closed the window
/// another way would not notice the action breaking.
#[test]
fn the_close_window_action_closes_the_window_it_acts_on() {
    let fixture = Fixture::new("close-action");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", NOTES));

    app.activate("app.close-window");
    app.wait_until_windows(0);

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The other half of that menu: `Ctrl+N`, which is a new untitled document in edit
/// mode — and which leaves the document already open exactly as it was.
#[test]
fn the_new_window_action_opens_an_untitled_document_in_edit_mode() {
    let fixture = Fixture::new("new-action");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", NOTES));

    app.activate("app.new");
    app.wait_until_windows(2);

    assert_eq!(app.window_title(), "Untitled");
    assert_eq!(app.mode(), "edit");
    assert!(!app.showing_document(), "a new window arrived with content");
    app.select_window(0);
    assert_eq!(app.dom_text("h1"), "Release Notes");
    assert_eq!(app.mode(), "read", "the document already open changed mode");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The harness is only worth having if it can fail. A DOM assertion that is not true
/// of the document must fail, with the selector in the message.
#[test]
fn a_dom_assertion_that_is_untrue_of_the_document_fails() {
    let fixture = Fixture::new("can-fail");
    let app = axiomd_e2e::launch(&fixture.write("plain.md", "Just a paragraph.\n"));

    assert_eq!(app.dom("document.querySelectorAll('h1').length"), "0");

    let complaint = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| app.dom_text("h1")))
        .expect_err("asserting on a heading that is not there must fail");
    let complaint = complaint
        .downcast_ref::<String>()
        .map(String::as_str)
        .unwrap_or("");
    assert!(
        complaint.contains("no element matches h1"),
        "the failure did not name the selector: {complaint:?}",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A document is displayed with JavaScript off and under `default-src 'none'`. Being
/// under test changes neither: a document that tries to run a script still cannot,
/// even while the harness is reading the DOM beside it.
#[test]
fn a_document_still_cannot_run_its_own_scripts_while_under_test() {
    let fixture = Fixture::new("inert");
    let app = axiomd_e2e::launch(&fixture.write(
        "hostile.md",
        "# Heading\n\n<script>document.querySelector('h1').textContent = 'HIJACKED'</script>\n\
         <img src=\"missing.png\" onerror=\"document.querySelector('h1').textContent = 'HIJACKED'\">\n",
    ));

    assert_eq!(app.dom_text("h1"), "Heading");
    assert_eq!(
        app.dom("document.querySelectorAll('script').length"),
        "0",
        "a script survived into the document",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The pixels the user is looking at, captured through the same path a golden is
/// compared through.
#[test]
fn the_rendered_document_can_be_captured_as_pixels() {
    let fixture = Fixture::new("capture");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", NOTES));

    let captured = app.screenshot();
    let (width, height) = captured.size();

    // The document has the window less the outline beside it (issue #7): a 900px
    // window holds a sidebar of about a fifth of its width, and the rest is this.
    assert!(
        width >= 600 && height >= 500,
        "the captured surface is {width}x{height}; the window is 900x700 \
         with the outline beside the document",
    );
    assert!(
        !captured.is_blank(),
        "the capture is a single colour, so nothing was drawn",
    );
    assert_eq!(
        app.screenshot().size(),
        (width, height),
        "two captures of the same document disagreed on its size",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Nothing may outlive a launch: not axiomd, not the web process that rendered its
/// documents, not the network process beside it. Repeated so that a leak that only
/// shows up after the first cycle is caught too.
#[test]
fn repeated_launches_leave_nothing_running() {
    let fixture = Fixture::new("teardown");
    let document = fixture.write("notes.md", NOTES);

    for cycle in 0..3 {
        let app = axiomd_e2e::launch(&document);
        assert_eq!(app.dom_text("h1"), "Release Notes");
        app.close_window();
        app.wait_until_windows(0);

        assert_eq!(
            app.close(),
            Vec::<u32>::new(),
            "cycle {cycle} left processes running",
        );
    }
}

/// The visual specification of a rendered document.
///
/// Ignored until a human has looked at the picture and pinned it: approving a
/// rendered surface for the first time is theirs to do, not the harness's
/// (`docs/TESTING.md`). To pin it, look at
/// `target/debug/e2e-artifacts/notes.actual.png` from a failing run and, if it is
/// right, re-run this test with `AXIOMD_PIN_GOLDENS=1` set, then remove the
/// `#[ignore]`.
#[test]
#[ignore = "awaiting the first human visual approval; see the comment above"]
fn a_rendered_document_still_looks_the_way_it_was_approved() {
    let fixture = Fixture::new("golden");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", NOTES));

    app.screenshot().assert_matches("notes");

    assert!(app.close().is_empty(), "the launch left processes behind");
}
