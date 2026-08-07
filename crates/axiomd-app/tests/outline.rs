//! UT-005: the document's headings beside the document, and the reader's place in it.
//!
//! Every test here drives the shipped binary on a headless compositor and reads the
//! sidebar back the way the reader sees it: which sections it lists, which one is
//! highlighted, whether it is there at all. The two numbers that recur are the ones the
//! feature must not cost — the navigation count, because going to a section is not a
//! page load, and how many times the page has reported the reader's place, because a
//! bridge that answers every scroll event instead of every frame is Apostrophe's
//! mistake reproduced.

use axiomd_e2e::{App, Fixture, Preferences, Screenshot};

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
        outline.headings(),
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
        app.outline().headings().contains(&"h2 Preface".to_owned())
    });
    assert_eq!(
        app.outline().headings(),
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
    assert!(outline.headings().is_empty(), "{:?}", outline.headings());
    assert_eq!(outline.notice, "No headings");
    assert_eq!(outline.section, "");
    assert_eq!(
        app.visible_dialog(),
        "",
        "opening a document without headings raised a dialog",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Issue #32: a window opened with nothing in it reads without a sidebar standing
/// empty beside it.
///
/// "No headings" is the right answer for a document that *has* none; for a page the
/// reader has not written yet it is a fifth of the window saying nothing, in front of
/// the first line they came to type. The sidebar is out of the way until there is a
/// reason for it — and here that reason is the first heading they write, which arrives
/// through the same render every keystroke does.
#[test]
fn a_window_with_nothing_in_it_yet_reads_without_an_empty_sidebar_beside_it() {
    let app = axiomd_e2e::launch_without_document();
    app.wait_until_mode("edit");

    let opened = app.outline();
    assert!(
        !opened.shown,
        "an untitled window opened with an empty sidebar beside it, saying {:?}",
        opened.notice,
    );

    // The reader's first heading. The sidebar has something to list now, so it comes
    // back with it in — no key pressed, no preference changed.
    app.type_text("# Notes\n\nThe first line.\n");
    app.wait_for(
        "the sidebar to come back with the new heading in it",
        || app.outline().headings() == ["h1 Notes"],
    );
    assert!(app.outline().shown, "the sidebar listed a heading unseen");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// `F9` in an empty window is the reader saying they want the sidebar there — so it is
/// there, saying why it is empty, and once they have had their say the sidebar is
/// theirs: a heading written into one they have since shut does not push it back open
/// over them.
#[test]
fn the_reader_can_call_up_the_sidebar_of_an_untitled_window_and_it_stays_theirs() {
    let app = axiomd_e2e::launch_without_document();
    app.wait_until_mode("edit");
    assert!(!app.outline().shown);

    app.activate("win.outline");
    let asked_for = app.outline();
    assert!(asked_for.shown, "F9 did not bring the sidebar up");
    assert_eq!(
        asked_for.notice, "No headings",
        "a sidebar the reader called up says why it is empty",
    );

    app.activate("win.outline");
    app.type_text("# Notes\n");
    app.wait_for("the window to render what was typed", || {
        app.outline().headings() == ["h1 Notes"]
    });
    assert!(
        !app.outline().shown,
        "a heading pushed the sidebar back open over the reader",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Giving the untitled page a name is not opening a document: the reader who saves
/// what they are still writing does not get a sidebar pushed open beside them.
#[test]
fn saving_an_untitled_page_does_not_push_a_sidebar_open_over_the_reader() {
    let fixture = Fixture::new("outline-untitled-saved");
    let app = axiomd_e2e::launch_without_document();
    app.wait_until_mode("edit");
    app.type_text("Just words, and no heading yet.\n");

    app.save_as(&fixture.write("draft.md", ""));
    app.wait_until_saved();

    let saved = app.outline();
    assert!(
        !saved.shown,
        "saving opened a sidebar saying {:?} over the reader",
        saved.notice,
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The window the reader launched bare and then opened a document into reads that
/// document the way any opened document is read — sidebar and all. The quiet start was
/// for want of a document, and there is one now.
#[test]
fn a_document_opened_into_an_untitled_window_is_read_with_its_outline_beside_it() {
    let fixture = Fixture::new("outline-untitled-then-opened");
    let app = axiomd_e2e::launch_without_document();
    app.wait_until_mode("edit");
    assert!(!app.outline().shown);

    app.open_here(&fixture.write("guide.md", &guide()));

    app.wait_for("the opened document's sidebar", || app.outline().shown);
    assert_eq!(
        app.outline().headings().first().map(String::as_str),
        Some("h1 Guide"),
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
        app.outline().headings().first().map(String::as_str),
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
    assert_eq!(app.preference("Show Outline"), "true");
    app.set_preference("Show Outline", "false");

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
        app.outline().headings().first().map(String::as_str),
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
        app.outline().headings().first().map(String::as_str),
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

/// Issue #35, the fold: a section with sections under it carries a chevron, turning it
/// takes those sections off the screen, and turning it back brings them home.
///
/// The chevron is turned by the very action `GtkTreeExpander` puts on its own gesture
/// and on `Ctrl+Space`, so this is the reader's own path to it rather than a shortcut
/// into the model behind it.
#[test]
fn a_section_with_sections_under_it_folds_away_and_comes_back() {
    let fixture = Fixture::new("outline-fold");
    let app = axiomd_e2e::launch(&fixture.write("guide.md", &guide()));

    let opened = app.outline();
    assert_eq!(
        opened.headings(),
        [
            "h1 Guide",
            "h2 Getting started",
            "h3 Requirements",
            "h2 Reference",
            "h2 Notes",
        ],
        "a document opens with its sections showing",
    );
    let holder = opened.row("Getting started");
    assert!(
        holder.expandable && holder.expanded,
        "the section holding another one has no chevron turned down: {holder:?}",
    );
    let leaf = opened.row("Notes");
    assert!(
        !leaf.expandable,
        "a section with nothing under it was given a chevron: {leaf:?}",
    );

    app.toggle_section("Getting started");

    let folded = app.outline();
    assert_eq!(
        folded.headings(),
        ["h1 Guide", "h2 Getting started", "h2 Reference", "h2 Notes"],
        "folding a section left its own sections on screen",
    );
    let holder = folded.row("Getting started");
    assert!(
        holder.expandable && !holder.expanded,
        "the folded section's chevron is still turned down: {holder:?}",
    );

    app.toggle_section("Getting started");
    assert_eq!(
        app.outline().headings(),
        [
            "h1 Guide",
            "h2 Getting started",
            "h3 Requirements",
            "h2 Reference",
            "h2 Notes",
        ],
        "unfolding the section did not bring its sections back",
    );

    // A fold belongs to the document it was made in. The next document opened here is
    // read whole, not with a section missing because the last one happened to have a
    // section called the same thing.
    app.toggle_section("Getting started");
    app.wait_for("the section to fold away", || {
        !app.outline()
            .headings()
            .contains(&"h3 Requirements".to_owned())
    });
    app.open_here(&fixture.write("again.md", &guide()));
    app.wait_for("the second document's outline", || {
        app.outline().headings().len() == 5
    });
    assert!(
        app.outline().row("Getting started").expanded,
        "a fold made in one document was carried into the next",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The #7 rule meets the fold (issue #35): the file changes under the reader, the
/// outline follows it, and what the reader folded away stays folded away.
///
/// A rebuild that forgot it would unfold the whole document under them on every save,
/// which in edit mode is every keystroke.
#[test]
fn a_folded_section_is_still_folded_when_the_file_changes_under_the_reader() {
    let fixture = Fixture::new("outline-fold-reload");
    let document = fixture.write("guide.md", &guide());
    let app = axiomd_e2e::launch(&document);

    app.toggle_section("Getting started");
    app.wait_for("the section to fold away", || {
        !app.outline()
            .headings()
            .contains(&"h3 Requirements".to_owned())
    });

    // A section inserted above everything, which moves every source line below it and
    // rebuilds the whole sidebar.
    let grown = guide().replace(
        "# Guide\n\nOpening words.\n\n",
        "# Guide\n\nOpening words.\n\n## Preface\n\nAdded later.\n\n",
    );
    std::fs::write(&document, &grown).expect("save the document");

    app.wait_for("the outline to gain the new section", || {
        app.outline().headings().contains(&"h2 Preface".to_owned())
    });
    assert_eq!(
        app.outline().headings(),
        [
            "h1 Guide",
            "h2 Preface",
            "h2 Getting started",
            "h2 Reference",
            "h2 Notes",
        ],
        "the reload unfolded the section the reader had folded away",
    );
    assert!(
        !app.outline().row("Getting started").expanded,
        "the chevron came back turned down",
    );

    // And unfolding it after the reload still works, so what survived is the fold and
    // not a row that has stopped answering.
    app.toggle_section("Getting started");
    assert!(
        app.outline()
            .headings()
            .contains(&"h3 Requirements".to_owned()),
        "the section could not be unfolded after the document changed",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The non-happy path of the fold: the reader reads on into a section they have folded
/// away. Their place is still shown — on the section that holds it, which is the row
/// they can actually see.
#[test]
fn a_reader_inside_a_folded_section_is_shown_the_section_that_holds_it() {
    let fixture = Fixture::new("outline-fold-place");
    let app = axiomd_e2e::launch(&fixture.write("guide.md", &guide()));

    app.dom("document.querySelector('h3[id=\"requirements\"]').scrollIntoView(true)");
    app.wait_until_section("Requirements");

    app.toggle_section("Getting started");

    app.wait_until_section("Getting started");
    let folded = app.outline();
    assert!(
        !folded.headings().contains(&"h3 Requirements".to_owned()),
        "the section the reader is in was not folded away at all",
    );
    assert_eq!(
        folded
            .rows
            .iter()
            .filter(|row| row.current)
            .map(|row| row.text.clone())
            .collect::<Vec<_>>(),
        ["Getting started"],
        "the reader's place is drawn on no row, or on more than one",
    );

    // Unfolding it hands their place back to the section they are really in.
    app.toggle_section("Getting started");
    app.wait_until_section("Requirements");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The panel reads as a panel (issue #35): over the sections is the document they are
/// the sections of, and how many of them there are.
#[test]
fn the_sidebar_says_which_document_it_is_the_index_of_and_how_many_sections_it_has() {
    let fixture = Fixture::new("outline-titled");
    let app = axiomd_e2e::launch(&fixture.write("guide.md", &guide()));

    let outline = app.outline();
    assert_eq!(outline.title, "guide.md");
    assert_eq!(outline.count, "5 headings");

    // Folding takes rows off the screen and changes nothing about the document, so the
    // count is still the document's.
    app.toggle_section("Getting started");
    assert_eq!(app.outline().count, "5 headings");

    // A document with no sections says whose sidebar it is all the same, and leaves the
    // counting to the empty state beneath it.
    let plain = axiomd_e2e::launch(&fixture.write("plain.md", "Just words.\n"));
    let bare = plain.outline();
    assert_eq!(bare.title, "plain.md");
    assert_eq!(bare.count, "");
    assert_eq!(bare.notice, "No headings");

    assert!(plain.close().is_empty(), "the launch left processes behind");
    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// libadwaita's own default accent colour, which is what a desktop that has never
/// chosen one is drawn in — `--accent-blue`, `#3584e4`
/// (`/usr/share/doc/libadwaita-1/css-variables.html`, libadwaita 1.8.6). Every launch
/// here runs on a settings store of its own with no accent in it, so this is the colour
/// the pill comes out.
const ACCENT: (u8, u8, u8) = (0x35, 0x84, 0xe4);

/// The live position indicator, in pixels (issue #35): the sidebar really draws the
/// reader's place, and it really redraws when they move.
///
/// Pixels rather than the model, because "the row is selected" is not what the reader
/// is promised — being able to see where they are is. A picture taken at the top of the
/// document, one taken inside a section, and one taken back at the top again: the first
/// two must differ, and the third must be the first one again.
#[test]
fn the_sidebar_draws_the_readers_place_and_redraws_it_as_they_read() {
    let fixture = Fixture::new("outline-pill");
    let app = axiomd_e2e::launch(&fixture.write("guide.md", &guide()));
    app.wait_for("the page to say where the reader is", || {
        app.section_reports() > 0
    });
    app.wait_until_section("");

    let above_everything = app.sidebar_screenshot();
    assert!(
        !above_everything.is_blank(),
        "the sidebar was captured as a blank rectangle",
    );
    // Above every section the mark is on the title row and on no section (issue #50):
    // the panel is never markerless, and the mark never claims a section the reader is
    // not in.
    let (_, title_ends) = place_marked_in_accent(&above_everything);
    assert!(
        drawn_as_here(&app).is_empty(),
        "a reader above every section was drawn a pill on {:?}",
        drawn_as_here(&app),
    );

    app.dom("document.querySelector('h2[id=\"reference\"]').scrollIntoView(true)");
    app.wait_until_section("Reference");
    let reading = app.sidebar_screenshot();
    assert!(
        place_marked_in_accent(&reading).0 > title_ends,
        "the mark was still on the title row while the reader was inside a section",
    );
    assert!(
        !reading.looks_like(&above_everything),
        "the sidebar drew the same picture whether or not the reader was in a section",
    );
    // And the pill is the accent pill: `.navigation-sidebar` makes selection neutral by
    // design, so a sidebar drawing the reader's place in grey is one whose stylesheet
    // never took (issue #35). One row of it is some thousands of pixels.
    let pill = reading.pixels_coloured(ACCENT);
    assert!(
        pill > 1_000,
        "the reader's place is drawn in {pill} pixels of the accent colour",
    );

    app.dom("document.scrollingElement.scrollTop = 0");
    app.wait_until_section("");
    assert!(
        app.sidebar_screenshot().looks_like(&above_everything),
        "the sidebar did not go back to the picture it draws above every section",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Issue #50: the sidebar is never markerless. Above the first heading the reader is at
/// the document's title, so the title row (issue #35) is what carries their place —
/// which is true, where marking the first section would be a lie.
///
/// The whole journey in one test, because the defect is a state rather than a moment:
/// the panel a freshly opened document greets the reader with, the mark moving into the
/// first section as they read into it, and the mark coming home when they scroll back
/// up. Read off the pixels as well as off the model, because "the title row is marked"
/// is a claim about what the reader can see.
#[test]
fn the_title_row_carries_the_readers_place_while_they_are_above_every_section() {
    let fixture = Fixture::new("outline-title-place");
    let app = axiomd_e2e::launch(&fixture.write("guide.md", &guide()));
    app.wait_for("the page to say where the reader is", || {
        app.section_reports() > 0
    });
    app.wait_until_section("");

    // A document opens showing its very top, which is the state every first open starts
    // in: something is marked, and it is the title row.
    let outline = app.outline();
    assert!(
        outline.at_the_title,
        "a freshly opened document left the sidebar with nothing marked at all",
    );
    assert!(
        drawn_as_here(&app).is_empty(),
        "the sidebar marked {:?} for a reader who is above every section",
        drawn_as_here(&app),
    );
    assert_eq!(
        outline.section, "",
        "marking the title row changed what the sidebar says the reader's section is",
    );

    let at_the_title = app.sidebar_screenshot();
    let (top, bottom) = place_marked_in_accent(&at_the_title);
    assert!(
        top <= 1,
        "the mark a reader above every section is drawn starts {top} pixels down the \
         panel, which is not the title row at the top of it",
    );
    // The same grammar the sections are marked in (issue #48): the accent is drawn
    // around the row, never as its fill.
    let height = bottom - top + 1;
    let rim = at_the_title.pixels_coloured(ACCENT);
    let inside = at_the_title
        .band(top + 4, height - 8)
        .pixels_coloured(ACCENT);
    assert!(
        inside * 4 < rim,
        "{inside} of the {rim} accent pixels on the title row are inside it rather than \
         around it, so the title row is filled with the accent and not outlined in it",
    );

    // The reader reads on into the first section of the document. The mark moves to it,
    // exactly as it has always moved between sections.
    app.dom("document.querySelector('h1[id=\"guide\"]').scrollIntoView(true)");
    app.wait_until_section("Guide");
    let in_the_first_section = app.sidebar_screenshot();
    assert!(
        !app.outline().at_the_title,
        "the title row kept the reader's place after they read into a section",
    );
    assert_eq!(drawn_as_here(&app), ["Guide"]);
    assert!(
        place_marked_in_accent(&in_the_first_section).0 > bottom,
        "the mark is still drawn on the title row while the reader is in a section",
    );
    // And the title row really was painted as the reader's place, rather than merely
    // said to be: the row through the middle of it is a different row of pixels now.
    let middle = (top + bottom) / 2;
    assert!(
        !at_the_title
            .band(middle, 1)
            .looks_like(&in_the_first_section.band(middle, 1)),
        "the title row is painted the same whether or not it is the reader's place",
    );

    // And back up above it, which is where the reader who scrolls home ends up.
    app.dom("document.scrollingElement.scrollTop = 0");
    app.wait_until_section("");
    assert!(
        app.outline().at_the_title,
        "scrolling back above the first heading left the sidebar markerless",
    );
    assert!(
        app.sidebar_screenshot().looks_like(&at_the_title),
        "the sidebar did not go back to the picture it draws above every section",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A document with no headings at all: there is no section to be in and never will be,
/// so the title row is the reader's place for as long as the document is open.
#[test]
fn a_document_with_no_headings_marks_its_title_row() {
    let fixture = Fixture::new("outline-title-no-headings");
    let app = axiomd_e2e::launch(&fixture.write("plain.md", "Just words.\n\n- and a list\n"));

    let outline = app.outline();
    assert!(outline.shown, "the sidebar went away instead of saying so");
    assert!(outline.headings().is_empty(), "{:?}", outline.headings());
    assert_eq!(outline.notice, "No headings");
    assert!(
        outline.at_the_title,
        "a document with no headings left the sidebar with nothing marked at all",
    );

    let panel = app.sidebar_screenshot();
    let (top, _) = place_marked_in_accent(&panel);
    assert!(
        top <= 1,
        "the mark is {top} pixels down a panel whose only row is its title row",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The sections the sidebar is drawing as the reader's place — none, one, or (which
/// would be a bug of its own) several.
fn drawn_as_here(app: &App) -> Vec<String> {
    app.outline()
        .rows
        .iter()
        .filter(|row| row.current)
        .map(|row| row.text.clone())
        .collect()
}

/// Issue #42: the reader's place is not something the pointer can move.
///
/// The sidebar draws two different things — where the reader is, and what is under
/// their hand — and until this was fixed it drew them the same way and could only draw
/// one of them at a time: the list is built with single-click activation, GTK moves the
/// selection on to whatever row the pointer crosses, and the accent pill rode the
/// selection. So running the pointer down the sidebar took the pill with it, and the
/// section the reader was actually reading lost it.
#[test]
fn running_the_pointer_over_the_sidebar_never_moves_the_readers_place() {
    let fixture = Fixture::new("outline-hover-place");
    let app = axiomd_e2e::launch(&fixture.write("guide.md", &guide()));
    app.dom("document.querySelector('h2[id=\"reference\"]').scrollIntoView(true)");
    app.wait_until_section("Reference");

    let resting = app.sidebar_screenshot();
    let pill = resting.pixels_coloured(ACCENT);
    assert!(pill > 1_000, "the reader's place was not drawn at all");

    // The pointer wanders over every other section in turn. Each of them is a row the
    // reader could click, and none of them is where they are.
    for section in ["Guide", "Getting started", "Requirements", "Notes"] {
        app.hover_over(section);

        assert_eq!(
            app.outline().section,
            "Reference",
            "the pointer passing over {section:?} moved the reader's place",
        );
        assert_eq!(
            drawn_as_here(&app),
            ["Reference"],
            "with the pointer over {section:?}, the sidebar draws the wrong rows as \
             the reader's place",
        );
    }

    // And in pixels, which is the only place the difference between the two marks is
    // real: the accent is exactly the accent that was there, so hovering neither took
    // the pill away nor drew a second one.
    let hovered = app.sidebar_screenshot();
    assert_eq!(
        hovered.pixels_coloured(ACCENT),
        pill,
        "the pointer changed how much of the sidebar is drawn in the accent colour",
    );
    // Hover is still feedback — the sidebar does not look untouched.
    assert!(
        !hovered.looks_like(&resting),
        "the pointer over a row drew nothing at all, so nothing says it is clickable",
    );

    // The whole of the ruling, in one picture: what the pointer does to a row must not
    // be what reading that row looks like. Same row, same panel, two different answers.
    app.hover_away();
    app.dom("document.querySelector('h2[id=\"notes\"]').scrollIntoView(true)");
    app.wait_until_section("Notes");
    let reading_it = app.sidebar_screenshot();
    assert!(
        !hovered.looks_like(&reading_it),
        "the pointer over Notes drew the same panel as reading Notes does",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The other half: hovering is not a thing that happens to the document. The window's
/// answer about where the reader is comes back unchanged, and the page was never asked.
///
/// The non-happy paths are the two the pointer really takes: on to the very row that is
/// already the reader's place, and off the list altogether. Neither may leave a mark
/// behind — the pill must not be dropped when the pointer arrives on it, and must not be
/// left on the last row the pointer touched when it goes.
#[test]
fn hovering_changes_nothing_the_window_says_about_where_the_reader_is() {
    let fixture = Fixture::new("outline-hover-tracking");
    let app = axiomd_e2e::launch(&fixture.write("guide.md", &guide()));
    app.dom("document.querySelector('h2[id=\"reference\"]').scrollIntoView(true)");
    app.wait_until_section("Reference");
    let reports = app.section_reports();
    let resting = app.sidebar_screenshot();

    // On to the row the reader is on.
    app.hover_over("Reference");
    assert_eq!(app.outline().section, "Reference");
    assert_eq!(drawn_as_here(&app), ["Reference"]);

    // On to another one, and then off the sidebar entirely.
    app.hover_over("Notes");
    app.hover_away();
    assert_eq!(
        app.outline().section,
        "Reference",
        "the pointer left the sidebar and took the reader's place with it",
    );
    assert_eq!(drawn_as_here(&app), ["Reference"]);
    assert!(
        app.sidebar_screenshot().looks_like(&resting),
        "the pointer left a mark on the sidebar after it had gone",
    );

    // And none of it was the document's business: the page said nothing, because
    // nothing about where the reader is changed.
    assert_eq!(
        app.section_reports(),
        reports,
        "moving the pointer over the sidebar made the page report the reader's place",
    );
    assert_eq!(app.navigation_count(), 1);

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The non-happy path of the pill itself: a reader above the first heading is in no
/// section, and running the pointer over the sidebar must not invent one for them.
///
/// Their place is the title row while they are up there (issue #50), so what the
/// pointer may not do is move it off the title row on to a section — measured in the
/// pixels of the panel as well as in the rows it says are marked.
#[test]
fn hovering_above_every_section_draws_the_reader_no_place_at_all() {
    let fixture = Fixture::new("outline-hover-nowhere");
    let app = axiomd_e2e::launch(&fixture.write("guide.md", &guide()));
    app.wait_for("the page to say where the reader is", || {
        app.section_reports() > 0
    });
    app.wait_until_section("");
    let at_the_title = app.sidebar_screenshot();
    let (_, title_ends) = place_marked_in_accent(&at_the_title);
    let marked = at_the_title.pixels_coloured(ACCENT);

    for section in ["Getting started", "Reference"] {
        app.hover_over(section);

        assert_eq!(
            app.outline().section,
            "",
            "the pointer over {section:?} put the reader in a section they are not in",
        );
        assert!(
            drawn_as_here(&app).is_empty(),
            "the sidebar drew {:?} as the reader's place while they are above every \
             section",
            drawn_as_here(&app),
        );
        assert!(
            app.outline().at_the_title,
            "the pointer over {section:?} took the reader's place off the title row",
        );
        let hovered = app.sidebar_screenshot();
        assert_eq!(
            hovered.pixels_coloured(ACCENT),
            marked,
            "the pointer over {section:?} changed how much of the sidebar is drawn in \
             the accent colour",
        );
        assert_eq!(
            place_marked_in_accent(&hovered).1,
            title_ends,
            "the pointer over {section:?} drew an accent pill somewhere down the list \
             for a reader who is in no section",
        );
    }

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// What must still work after all that: one click on a section still goes there
/// (the ruling of the #7 era stands), and it leaves exactly one mark behind — on the
/// section that was reached, and nowhere the pointer merely passed over on the way.
#[test]
fn a_single_click_still_takes_the_reader_to_a_section_the_pointer_crossed_to_reach() {
    let fixture = Fixture::new("outline-hover-click");
    let app = axiomd_e2e::launch(&fixture.write("guide.md", &guide()));
    assert_eq!(scroll_offset(&app), 0);

    // The pointer travels down the sidebar to get to the row it is going to click.
    app.hover_over("Getting started");
    app.hover_over("Requirements");
    app.hover_over("Reference");
    app.press("Reference");

    app.wait_for("the document to arrive at the section", || {
        screen_position_of(&app, "Reference").abs() <= 4
    });
    app.wait_until_section("Reference");
    assert_eq!(
        drawn_as_here(&app),
        ["Reference"],
        "the click left the reader's place on the wrong rows",
    );
    assert_eq!(
        app.navigation_count(),
        1,
        "the section was reached by loading the page again",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// And the keyboard, which browses the sidebar the same way the pointer does and must
/// be told apart from reading the same way: walking the cursor down the rows moves no
/// pill, and activating one still goes there.
#[test]
fn the_keyboard_walks_the_sidebar_without_moving_the_place_and_still_picks_a_section() {
    let fixture = Fixture::new("outline-keyboard");
    let app = axiomd_e2e::launch(&fixture.write("guide.md", &guide()));
    app.dom("document.querySelector('h2[id=\"reference\"]').scrollIntoView(true)");
    app.wait_until_section("Reference");
    let resting = app.sidebar_screenshot();
    let pill = resting.pixels_coloured(ACCENT);

    for section in ["Guide", "Getting started", "Notes"] {
        app.key_to_section(section);

        assert_eq!(
            app.outline().section,
            "Reference",
            "the keyboard cursor on {section:?} moved the reader's place",
        );
        assert_eq!(drawn_as_here(&app), ["Reference"]);
        // And the accent is exactly the accent that was there: the cursor's own mark is
        // the focus ring, which is a third thing rather than the pill on loan.
        assert_eq!(
            app.sidebar_screenshot().pixels_coloured(ACCENT),
            pill,
            "the keyboard cursor on {section:?} changed how much of the sidebar is \
             drawn in the accent colour",
        );
    }
    // The reader has to be able to see where the cursor is, or the keyboard is walking
    // an outline in the dark.
    assert!(
        !app.sidebar_screenshot().looks_like(&resting),
        "the keyboard cursor is drawn nowhere at all",
    );

    // `Enter` on the row the cursor is on: `list.activate-item`, which is the action
    // GTK binds that key to (GTK 4.20, `GtkListBase`).
    app.press("Notes");

    app.wait_for("the document to arrive at the section", || {
        screen_position_of(&app, "Notes").abs() <= 4
    });
    app.wait_until_section("Notes");
    assert_eq!(
        drawn_as_here(&app),
        ["Notes"],
        "activating a section from the keyboard left the place on the wrong rows",
    );
    assert_eq!(app.navigation_count(), 1);

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A strip of the panel the sections are never written across: past the end of the
/// longest of them and short of the pill's own right-hand edge, so a luminance read
/// across it is a reading of the paint and never of a letter that happened to be there.
const PLAIN: (u32, u32) = (190, 225);

/// The pill the accent is drawn on, as the first and last row of the picture it appears
/// on — found the way a reader finds it, by looking for the blue.
fn place_marked_in_accent(sidebar: &Screenshot) -> (u32, u32) {
    let (_, height) = sidebar.size();
    let rows: Vec<u32> = (0..height)
        .filter(|row| sidebar.band(*row, 1).pixels_coloured(ACCENT) > 0)
        .collect();
    match (rows.first(), rows.last()) {
        (Some(top), Some(bottom)) => (*top, *bottom),
        _ => panic!("the accent is drawn nowhere on the sidebar"),
    }
}

/// How far the fill of the pill whose top row is `top` is from the panel's plain
/// background, in channel steps of luminance.
///
/// Luminance rather than colour, because that is the whole question two of these tests
/// ask: a difference this measures is one a reader who cannot tell the accent from the
/// wash — or is looking at a screen that renders neither in colour — can still see. It
/// is read both ways round because a wash of the foreground darkens a row on a light
/// desktop and lightens one on a dark desktop, and only one of the two readings is
/// above zero on either.
fn wash_depth(sidebar: &Screenshot, top: u32, height: u32) -> u8 {
    // Down and out of the pill: the row the profile settles on is the margin below it,
    // so this reads how much darker the pill is than the panel.
    let darker = sidebar.shading_below(top + height - 11, PLAIN)[0];
    // Down and into it: the profile settles inside the pill instead, so this reads how
    // much darker the panel is than the pill.
    let lighter = sidebar.shading_below(top - 8, PLAIN)[0];
    darker.max(lighter)
}

/// How hard an edge the pill whose top row is `top` has, in the same channel steps: the
/// step between its outline and its own fill, which is nothing at all on a pill that has
/// no outline.
fn rim_step(sidebar: &Screenshot, top: u32, height: u32) -> u8 {
    // The top edge against the fill below it, and the bottom edge against the fill above
    // it — again both ways round, because the accent is darker than the fill on a light
    // desktop and lighter than it on a dark one.
    let darker = sidebar.shading_below(top, PLAIN)[0];
    let lighter = sidebar.shading_below(top + height - 14, PLAIN)[0];
    darker.max(lighter)
}

/// The height of one pill and the row it sits in, taken from the pill the accent marks:
/// libadwaita leaves 2px under a `.navigation-sidebar` row (`margin: 0 6px 2px`,
/// libadwaita 1.8.6) and this panel does not take that back.
fn pitch(height: u32) -> u32 {
    height + 2
}

/// Issue #48: the reader's place is a quiet grey pill with the accent around it, and
/// never a pill of accent.
///
/// "Use the accent colour" meant use it *as an accent*; the sidebar had been reading it
/// as a fill, and painted the section being read in a solid blue lozenge with white
/// text. `.navigation-sidebar` says "this one" in washes of its own foreground and
/// spends colour on an outline, and that is the language this panel speaks now.
///
/// Everything asserted here is read off the pixels rather than off a stylesheet: how
/// much of the panel is drawn in the accent, where in the pill it is, and how far the
/// fill is from the panel behind it. A rule that never resolved leaves the surface
/// looking almost right and answers every one of these with the wrong number.
fn the_place_is_a_grey_pill_outlined_in_the_accent(name: &str, desktop: &Preferences) {
    let fixture = Fixture::new(name);
    let app = axiomd_e2e::launch_with(&fixture.write("guide.md", &guide()), desktop);
    app.dom("document.querySelector('h2[id=\"reference\"]').scrollIntoView(true)");
    app.wait_until_section("Reference");

    let resting = app.sidebar_screenshot();
    let (top, bottom) = place_marked_in_accent(&resting);
    let height = bottom - top + 1;

    // The accent is around the pill, not in it. Everything inside the outline — the pill
    // less a generous four pixels of rim on each side — is drawn in something else, so
    // what the row is *filled* with cannot be the accent whatever else it may be.
    let inside = resting.band(top + 4, height - 8);
    let rim = resting.pixels_coloured(ACCENT);
    assert!(
        inside.pixels_coloured(ACCENT) * 4 < rim,
        "{} of the {rim} accent pixels are inside the pill rather than around it, so the \
         reader's place is filled with the accent and not outlined in it",
        inside.pixels_coloured(ACCENT),
    );

    // And it is an edge, not a tint: the outline steps away from the fill it surrounds by
    // far more than the eight channel steps this harness calls a change at all.
    let edge = rim_step(&resting, top, height);
    assert!(
        edge >= 40,
        "the reader's place is outlined by a step of {edge} channel steps, which is not \
         an outline a reader can see",
    );

    // The fill itself is a wash of the panel's own foreground, deep enough to read as a
    // marked row on its own.
    let here = wash_depth(&resting, top, height);
    assert!(
        here >= 24,
        "the reader's place is filled with a wash {here} channel steps from the plain \
         panel, which is not a pill",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The hierarchy the marker exists inside, in luminance: what the pointer does to a row
/// has to be visibly less than what reading it does, and hovering the row the reader is
/// already on has to leave it still the row they are on.
///
/// The pointer is put on "Getting started", two rows above the section being read, so
/// that neither mark is measured across the other's edge.
fn hover_is_visibly_less_than_being_read(name: &str, desktop: &Preferences) {
    let fixture = Fixture::new(name);
    let app = axiomd_e2e::launch_with(&fixture.write("guide.md", &guide()), desktop);
    app.dom("document.querySelector('h2[id=\"reference\"]').scrollIntoView(true)");
    app.wait_until_section("Reference");

    let resting = app.sidebar_screenshot();
    let (top, bottom) = place_marked_in_accent(&resting);
    let height = bottom - top + 1;
    let elsewhere = top - 2 * pitch(height);
    let here = wash_depth(&resting, top, height);
    assert_eq!(
        wash_depth(&resting, elsewhere, height),
        0,
        "a row nobody is reading and nothing is pointing at is painted anyway",
    );

    app.hover_over("Getting started");
    let hovered = app.sidebar_screenshot();

    // Hover is drawn, and drawn as a wash of its own with no edge to it.
    let under_the_pointer = wash_depth(&hovered, elsewhere, height);
    assert!(
        under_the_pointer >= 8,
        "the pointer over a row washed it by {under_the_pointer} channel steps, which is \
         inside this harness's own tolerance — nothing says the row is clickable",
    );
    assert_eq!(
        rim_step(&hovered, elsewhere, height),
        0,
        "the pointer drew an outline round a row, which is the mark that means the \
         reader is reading it",
    );

    // And it is unmistakably the lesser of the two: the reader's place is washed at
    // least twice as deep, and still carries the outline the hovered row has none of.
    assert!(
        here >= 2 * under_the_pointer,
        "the reader's place is washed {here} channel steps deep and a row under the \
         pointer {under_the_pointer}, which is not a difference told apart at a glance",
    );
    assert_eq!(
        wash_depth(&hovered, top, height),
        here,
        "the pointer somewhere else changed how the reader's place is painted",
    );

    // The pointer arriving on the very row the reader is on: the wash deepens, and every
    // part of the marker that says "you are here" is still there.
    app.hover_over("Reference");
    let on_it = app.sidebar_screenshot();
    let pressed = wash_depth(&on_it, top, height);
    assert!(
        pressed > here,
        "the pointer on the reader's own row answered it with nothing: {pressed} channel \
         steps against {here} at rest",
    );
    assert!(
        rim_step(&on_it, top, height) >= 40,
        "the pointer on the reader's own row took its outline away",
    );
    assert_eq!(
        on_it.pixels_coloured(ACCENT),
        resting.pixels_coloured(ACCENT),
        "the pointer on the reader's own row changed how much of the panel is accent",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

#[test]
fn a_light_sidebar_draws_the_place_in_grey_outlined_in_the_accent() {
    let desktop = Preferences::new("outline-paint-light");
    the_place_is_a_grey_pill_outlined_in_the_accent("outline-paint-light", &desktop);
}

#[test]
fn a_dark_sidebar_draws_the_place_in_grey_outlined_in_the_accent() {
    let desktop = Preferences::with("outline-paint-dark", "theme", "'dark'");
    the_place_is_a_grey_pill_outlined_in_the_accent("outline-paint-dark", &desktop);
}

/// The desktop the marker may not be a colour on: high contrast, where a reader who
/// cannot tell the accent from the wash still has to be able to see which row is theirs.
/// Both of the differences asserted above are luminance — the outline's step and the
/// depth of the wash — so passing here is passing without colour.
#[test]
fn a_high_contrast_sidebar_draws_the_place_without_relying_on_colour() {
    let desktop = Preferences::new("outline-paint-contrast");
    desktop.set_high_contrast(true);
    the_place_is_a_grey_pill_outlined_in_the_accent("outline-paint-contrast", &desktop);
}

#[test]
fn a_light_sidebar_tells_the_pointer_apart_from_the_reader() {
    let desktop = Preferences::new("outline-ladder-light");
    hover_is_visibly_less_than_being_read("outline-ladder-light", &desktop);
}

#[test]
fn a_dark_sidebar_tells_the_pointer_apart_from_the_reader() {
    let desktop = Preferences::with("outline-ladder-dark", "theme", "'dark'");
    hover_is_visibly_less_than_being_read("outline-ladder-dark", &desktop);
}

#[test]
fn a_high_contrast_sidebar_tells_the_pointer_apart_without_relying_on_colour() {
    let desktop = Preferences::new("outline-ladder-contrast");
    desktop.set_high_contrast(true);
    hover_is_visibly_less_than_being_read("outline-ladder-contrast", &desktop);
}

/// The visual specification of the sidebar on a light desktop (issue #35).
///
/// Ignored until the owner has looked at the picture and pinned it: they are the arbiter
/// of "beautiful", and approving a rendered surface for the first time is theirs to do
/// rather than the harness's (`docs/TESTING.md`). To pin it, look at
/// `target/debug/e2e-artifacts/outline-light.actual.png` from a failing run and, if it
/// is right, re-run this test with `AXIOMD_PIN_GOLDENS=1` set, then remove the
/// `#[ignore]`.
#[test]
#[ignore = "awaiting the owner's first visual approval; see the comment above"]
fn a_light_sidebar_still_looks_the_way_it_was_approved() {
    let fixture = Fixture::new("outline-golden-light");
    let app = axiomd_e2e::launch(&fixture.write("guide.md", &guide()));
    app.dom("document.querySelector('h2[id=\"reference\"]').scrollIntoView(true)");
    app.wait_until_section("Reference");

    app.sidebar_screenshot().assert_matches("outline-light");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The same panel on a dark desktop. Pinned the same way, from
/// `outline-dark.actual.png`.
#[test]
#[ignore = "awaiting the owner's first visual approval; see the comment above"]
fn a_dark_sidebar_still_looks_the_way_it_was_approved() {
    let fixture = Fixture::new("outline-golden-dark");
    let dark = Preferences::with("outline-golden-dark", "theme", "'dark'");
    let app = axiomd_e2e::launch_with(&fixture.write("guide.md", &guide()), &dark);
    app.dom("document.querySelector('h2[id=\"reference\"]').scrollIntoView(true)");
    app.wait_until_section("Reference");

    app.sidebar_screenshot().assert_matches("outline-dark");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// And on a desktop asking for high contrast, which is the one no colour value can show
/// is legible rather than merely different. Pinned from
/// `outline-high-contrast.actual.png`.
#[test]
#[ignore = "awaiting the owner's first visual approval; see the comment above"]
fn a_high_contrast_sidebar_still_looks_the_way_it_was_approved() {
    let fixture = Fixture::new("outline-golden-contrast");
    let desktop = Preferences::new("outline-golden-contrast");
    desktop.set_high_contrast(true);
    let app = axiomd_e2e::launch_with(&fixture.write("guide.md", &guide()), &desktop);
    app.dom("document.querySelector('h2[id=\"reference\"]').scrollIntoView(true)");
    app.wait_until_section("Reference");

    app.sidebar_screenshot()
        .assert_matches("outline-high-contrast");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The picture a freshly opened document greets the reader with (issue #50): the top of
/// the document, so the title row is the row carrying their place. Pinned the same way
/// as every other golden here, from `outline-title-light.actual.png`.
#[test]
#[ignore = "awaiting the owner's first visual approval; see the comment above"]
fn a_sidebar_at_the_top_of_a_light_document_still_looks_the_way_it_was_approved() {
    let fixture = Fixture::new("outline-golden-title-light");
    let app = axiomd_e2e::launch(&fixture.write("guide.md", &guide()));
    app.wait_for("the page to say where the reader is", || {
        app.section_reports() > 0
    });
    app.wait_until_section("");

    app.sidebar_screenshot()
        .assert_matches("outline-title-light");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The same panel on a dark desktop. Pinned from `outline-title-dark.actual.png`.
#[test]
#[ignore = "awaiting the owner's first visual approval; see the comment above"]
fn a_sidebar_at_the_top_of_a_dark_document_still_looks_the_way_it_was_approved() {
    let fixture = Fixture::new("outline-golden-title-dark");
    let dark = Preferences::with("outline-golden-title-dark", "theme", "'dark'");
    let app = axiomd_e2e::launch_with(&fixture.write("guide.md", &guide()), &dark);
    app.wait_for("the page to say where the reader is", || {
        app.section_reports() > 0
    });
    app.wait_until_section("");

    app.sidebar_screenshot()
        .assert_matches("outline-title-dark");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The two pictures issue #42 is about, on a light desktop: the pointer over a row that
/// is not the reader's place, and the pointer over the row that is. Beside the pinned
/// picture of the panel at rest above, they are what "hover is unmistakably different
/// from the current chapter" means — the owner arbitrates that, once, by looking.
///
/// Pinned the same way as every other golden here, from
/// `outline-hover-light.actual.png` and `outline-hover-current-light.actual.png`.
#[test]
#[ignore = "awaiting the owner's first visual approval; see the comment above"]
fn a_hovered_light_sidebar_still_looks_the_way_it_was_approved() {
    let fixture = Fixture::new("outline-golden-hover-light");
    let app = axiomd_e2e::launch(&fixture.write("guide.md", &guide()));
    app.dom("document.querySelector('h2[id=\"reference\"]').scrollIntoView(true)");
    app.wait_until_section("Reference");

    app.hover_over("Notes");
    app.sidebar_screenshot()
        .assert_matches("outline-hover-light");

    app.hover_over("Reference");
    app.sidebar_screenshot()
        .assert_matches("outline-hover-current-light");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The same two, on a dark desktop.
#[test]
#[ignore = "awaiting the owner's first visual approval; see the comment above"]
fn a_hovered_dark_sidebar_still_looks_the_way_it_was_approved() {
    let fixture = Fixture::new("outline-golden-hover-dark");
    let dark = Preferences::with("outline-golden-hover-dark", "theme", "'dark'");
    let app = axiomd_e2e::launch_with(&fixture.write("guide.md", &guide()), &dark);
    app.dom("document.querySelector('h2[id=\"reference\"]').scrollIntoView(true)");
    app.wait_until_section("Reference");

    app.hover_over("Notes");
    app.sidebar_screenshot()
        .assert_matches("outline-hover-dark");

    app.hover_over("Reference");
    app.sidebar_screenshot()
        .assert_matches("outline-hover-current-dark");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// And on a desktop asking for high contrast, where the two marks have the least room
/// to be told apart by colour: the pill keeps a shape of its own and the wash comes up
/// to where it can be seen (`outline.css`).
#[test]
#[ignore = "awaiting the owner's first visual approval; see the comment above"]
fn a_hovered_high_contrast_sidebar_still_looks_the_way_it_was_approved() {
    let fixture = Fixture::new("outline-golden-hover-contrast");
    let desktop = Preferences::new("outline-golden-hover-contrast");
    desktop.set_high_contrast(true);
    let app = axiomd_e2e::launch_with(&fixture.write("guide.md", &guide()), &desktop);
    app.dom("document.querySelector('h2[id=\"reference\"]').scrollIntoView(true)");
    app.wait_until_section("Reference");

    app.hover_over("Notes");
    app.sidebar_screenshot()
        .assert_matches("outline-hover-high-contrast");

    app.hover_over("Reference");
    app.sidebar_screenshot()
        .assert_matches("outline-hover-current-high-contrast");

    assert!(app.close().is_empty(), "the launch left processes behind");
}
