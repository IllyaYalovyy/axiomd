//! Choosing which markdown engine reads a document, on the running application
//! (issue #17).
//!
//! Three levels, and every one of them is asserted by what the reader sees rather than
//! by what was configured: the application default in preferences, this window's own
//! choice in the main menu, and the `--engine` flag a test launches with.
//!
//! # What "the reader sees the engine change" means here
//!
//! The two engines this build has differ on GFM extended autolinks: comrak turns a
//! bare `www.example.com` in prose into a link, pulldown-cmark leaves it as prose,
//! and pulldown-cmark says so in its capability report
//! (`designs/engine-comparison.md`). So the assertion is a link in the page appearing
//! and disappearing — a difference a reader would notice, read out of the DOM in front
//! of them — and not that a setting was written.
//!
//! # And what it must not cost
//!
//! Every switch is measured against the view's load count. An engine changes what the
//! document *is*, so it costs a render; it must never cost a reload, because a reload
//! is the flash and the lost place axiomd exists to avoid (invariants 5 and 9).

use axiomd_e2e::{App, Fixture, Preferences};

/// A document long enough to be scrolled in, with a bare address partway down.
///
/// The address is what the two engines disagree about; the rest is there so the reader
/// has somewhere to be when the engine changes under them.
fn notes() -> String {
    let mut document = String::from("# Engines\n\nVisit www.example.com for more.\n\n");
    for section in 1..=30 {
        document.push_str(&format!(
            "## Section {section}\n\nA paragraph of section {section}, long enough to \
             take a line of its own and then some more of the page.\n\n"
        ));
    }
    document
}

/// Whether the page in front of the reader has turned the bare address into a link.
const LINKED: &str = "document.querySelector('a[href=\"http://www.example.com\"]') !== null";

/// The engine each of this build's engines is, by what it does to that address.
const LINKS_BARE_ADDRESSES: &str = "comrak";
const LEAVES_BARE_ADDRESSES: &str = "pulldown-cmark";

