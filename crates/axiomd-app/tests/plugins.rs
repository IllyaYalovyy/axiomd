//! Optional rendering capabilities, asserted against the running application (#16).
//!
//! The plugin layer's own contract — every hook, failure and cost — is asserted in
//! `axiomd-render`. What is asserted here is the half a reader can see: a switch in
//! preferences that changes the document in front of them, the styling a capability
//! needs arriving with it and only with it, and the two things a toggle may never
//! cost — a page load, or the reader's place.

use axiomd_e2e::{App, Fixture, Preferences};

/// The switch the reader turns, as preferences names it.
const ROW: &str = "Emoji shortcodes";

/// What the document says where the shortcode was written.
const SHORTCODE_TEXT: &str =
    "(document.querySelector('article.markdown p:last-of-type') ?? {}).textContent ?? ''";

/// The emoji spans on the page — one per shortcode the plugin rewrote.
const EMOJI: &str = "document.querySelectorAll('article.markdown .emoji').length";

/// Whether the page is asking for any plugin's styling.
const PLUGIN_STYLESHEETS: &str =
    "document.querySelectorAll('link[href^=\"axiomd://assets/plugin/\"]').length";

/// A document long enough to scroll, with a shortcode in its last paragraph — below
/// the reader, so that switching the plugin changes a block they are not looking at.
fn long_document() -> String {
    let mut source = String::from("# Notes\n\n");
    for paragraph in 1..=120 {
        source.push_str(&format!("Paragraph {paragraph}.\n\n"));
    }
    source.push_str("Shipped :tada: today.\n");
    source
}

/// Where the paragraph reading `text` sits on screen, rounded to whole pixels.
fn screen_position_of(app: &App, text: &str) -> i32 {
    let script = format!(
        "Math.round(Array.from(document.querySelectorAll('p')) \
         .find(block => block.textContent === {text:?}) \
         .getBoundingClientRect().top)"
    );
    app.dom(&script)
        .parse()
        .unwrap_or_else(|_| panic!("{text:?} is not a paragraph of the document on screen"))
}

fn scroll_offset(app: &App) -> i32 {
    app.dom("Math.round(document.scrollingElement.scrollTop)")
        .parse()
        .expect("a scroll offset")
}

