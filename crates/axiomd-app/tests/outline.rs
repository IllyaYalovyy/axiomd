//! UT-005: the document's headings beside the document, and the reader's place in it.
//!
//! Every test here drives the shipped binary on a headless compositor and reads the
//! sidebar back the way the reader sees it: which sections it lists, which one is
//! highlighted, whether it is there at all. The two numbers that recur are the ones the
//! feature must not cost — the navigation count, because going to a section is not a
//! page load, and how many times the page has reported the reader's place, because a
//! bridge that answers every scroll event instead of every frame is Apostrophe's
//! mistake reproduced.

use axiomd_e2e::{App, Fixture, Preferences};

/// The sections of the test document, in order, with the level each is written at.
const SECTIONS: [(usize, &str); 4] = [
    (2, "Getting started"),
    (3, "Requirements"),
    (2, "Reference"),
    (2, "Notes"),
];

/// A guide whose every section is longer than a window, so that a reader can be
/// somewhere in it and the section they are in is a question with an answer.
fn guide() -> String {
    let mut source = String::from("# Guide\n\nOpening words.\n\n");
    for (level, title) in SECTIONS {
        source.push_str(&format!("{} {title}\n\n", "#".repeat(level)));
        for paragraph in 1..=40 {
            source.push_str(&format!("{title} paragraph {paragraph}.\n\n"));
        }
    }
    source
}

/// The source line a heading is written on — what an outline entry names, and where
/// the caret lands when the entry is picked in edit mode.
fn line_of(source: &str, heading: &str) -> u32 {
    let at = source
        .lines()
        .position(|line| line.trim_start_matches('#').trim() == heading && line.starts_with('#'))
        .unwrap_or_else(|| panic!("{heading:?} is not a heading of the document"));
    at as u32 + 1
}

/// Where the heading reading `text` sits on screen, rounded to whole pixels.
fn screen_position_of(app: &App, text: &str) -> i32 {
    let script = format!(
        "Math.round(Array.from(document.querySelectorAll('h1, h2, h3')) \
         .find(heading => heading.textContent === {text:?}) \
         .getBoundingClientRect().top)"
    );
    app.dom(&script)
        .parse()
        .unwrap_or_else(|_| panic!("{text:?} is not a heading of the document on screen"))
}

fn scroll_offset(app: &App) -> i32 {
    app.dom("Math.round(document.scrollingElement.scrollTop)")
        .parse()
        .expect("a scroll offset")
}