/// The reader switches the engine from the main menu, and the document in front of
/// them changes where it stands.
///
/// The whole exit criterion in one test: the page is re-rendered, it is not reloaded,
/// and the reader is left exactly where they were reading.
#[test]
fn switching_the_engine_re_renders_the_document_without_moving_the_reader() {
    let fixture = Fixture::new("engine-switch");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", &notes()));

    assert_eq!(
        app.engine(),
        LINKS_BARE_ADDRESSES,
        "the default is not comrak"
    );
    assert_eq!(app.dom(LINKED), "true", "comrak did not link the address");

    // The reader is a long way down the document, on a block the page can be asked
    // about by name.
    app.wait_until("document.querySelector('[data-line=\"59\"]') !== null");
    app.dom("document.querySelector('[data-line=\"59\"]').scrollIntoView(true)");
    let where_they_are = app.dom("Math.round(document.scrollingElement.scrollTop)");
    assert_ne!(where_they_are, "0", "the reader never left the top");
    let loads = app.navigation_count();

    // The menu item, activated exactly as pressing it does.
    app.activate(&format!("win.engine::{LEAVES_BARE_ADDRESSES}"));
    app.wait_until(&format!("!({LINKED})"));

    assert_eq!(
        app.engine(),
        LEAVES_BARE_ADDRESSES,
        "the menu still shows the engine the reader switched away from",
    );
    // The address is still prose — less markup, never less document.
    assert!(
        app.dom_text("article.markdown p")
            .contains("www.example.com"),
        "the address itself left the document",
    );
    assert_eq!(
        app.navigation_count(),
        loads,
        "switching the engine reloaded the document",
    );
    assert_eq!(
        app.dom("Math.round(document.scrollingElement.scrollTop)"),
        where_they_are,
        "switching the engine moved the reader",
    );
    assert_eq!(
        app.dom_text("[data-line=\"59\"]").trim(),
        "A paragraph of section 14, long enough to take a line of its own and then \
         some more of the page.",
        "the document under the reader is not the one that was there",
    );

    // And back again, which is the same journey in the other direction.
    app.activate(&format!("win.engine::{LINKS_BARE_ADDRESSES}"));
    app.wait_until(LINKED);
    assert_eq!(app.navigation_count(), loads, "coming back reloaded");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A window the reader switched keeps its engine as they read on.
///
/// An override that a link forgot would be a control that only appears to work: the
/// reader chose how to read, not how to read one page.
#[test]
fn a_switched_window_keeps_its_engine_across_documents() {
    let fixture = Fixture::new("engine-across");
    let first = fixture.write("first.md", &notes());
    let second = fixture.write("second.md", "# Second\n\nAlso www.example.com here.\n");
    let app = axiomd_e2e::launch(&first);

    app.activate(&format!("win.engine::{LEAVES_BARE_ADDRESSES}"));
    app.wait_until(&format!("!({LINKED})"));

    app.open_here(&second);
    app.wait_until("document.querySelector('h1') !== null");

    assert_eq!(app.dom_text("h1"), "Second");
    assert_eq!(
        app.engine(),
        LEAVES_BARE_ADDRESSES,
        "the window forgot the engine the reader chose when it was given a document",
    );
    assert_eq!(
        app.dom(LINKED),
        "false",
        "the second document was read with an engine the reader had switched away from",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The override belongs to one window, and to nothing else (invariant 7).
#[test]
fn switching_one_window_leaves_every_other_window_alone() {
    let fixture = Fixture::new("engine-windows");
    let document = notes();
    let notes = fixture.write("notes.md", &document);
    let more = fixture.write("more.md", &document);

    let app = axiomd_e2e::launch(&notes);
    app.open(&more);
    app.wait_until_windows(2);

    // The newest window is switched; the first one is not.
    app.activate(&format!("win.engine::{LEAVES_BARE_ADDRESSES}"));
    app.wait_until(&format!("!({LINKED})"));

    app.select_window(0);
    assert_eq!(
        app.engine(),
        LINKS_BARE_ADDRESSES,
        "one window's choice of engine reached another window",
    );
    assert_eq!(app.dom(LINKED), "true", "the first window was re-read");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The application default, in preferences: it offers every engine this build has, it
/// applies to the document the reader is looking at, and it is there when they come
/// back (issue #20, invariant 14).
#[test]
fn the_default_engine_is_a_preference_that_applies_live_and_is_remembered() {
    let fixture = Fixture::new("engine-preference");
    let notes = fixture.write("notes.md", &notes());
    let preferences = Preferences::new("engine-preference");

    let app = axiomd_e2e::launch_with(&notes, &preferences);
    assert_eq!(app.dom(LINKED), "true");
    let loads = app.navigation_count();

    app.activate("app.preferences");
    assert_eq!(app.preference("Markdown engine"), LINKS_BARE_ADDRESSES);
    app.set_preference("Markdown engine", LEAVES_BARE_ADDRESSES);

    // The document the reader is looking at, re-read where it stands.
    app.wait_until(&format!("!({LINKED})"));
    assert_eq!(app.engine(), LEAVES_BARE_ADDRESSES);
    assert_eq!(
        app.navigation_count(),
        loads,
        "changing the default engine reloaded the document",
    );
    preferences.wait_until("engine", &format!("'{LEAVES_BARE_ADDRESSES}'"));
    assert!(app.close().is_empty(), "the launch left processes behind");

    // And it is still theirs when they come back, with no dialog and nothing to reapply.
    let returning: App = axiomd_e2e::launch_with(&notes, &preferences);
    assert_eq!(returning.engine(), LEAVES_BARE_ADDRESSES);
    assert_eq!(returning.dom(LINKED), "false");
    assert_eq!(returning.visible_dialog(), "");
    assert!(
        returning.close().is_empty(),
        "the launch left processes behind",
    );
}

/// A window the reader switched keeps its own engine when the *default* changes, and
/// a window they have not switched follows it.
///
/// The two halves of what an override is: it overrides, and it is the only thing that
/// does.
#[test]
fn a_switched_window_keeps_its_engine_when_the_default_changes() {
    let fixture = Fixture::new("engine-override");
    let document = notes();
    let notes = fixture.write("notes.md", &document);
    let more = fixture.write("more.md", &document);
    let preferences = Preferences::new("engine-override");

    let app = axiomd_e2e::launch_with(&notes, &preferences);
    app.open(&more);
    app.wait_until_windows(2);

    // The newest window is switched to the engine that leaves addresses alone. The
    // first is left following the preference.
    app.activate(&format!("win.engine::{LEAVES_BARE_ADDRESSES}"));
    app.wait_until(&format!("!({LINKED})"));

    // Now the reader changes the default to that same engine and back again.
    app.activate("app.preferences");
    app.set_preference("Markdown engine", LEAVES_BARE_ADDRESSES);
    app.select_window(0);
    app.wait_until(&format!("!({LINKED})"));
    assert_eq!(
        app.engine(),
        LEAVES_BARE_ADDRESSES,
        "a window that had made no choice did not follow the preference",
    );

    app.select_window(1);
    app.activate("app.preferences");
    app.set_preference("Markdown engine", LINKS_BARE_ADDRESSES);

    app.select_window(0);
    app.wait_until(LINKED);
    app.select_window(1);
    assert_eq!(
        app.engine(),
        LEAVES_BARE_ADDRESSES,
        "the preference overrode the engine this window's reader had chosen",
    );
    assert_eq!(
        app.dom(LINKED),
        "false",
        "the switched window was re-read with the preference's engine",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// `--engine`: the flag a test launches the real application with, reading documents
/// with an engine other than the reader's preference and writing nothing to it.
#[test]
fn the_engine_flag_reads_documents_with_the_engine_it_names() {
    let fixture = Fixture::new("engine-flag");
    let notes = fixture.write("notes.md", &notes());

    let app = axiomd_e2e::launch_with_engine(&notes, LEAVES_BARE_ADDRESSES);
    assert_eq!(app.engine(), LEAVES_BARE_ADDRESSES);
    assert_eq!(
        app.dom(LINKED),
        "false",
        "--engine named an engine and the document was read with another",
    );
    // A flag, not a preference: nothing about the reader's own settings changed, so the
    // menu still offers the way back.
    app.activate(&format!("win.engine::{LINKS_BARE_ADDRESSES}"));
    app.wait_until(LINKED);

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Every engine this build has is in the menu, and picking any of them does something
/// real. An affordance that renders and does nothing is a defect.
#[test]
fn every_engine_the_build_has_can_be_picked_and_reads_the_document() {
    let fixture = Fixture::new("engine-menu");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", &notes()));

    for engine in [
        LEAVES_BARE_ADDRESSES,
        LINKS_BARE_ADDRESSES,
        LEAVES_BARE_ADDRESSES,
    ] {
        app.activate(&format!("win.engine::{engine}"));
        app.wait_until_engine(engine);
        // Not just that the state moved: the document really was read again by it.
        app.wait_until(&format!(
            "{} document.querySelector('h1').textContent === 'Engines'",
            match engine == LINKS_BARE_ADDRESSES {
                true => format!("{LINKED} &&"),
                false => format!("!({LINKED}) &&"),
            }
        ));
    }

    assert!(app.close().is_empty(), "the launch left processes behind");
}