/// The whole of a runtime toggle: the document changes where the reader is looking at
/// it, the page is never loaded again, and they keep their place (invariants 5 and 14).
///
/// A plugin is the one preference that costs a render — it decides what the document
/// *is*, not how it looks — so the page count moves here where a restyle may not move
/// it. The load count still may not: navigating the view is the flash and the lost
/// place that `design_decisions.md` exists to prevent.
#[test]
fn switching_a_plugin_off_gives_the_reader_their_source_back_where_they_stand() {
    let fixture = Fixture::new("plugins-toggle");
    let preferences = Preferences::new("plugins-toggle");
    let app = axiomd_e2e::launch_with(&fixture.write("notes.md", &long_document()), &preferences);

    assert_eq!(app.dom(EMOJI), "1", "the shortcode was not rewritten");
    assert_eq!(app.dom(SHORTCODE_TEXT), "Shipped 🎉 today.");

    // The reader is looking at paragraph 40, well above the block that will change.
    app.dom("document.querySelector('[data-line=\"81\"]').scrollIntoView(true)");
    let before = screen_position_of(&app, "Paragraph 40.");
    let scrolled = scroll_offset(&app);
    assert!(
        scrolled > 0,
        "the document did not scroll, so there is nothing to preserve",
    );

    let loads = app.navigation_count();
    let pages = app.render_count();

    app.activate("app.preferences");
    app.set_preference(ROW, "false");
    app.wait_until(&format!("{EMOJI} === 0"));

    assert_eq!(
        app.dom(SHORTCODE_TEXT),
        "Shipped :tada: today.",
        "the reader did not get the source they wrote back",
    );
    assert_eq!(
        app.dom(PLUGIN_STYLESHEETS),
        "0",
        "the styling stayed behind"
    );
    // And it is written down, so the next launch reads the same way.
    preferences.wait_until("disabled-plugins", "['emoji']");
    assert_eq!(
        app.navigation_count(),
        loads,
        "switching a plugin reloaded the page",
    );
    assert!(
        app.render_count() > pages,
        "the document was not rendered again, so nothing could have changed",
    );

    let after = screen_position_of(&app, "Paragraph 40.");
    assert!(
        (after - before).abs() <= 2,
        "the reader's paragraph moved from {before}px to {after}px on screen",
    );
    assert_eq!(scroll_offset(&app), scrolled, "the reader lost their place");

    // And back on again — with the styling it needs, which the page did not have a
    // moment ago and was never loaded again to get.
    app.set_preference(ROW, "true");
    app.wait_until(&format!("{EMOJI} === 1"));
    assert_eq!(
        app.dom(PLUGIN_STYLESHEETS),
        "1",
        "the capability came back and its styling did not",
    );
    assert_eq!(
        app.navigation_count(),
        loads,
        "coming back reloaded the page"
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The styling a plugin carries reaches the document it is used in, and no other —
/// which is what "an asset is conditional" means where the reader can see it: an emoji
/// inside emphasised prose is upright, and only the plugin's own stylesheet says so.
#[test]
fn the_styling_a_plugin_needs_arrives_with_it_and_nowhere_else() {
    let fixture = Fixture::new("plugins-assets");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", "*Shipped :tada: today.*\n"));

    assert_eq!(
        app.dom(PLUGIN_STYLESHEETS),
        "1",
        "the document used the plugin and asked for no styling of its",
    );
    assert_eq!(
        app.dom("getComputedStyle(document.querySelector('em')).fontStyle"),
        "italic",
        "the fixture's prose is not emphasised, so there is nothing to be upright in",
    );
    // The bytes behind `axiomd://assets/plugin/emoji/emoji.css` reached the page: this
    // is that stylesheet's rule and nothing else in the app declares it.
    assert_eq!(
        app.dom("getComputedStyle(document.querySelector('.emoji')).fontStyle"),
        "normal",
        "the plugin's stylesheet was linked and never served",
    );

    // A document with no shortcode in it needs none of that, and asks for none of it.
    app.open(&fixture.write("plain.md", "*Shipped today.*\n"));
    app.wait_until_windows(2);
    assert_eq!(
        app.dom(PLUGIN_STYLESHEETS),
        "0",
        "a document that used no capability loaded its styling anyway",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A preference belongs to the reader rather than to one document: it is there when
/// they come back, and every window is already reading with it.
#[test]
fn a_switched_off_plugin_is_still_off_in_the_next_launch_and_in_every_window() {
    let fixture = Fixture::new("plugins-persist");
    let notes = fixture.write("notes.md", "Shipped :tada: today.\n");
    let preferences = Preferences::with("plugins-persist", "disabled-plugins", "['emoji']");

    let app = axiomd_e2e::launch_with(&notes, &preferences);
    assert_eq!(
        app.dom(SHORTCODE_TEXT),
        "Shipped :tada: today.",
        "a launch ignored the plugin the reader had switched off",
    );
    app.activate("app.preferences");
    assert_eq!(
        app.preference(ROW),
        "false",
        "the switch disagrees with the document the reader is looking at",
    );

    // A second window opens with the same answer, and a plugin switched on reaches
    // both of them without either being reloaded.
    app.open(&fixture.write("more.md", "Shipped :tada: today.\n"));
    app.wait_until_windows(2);
    assert_eq!(app.dom(SHORTCODE_TEXT), "Shipped :tada: today.");

    let loads = app.navigation_count();
    app.activate("app.preferences");
    app.set_preference(ROW, "true");
    app.wait_until(&format!("{EMOJI} === 1"));
    assert_eq!(app.navigation_count(), loads, "the newest window reloaded");

    app.select_window(0);
    app.wait_until(&format!("{EMOJI} === 1"));

    assert!(app.close().is_empty(), "the launch left processes behind");
}
