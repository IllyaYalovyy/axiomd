//! Issue #51: a document opens where the reader left it.
//!
//! Every test here drives the shipped binary on a headless compositor twice over one
//! store — which is the reader closing a document today and opening it tomorrow — and
//! asserts the one thing they would see: the block they were reading is at the top of
//! the page, with no moment in which the top of the document was.
//!
//! What is *not* asserted here is the store itself. What it holds, how it is written
//! and what stops it growing are unit-tested where that policy lives
//! (`crates/axiomd-app/src/places.rs`); these are about the reader.

use std::path::Path;

use axiomd_e2e::{App, Fixture, Preferences, Screenshot};

/// The colour the bundled stylesheet paints a page in on a light desktop — the number
/// `arrival.rs` holds the frames of a document arriving to, and the same one here.
const PAGE_LIGHT: (u8, u8, u8) = (255, 255, 255);

/// How many sections the test document has, and how many paragraphs are under each.
///
/// Enough of both that the section picked below is well down the document and has
/// plenty of document under it — a section a window's own height from the end cannot
/// be brought to the top of the page at all, and a test asserting that it was would be
/// asserting the browser's scroll limit.
const SECTIONS: usize = 9;
const PARAGRAPHS: usize = 12;

/// The section the reader is left in, and the one they must come back to.
const LEFT_IN: &str = "Chapter 4";

/// A document with more sections than a window can hold, each longer than a window.
fn chapters() -> String {
    let mut source = String::from("# Reading Position\n\nThe opening words, at the top.\n\n");
    for chapter in 1..=SECTIONS {
        source.push_str(&format!("## Chapter {chapter}\n\n"));
        for paragraph in 1..=PARAGRAPHS {
            source.push_str(&format!(
                "Chapter {chapter}, paragraph {paragraph}: a line of prose long enough \
                 to take up room on the page it is read on.\n\n"
            ));
        }
    }
    source
}

/// Where the heading reading `text` sits on screen, rounded to whole pixels — the same
/// question `outline.rs` asks, and the only one that says where the reader is.
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

/// Fails unless `frame` is the blank page a document arrives on, whatever else the
/// window is doing — the assertion `arrival.rs` holds every pre-document frame to, and
/// here the proof that no frame before the swap has any of the document in it.
///
/// Ninety-nine hundredths rather than all of it: the pane is drawn on a compositor and
/// its very edge belongs to the widgets around it.
fn assert_is_the_blank_page(frame: &Screenshot, what: &str) {
    let (width, height) = frame.size();
    let pixels = usize::try_from(width * height).expect("a pane of pixels");
    assert!(pixels > 0, "{what}: the pane has no pixels at all");
    let page = frame.pixels_coloured(PAGE_LIGHT);
    assert!(
        page * 100 >= pixels * 99,
        "{what}: only {page} of {pixels} pixels are the page {PAGE_LIGHT:?}",
    );
}

fn scroll_offset(app: &App) -> i32 {
    app.dom("Math.round(document.scrollingElement.scrollTop)")
        .parse()
        .expect("a scroll offset")
}