/// The outline is the document's headings, at the level the document wrote them, and
/// it is there when the document arrives rather than after the reader asks.
#[test]
fn the_outline_lists_the_documents_headings_beside_it() {
    let fixture = Fixture::new("outline-lists");
    let app = axiomd_e2e::launch(&fixture.write("guide.md", &guide()));

    let outline = app.outline();
    assert!(outline.shown, "the outline was not beside the document");
    assert_eq!(
        outline.headings,
        [
            "h1 Guide",
            "h2 Getting started",
            "h3 Requirements",
            "h2 Reference",
            "h2 Notes",
        ],
    );
    assert!(
        outline.notice.is_empty(),
        "a document with headings was told it has none: {}",
        outline.notice,
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The whole of UT-005's second step: the reader picks a section and the document is
/// at that section — without the page being loaded again.
#[test]
fn picking_a_section_takes_the_reader_to_it_without_reloading_the_page() {
    let fixture = Fixture::new("outline-pick");
    let source = guide();
    let app = axiomd_e2e::launch(&fixture.write("guide.md", &source));
    assert_eq!(
        scroll_offset(&app),
        0,
        "the document did not start at the top"
    );

    app.press("Reference");

    app.wait_for("the document to arrive at the section", || {
        screen_position_of(&app, "Reference").abs() <= 4
    });
    assert!(
        scroll_offset(&app) > 0,
        "the document never scrolled to the section",
    );
    assert_eq!(
        app.navigation_count(),
        1,
        "the section was reached by loading the page again",
    );
    app.wait_until_section("Reference");
    // And the entry names the source line the anchor map has for that heading.
    assert_eq!(
        app.dom("document.querySelector('h2[id=\"reference\"]').dataset.line"),
        line_of(&source, "Reference").to_string(),
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The other half of UT-005: the outline tracks the reader while they scroll, and the
/// end of the document is the last section rather than none.
#[test]
fn scrolling_the_document_moves_the_highlight_to_the_section_being_read() {
    let fixture = Fixture::new("outline-tracks");
    let app = axiomd_e2e::launch(&fixture.write("guide.md", &guide()));
    app.wait_for("the page to say where the reader is", || {
        app.section_reports() > 0
    });
    assert_eq!(
        app.outline().section,
        "",
        "a reader at the very top, above every heading, was already in a section",
    );

    app.dom("document.querySelector('h3[id=\"requirements\"]').scrollIntoView(true)");
    app.wait_until_section("Requirements");

    app.dom("document.scrollingElement.scrollTop = document.scrollingElement.scrollHeight");
    app.wait_until_section("Notes");

    // Back to the top, and back out of every section.
    app.dom("document.scrollingElement.scrollTop = 0");
    app.wait_until_section("");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The bridge's promise, and the whole reason it is not a scroll listener: a movement
/// that carries the reader past every section of a document is one message, not one
/// per section it passed.
///
/// Jumping to the end of the document crosses all five headings in a single frame. A
/// bridge that answered each crossing would send five; one delivered with the frame
/// sends one.
#[test]
fn the_page_reports_the_readers_section_once_however_far_they_move() {
    let fixture = Fixture::new("outline-throttle");
    let app = axiomd_e2e::launch(&fixture.write("guide.md", &guide()));
    // Settled first: the page also reports once after every render, and this test is
    // about what moving costs rather than what rendering does.
    app.wait_for("the page to say where the reader is", || {
        app.section_reports() > 0
    });
    let before = app.section_reports();

    app.dom("document.scrollingElement.scrollTop = document.scrollingElement.scrollHeight");

    app.wait_until_section("Notes");
    assert_eq!(
        app.section_reports(),
        before + 1,
        "crossing five headings in one frame produced {} messages",
        app.section_reports() - before,
    );

    // And reading on inside that section costs nothing at all: the answer cannot have
    // changed, so nothing is said.
    let settled = app.section_reports();
    let drawn: f64 = app
        .dom("String(document.timeline.currentTime)")
        .parse()
        .expect("the page's own clock");
    app.dom("document.scrollingElement.scrollTop -= 60");
    // A frame has gone by since that scroll, so a bridge that was going to speak has
    // had its chance to.
    app.wait_until(&format!("Number(document.timeline.currentTime) > {drawn}"));
    assert_eq!(
        app.section_reports(),
        settled,
        "scrolling inside one section still cost a message",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Putting a reader back where they were is not pixel-exact: a live reload restores
/// their place to within a pixel or two of it, and gliding a heading to the top can
/// leave it a fraction of a device pixel below it. A section they are looking at from
/// two pixels off is still the section they are in — reading it as the one above is
/// the sidebar telling them they are somewhere they are not.
#[test]
fn a_section_reached_a_pixel_or_two_short_is_still_the_section_being_read() {
    let fixture = Fixture::new("outline-slack");
    let app = axiomd_e2e::launch(&fixture.write("guide.md", &guide()));

    app.dom("document.querySelector('h2[id=\"reference\"]').scrollIntoView(true)");
    app.wait_until_section("Reference");

    let drawn: f64 = app
        .dom("String(document.timeline.currentTime)")
        .parse()
        .expect("the page's own clock");
    app.dom("document.scrollingElement.scrollTop -= 2");
    // A frame has gone by, so the page has had its chance to say otherwise.
    app.wait_until(&format!("Number(document.timeline.currentTime) > {drawn}"));

    assert_eq!(
        app.dom(
            "String(Math.round(document.querySelector('h2[id=\"reference\"]')\
                 .getBoundingClientRect().top))"
        ),
        "2",
        "the test did not manage to leave the heading two pixels short of the top",
    );
    assert_eq!(
        app.outline().section,
        "Reference",
        "two pixels short of a section counted as being in the one before it",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Live reload (#5) meets the outline: the file gains a section under the reader, the
/// list follows it, and the reader is still in the section they were reading — which
/// is now several source lines further down the file.
#[test]
fn the_outline_follows_a_live_reload_without_losing_the_readers_section() {
    let fixture = Fixture::new("outline-reload");
    let document = fixture.write("guide.md", &guide());
    let app = axiomd_e2e::launch(&document);

    app.dom("document.querySelector('h2[id=\"reference\"]').scrollIntoView(true)");
    app.wait_until_section("Reference");
    let before = screen_position_of(&app, "Reference");

    // A section inserted above everything the reader can see: every source line below
    // it moves, and none of the blocks on their screen do.
    let grown = guide().replace(
        "# Guide\n\nOpening words.\n\n",
        "# Guide\n\nOpening words.\n\n## Preface\n\nAdded later.\n\n",
    );
    std::fs::write(&document, &grown).expect("save the document");

    app.wait_for("the outline to gain the new section", || {
        app.outline().headings.contains(&"h2 Preface".to_owned())
    });
    assert_eq!(
        app.outline().headings,
        [
            "h1 Guide",
            "h2 Preface",
            "h2 Getting started",
            "h3 Requirements",
            "h2 Reference",
            "h2 Notes",
        ],
    );
    assert_eq!(
        app.outline().section,
        "Reference",
        "the reader's section was lost when the document changed under them",
    );
    let after = screen_position_of(&app, "Reference");
    assert!(
        (after - before).abs() <= 2,
        "the reader's section moved from {before}px to {after}px on screen",
    );
    assert_eq!(app.navigation_count(), 1);

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The non-happy path the sidebar exists to have an answer for: a document with no
/// sections says so where the sections would be, and nothing about it is a dialog.
#[test]
fn a_document_with_no_headings_says_so_where_its_headings_would_be() {
    let fixture = Fixture::new("outline-empty");
    let app = axiomd_e2e::launch(&fixture.write("plain.md", "Just words.\n\n- and a list\n"));

    let outline = app.outline();
    assert!(outline.shown, "the sidebar went away instead of saying so");
    assert!(outline.headings.is_empty(), "{:?}", outline.headings);
    assert_eq!(outline.notice, "No headings");
    assert_eq!(outline.section, "");
    assert_eq!(
        app.visible_dialog(),
        "",
        "opening a document without headings raised a dialog",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// `F9` and the header-bar button are one action and one state, so the button can
/// never say the sidebar is open while it is shut.
#[test]
fn the_outline_is_shown_and_hidden_by_the_action_the_key_and_the_button_share() {
    let fixture = Fixture::new("outline-toggle");
    let app = axiomd_e2e::launch(&fixture.write("guide.md", &guide()));
    assert!(app.outline().shown);

    app.activate("win.outline");
    assert!(!app.outline().shown, "F9 did not take the outline away");
    // The document is still there, and still the whole of the window.
    assert!(app.showing_document());
    assert_eq!(app.dom_text("h1"), "Guide");

    app.activate("win.outline");
    assert!(app.outline().shown, "F9 did not bring the outline back");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A window too narrow to hold both gets out of the document's way by itself, and the
/// reader can still call the outline up over it.
#[test]
fn a_narrow_window_reads_without_the_outline_taking_the_document_s_room() {
    let fixture = Fixture::new("outline-narrow");
    let app = axiomd_e2e::launch(&fixture.write("guide.md", &guide()));
    assert!(app.outline().shown);

    app.resize(480, 700);

    app.wait_for("the outline to get out of a narrow window's way", || {
        !app.outline().shown
    });
    assert!(app.showing_document());
    assert_eq!(app.dom_text("h1"), "Guide");

    // And it is still one key away, over the document rather than beside it.
    app.activate("win.outline");
    assert!(app.outline().shown, "the outline could not be called up");
    assert_eq!(
        app.outline().headings.first().map(String::as_str),
        Some("h1 Guide"),
    );

    app.resize(1000, 700);
    app.wait_for("the outline to come back beside the document", || {
        app.outline().shown
    });

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The sidebar is beside both surfaces, so picking a section has to mean something in
/// both: in edit mode it is where the caret goes.
#[test]
fn picking_a_section_while_editing_puts_the_caret_on_it() {
    let fixture = Fixture::new("outline-editing");
    let source = guide();
    let app = axiomd_e2e::launch(&fixture.write("guide.md", &source));

    app.activate("win.mode");
    app.wait_until_mode("edit");

    app.press("Requirements");

    assert_eq!(app.caret_line(), line_of(&source, "Requirements"));
    app.wait_until_section("Requirements");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Whether documents are read with their outline beside them is the reader's to
/// choose, and the choice applies where they are (invariant 14) — no reopening, no
/// restart, and nothing re-rendered for it.
#[test]
fn the_reader_can_switch_the_outline_off_and_it_goes_at_once() {
    let fixture = Fixture::new("outline-preference");
    let preferences = Preferences::new("outline-preference");
    let app = axiomd_e2e::launch_with(&fixture.write("guide.md", &guide()), &preferences);
    assert!(app.outline().shown);
    let rendered = app.render_count();

    app.activate("app.preferences");
    app.wait_for_dialog_saying("Preferences");
    assert_eq!(app.preference("Show the outline"), "true");
    app.set_preference("Show the outline", "false");

    preferences.wait_until("outline", "false");
    app.wait_for("the outline to go", || !app.outline().shown);
    assert_eq!(
        app.render_count(),
        rendered,
        "the document was rendered again to change a preference",
    );
    assert_eq!(app.navigation_count(), 1);

    // A window opened afterwards is the reader's way round too.
    let second = axiomd_e2e::launch_with(&fixture.write("second.md", &guide()), &preferences);
    assert!(
        !second.outline().shown,
        "a new window ignored the reader's choice",
    );

    assert!(
        second.close().is_empty(),
        "the launch left processes behind"
    );
    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// How wide the outline is for a reader who has never touched the divider, how narrow
/// it goes, and how wide it goes in the 900px window every launch here starts in — the
/// last being what is left after the document keeps a document's worth of room.
const USUAL: i32 = 260;
const NARROWEST: i32 = 180;
const WIDEST_IN_THIS_WINDOW: i32 = 420;

/// The least the document keeps beside the outline, whatever the reader drags.
const ROOM_FOR_THE_DOCUMENT: i32 = 480;

/// The owner's ruling in one test (issue #27): the reader drags the divider, the
/// outline is that wide, and it is still that wide the next time they open a document.
#[test]
fn dragging_the_divider_widens_the_outline_and_the_width_comes_back_next_time() {
    let fixture = Fixture::new("outline-resize");
    let preferences = Preferences::new("outline-resize");
    let document = fixture.write("guide.md", &guide());
    let app = axiomd_e2e::launch_with(&document, &preferences);

    let before = app.layout();
    assert_eq!(
        before.sidebar.width, USUAL,
        "a reader who has never touched the divider did not get the usual width",
    );

    app.drag_divider(90);
    app.wait_until_sidebar_width(USUAL + 90);

    // The room came out of the document beside it, and the two still meet: an outline
    // that widened over the document would be covering what it is an index of.
    let widened = app.layout();
    assert_eq!(
        widened.document.width,
        before.document.width - 90,
        "the document did not give up exactly the room the outline took",
    );
    assert_eq!(
        widened.document.x,
        widened.sidebar.right(),
        "the outline and the document are not meeting at the divider",
    );
    assert_eq!(
        app.outline().headings.first().map(String::as_str),
        Some("h1 Guide"),
        "the outline stopped listing the document it was widened beside",
    );

    // Window state, not a preference: it is written down where a window's own size is
    // written down, and no dialog was involved (`ux_decisions.md`).
    preferences.wait_until("sidebar-width", &(USUAL + 90).to_string());
    assert_eq!(
        app.visible_dialog(),
        "",
        "dragging the divider raised a dialog"
    );
    assert!(app.close().is_empty(), "the launch left processes behind");

    let again = axiomd_e2e::launch_with(&document, &preferences);
    assert_eq!(
        again.layout().sidebar.width,
        USUAL + 90,
        "the width the reader dragged to was not there when they came back",
    );
    assert!(again.close().is_empty(), "the launch left processes behind");
}

/// The non-happy path: a reader who shoves the divider as far as it goes, either way.
/// Neither pane may be crushed to something unusable, so both ends stop.
#[test]
fn the_divider_stops_before_either_pane_is_crushed() {
    let fixture = Fixture::new("outline-bounds");
    let app = axiomd_e2e::launch(&fixture.write("guide.md", &guide()));

    app.drag_divider(10_000);
    app.wait_until_sidebar_width(WIDEST_IN_THIS_WINDOW);
    let widest = app.layout();
    assert!(
        widest.document.width >= ROOM_FOR_THE_DOCUMENT,
        "the document was left {}px to be read in",
        widest.document.width,
    );
    assert_eq!(
        app.dom_text("h1"),
        "Guide",
        "the document beside the widest outline is not readable",
    );

    app.drag_divider(-10_000);
    app.wait_until_sidebar_width(NARROWEST);
    assert_eq!(
        app.outline().headings.first().map(String::as_str),
        Some("h1 Guide"),
        "the narrowest outline stopped being a list of headings",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The way back. A reader who has dragged themselves somewhere they did not mean to go
/// double-clicks the divider, and the outline is the width it started at — for good,
/// not just until the window closes.
#[test]
fn double_clicking_the_divider_puts_the_outline_back() {
    let fixture = Fixture::new("outline-restore");
    let preferences = Preferences::new("outline-restore");
    let app = axiomd_e2e::launch_with(&fixture.write("guide.md", &guide()), &preferences);

    app.drag_divider(120);
    app.wait_until_sidebar_width(USUAL + 120);
    preferences.wait_until("sidebar-width", &(USUAL + 120).to_string());

    app.restore_divider();
    app.wait_until_sidebar_width(USUAL);

    let second = axiomd_e2e::launch_with(&fixture.write("second.md", &guide()), &preferences);
    assert_eq!(
        second.layout().sidebar.width,
        USUAL,
        "a window opened after the divider was put back is still the dragged width",
    );

    assert!(
        second.close().is_empty(),
        "the launch left processes behind"
    );
    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The width and the breakpoint together (issue #27 meets issue #7): a window too
/// narrow to hold both panes still takes the outline away, and the width the reader
/// chose while it was wide is what they get back when it is wide again.
#[test]
fn a_width_chosen_while_wide_comes_back_when_the_window_is_wide_again() {
    let fixture = Fixture::new("outline-resize-narrow");
    let app = axiomd_e2e::launch(&fixture.write("guide.md", &guide()));

    app.drag_divider(90);
    app.wait_until_sidebar_width(USUAL + 90);

    app.resize(480, 700);
    app.wait_for("the outline to get out of a narrow window's way", || {
        !app.outline().shown
    });
    assert!(app.showing_document());
    assert_eq!(app.dom_text("h1"), "Guide");

    // And `F9` still calls it up over the document, as it does at any width.
    app.activate("win.outline");
    assert!(app.outline().shown, "the outline could not be called up");

    app.resize(1000, 700);
    app.wait_for("the outline to come back beside the document", || {
        app.outline().shown
    });
    app.wait_until_sidebar_width(USUAL + 90);

    assert!(app.close().is_empty(), "the launch left processes behind");
}