/// Reads `document` to `section` and closes the window on it, leaving the reader's
/// place wherever the application decided to leave it.
///
/// The waits are the point: the window is gone before the launch is ended, so what the
/// next launch finds is what a *closed window* wrote and never what a dying process
/// happened to flush.
fn read_to_and_close(document: &Path, preferences: &Preferences, section: &str) {
    let app = axiomd_e2e::launch_with(document, preferences);
    assert_eq!(
        scroll_offset(&app),
        0,
        "a document nobody had read yet did not open at the top",
    );

    app.press(section);
    app.wait_for("the document to arrive at the section", || {
        screen_position_of(&app, section).abs() <= 4
    });
    app.wait_until_section(section);

    app.close_window();
    app.wait_until_windows(0);
    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The whole of the feature, as the reader meets it: they leave a document half way
/// through, come back to it, and are where they were.
///
/// And the sidebar with them (issue #7 and #50's at-the-top case, the other way
/// round): a reader restored into the middle of a document is shown the section they
/// are in, not the top of the outline.
#[test]
fn a_document_opens_where_the_reader_left_it() {
    let fixture = Fixture::new("position-reopen");
    let preferences = Preferences::new("position-reopen");
    let notes = fixture.write("notes.md", &chapters());

    read_to_and_close(&notes, &preferences, LEFT_IN);

    let again = axiomd_e2e::launch_with(&notes, &preferences);
    assert!(
        screen_position_of(&again, LEFT_IN).abs() <= 4,
        "the reader came back to a document and {LEFT_IN} was {}px from the top of it",
        screen_position_of(&again, LEFT_IN),
    );
    assert!(
        scroll_offset(&again) > 0,
        "the document opened at the top after all",
    );
    // The page was patched into place, not navigated to: restoring a place is not a
    // second load of the document (design_decisions.md).
    assert_eq!(
        again.navigation_count(),
        1,
        "the reader's place was reached by loading the page again",
    );
    again.wait_until_section(LEFT_IN);

    assert!(again.close().is_empty(), "the launch left processes behind");
}

/// The half of it the reader would notice most if it were wrong: at no point is the
/// top of a document they left in the middle on screen (issue #41's frame assertions,
/// carried into the open path).
///
/// The document's own bytes are held back at the origin that serves them and released
/// mid-loop, which is what makes the moment worth watching last longer than a frame.
/// Until the page says it has been drawn the reader is looking at the surface in front
/// of it and not at the document at all; the first frame in which they are looking at
/// the document has to be a frame of the document where they left it.
#[test]
fn the_reader_is_never_shown_the_top_of_a_document_they_left_in_the_middle() {
    let fixture = Fixture::new("position-no-jump");
    let preferences = Preferences::new("position-no-jump");
    let notes = fixture.write("notes.md", &chapters());

    read_to_and_close(&notes, &preferences, LEFT_IN);

    let again = axiomd_e2e::launch_without_document_with(&preferences);
    again.open_to_the_empty_pane(&notes);
    assert_eq!(
        again.pane_showing(),
        "placeholder",
        "the webview was on screen before it had a frame of the document to show",
    );
    again.answer_the_held_pages();

    // Every frame up to the swap, photographed as the compositor is handed it: the
    // blank page, with nothing of the document in it — so there is no frame in this
    // half of the sequence in which the reader could have been shown its top.
    let frames = std::cell::Cell::new(0u32);
    again.wait_for("the document to be in the pane", || {
        if again.pane_showing() == "document" {
            return true;
        }
        assert_is_the_blank_page(
            &again.presented_pane(),
            &format!("frame {} of a document being opened again", frames.get()),
        );
        frames.set(frames.get() + 1);
        false
    });
    // The very first frame the reader can see the document in, and every frame after
    // it: the block they left is at the top and the top of the document is not.
    for frame in 0..12 {
        assert!(
            scroll_offset(&again) > 0,
            "frame {frame} after the document was presented showed its top",
        );
        assert!(
            screen_position_of(&again, LEFT_IN).abs() <= 4,
            "frame {frame} after the document was presented was somewhere else",
        );
    }
    assert!(
        frames.get() > 1,
        "only {} frame was watched, which is not a sequence",
        frames.get(),
    );

    assert!(again.close().is_empty(), "the launch left processes behind");
}

/// The document changed between visits and the anchored line went with it: the reader
/// lands on the nearest surviving block, silently.
///
/// The whole of the first half of the document is cut away, so the line written down
/// names a block the document no longer has and every line after it means something
/// else. There is nothing to say about that — a place is a best effort and never a
/// question (VISION principle 6) — so what is asserted is that they are inside the
/// document, are not at the top of it, and were told nothing.
#[test]
fn an_edit_that_took_the_anchor_away_lands_the_reader_on_a_surviving_block() {
    let fixture = Fixture::new("position-edited");
    let preferences = Preferences::new("position-edited");
    let notes = fixture.write("notes.md", &chapters());

    read_to_and_close(&notes, &preferences, LEFT_IN);

    // What the reader did to the document between visits: cut everything above the
    // last two chapters, which takes the anchored line away entirely.
    let source = chapters();
    let shortened = source
        .split_once("## Chapter 8")
        .map(|(_, rest)| format!("# Reading Position\n\n## Chapter 8{rest}"))
        .expect("a document with a chapter 8 in it");
    std::fs::write(&notes, &shortened).expect("write the shortened document");

    let again = axiomd_e2e::launch_with(&notes, &preferences);
    assert_eq!(
        again.dom_text("h1"),
        "Reading Position",
        "the shortened document is not the one on screen",
    );
    assert!(
        scroll_offset(&again) > 0,
        "a reader who left a document in the middle was put back at the top of the \
         edit that took their place away",
    );
    // Inside the document rather than past the end of it: the clamp landed on a block
    // that is still there.
    assert_eq!(
        again.dom(
            "String(document.scrollingElement.scrollTop <= \
             document.scrollingElement.scrollHeight - document.scrollingElement.clientHeight + 1)"
        ),
        "true",
        "the reader was put somewhere the document does not go",
    );
    assert_eq!(
        again.banner(),
        "",
        "the reader was told something about their reading position",
    );
    assert_eq!(
        again.visible_dialog(),
        "",
        "a document was opened with a question over it",
    );

    assert!(again.close().is_empty(), "the launch left processes behind");
}

/// The other non-happy path: a store that is not there any more, and one whose bytes
/// are not what axiomd wrote. Both open the document at the top and say nothing at all.
#[test]
fn a_store_that_is_gone_or_damaged_opens_the_document_at_the_top_and_says_nothing() {
    let fixture = Fixture::new("position-damaged");
    let preferences = Preferences::new("position-damaged");
    let notes = fixture.write("notes.md", &chapters());

    read_to_and_close(&notes, &preferences, LEFT_IN);
    let store = preferences.reading_positions();
    assert!(
        store.exists(),
        "nothing was written down, so this test would prove nothing",
    );

    for damage in ["", "\u{0}not a place at all\nnor is this one\n"] {
        match damage.is_empty() {
            true => std::fs::remove_file(&store).expect("take the store away"),
            false => std::fs::write(&store, damage).expect("damage the store"),
        }

        let again = axiomd_e2e::launch_with(&notes, &preferences);
        assert_eq!(
            scroll_offset(&again),
            0,
            "a reader whose store is unreadable did not simply open at the top",
        );
        assert_eq!(again.dom_text("h1"), "Reading Position");
        assert_eq!(again.banner(), "", "the reader was told about their store");
        assert_eq!(
            again.visible_dialog(),
            "",
            "a damaged store put a question in front of the reader",
        );
        assert!(again.close().is_empty(), "the launch left processes behind");
    }
}

/// The preference (invariant 14): off stops both halves, on again resumes, and turning
/// it reaches the very next document the reader opens rather than the next launch.
///
/// Two documents, because a window handed the document it already holds simply keeps
/// it: what the reader opens after turning the switch has to be a document they are
/// not already in.
#[test]
fn turning_the_preference_off_stops_remembering_and_restoring_and_on_resumes_it() {
    let fixture = Fixture::new("position-preference");
    let preferences = Preferences::new("position-preference");
    let notes = fixture.write("notes.md", &chapters());
    let diary = fixture.write("diary.md", &chapters());

    read_to_and_close(&notes, &preferences, LEFT_IN);
    read_to_and_close(&diary, &preferences, LEFT_IN);

    // The reader turns it off while a restored document is in front of them, and the
    // very next document they open is at the top: nothing was restarted or reopened
    // to apply it.
    let off = axiomd_e2e::launch_with(&notes, &preferences);
    assert!(
        screen_position_of(&off, LEFT_IN).abs() <= 4,
        "the launch this test turns the preference off in did not restore anything",
    );
    off.activate("app.preferences");
    assert_eq!(off.preference("Remember Reading Position"), "true");
    off.set_preference("Remember Reading Position", "false");
    preferences.wait_until("remember-position", "false");

    off.open_here(&diary);
    assert_eq!(
        scroll_offset(&off),
        0,
        "a reader who turned the preference off was still put where they left off",
    );

    assert!(off.close().is_empty(), "the launch left processes behind");

    // And nothing is written down either: a reader with the switch off reads on and
    // closes the window on it, and the store is byte for byte what it was. A launch of
    // its own, because a window with a dialog the reader opened over it does not close
    // — that is libadwaita dismissing the dialog instead, and it is the same with or
    // without this feature (probed on libadwaita 1.8.6).
    let written = std::fs::read(preferences.reading_positions()).expect("read the store");
    read_to_and_close(&diary, &preferences, LEFT_IN);
    assert_eq!(
        std::fs::read(preferences.reading_positions()).expect("read the store back"),
        written,
        "a reader who asked axiomd not to remember was written down anyway",
    );

    // On again, in one launch: the very next document opens where they left it, and
    // the places written down before they turned it off are all still theirs.
    let on = axiomd_e2e::launch_with(&diary, &preferences);
    assert_eq!(
        scroll_offset(&on),
        0,
        "the preference was off when this document opened",
    );
    on.activate("app.preferences");
    on.set_preference("Remember Reading Position", "true");
    preferences.wait_until("remember-position", "true");

    on.open_here(&notes);
    assert!(
        screen_position_of(&on, LEFT_IN).abs() <= 4,
        "turning the preference back on did not resume where the reader left off",
    );

    assert!(on.close().is_empty(), "the launch left processes behind");
}
