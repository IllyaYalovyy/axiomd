//! UT-007, against the running application: what happens when the reader clicks.
//!
//! Every link class the issue names gets a test that drives the shipped binary on a
//! headless compositor and asserts what the reader would see afterwards — the
//! document that appeared, where the view ended up, what left the app. The remote
//! images are asserted from the other side as well: a web server of the test's own
//! that counts what reaches it, so "zero requests until the reader clicks" is a
//! number rather than a belief.

mod support;

use axiomd_e2e::Fixture;
use support::Origin;

/// A document with one of each kind of link in it, and enough text on both sides of
/// its section that the section can actually be brought to the top of the window —
/// otherwise "did the reader arrive at it" would be asking the document to scroll
/// further than it can.
fn guide() -> String {
    let filler = "Filler paragraph.\n\n".repeat(60);
    format!(
        "# Guide\n\n\
         An [inline link](notes.md) to another document, an [anchor](#getting-started), \
         an [external link](https://example.com/page), and an [attachment](report.pdf).\n\n\
         {filler}\n\
         ## Getting Started\n\n\
         The section an anchor link lands on.\n\n{filler}",
    )
}

/// UT-007, first class: a relative `.md` link opens in the same window, and the
/// reader can get back.
#[test]
fn a_relative_markdown_link_opens_in_the_same_window_with_back_and_forward() {
    let fixture = Fixture::new("link-md");
    let guide_file = fixture.write("guide.md", &guide());
    fixture.write("notes.md", "# Notes\n\nThe document that was linked to.\n");

    let app = axiomd_e2e::launch(&guide_file);
    app.click("a[href=\"notes.md\"]");
    app.wait_until("document.querySelector('h1').textContent === 'Notes'");

    assert_eq!(app.window_count(), 1, "the link opened a second window");
    assert_eq!(app.window_title(), "notes.md");

    app.activate("win.back");
    app.wait_until("document.querySelector('h1').textContent === 'Guide'");
    assert_eq!(app.window_title(), "guide.md");

    app.activate("win.forward");
    app.wait_until("document.querySelector('h1').textContent === 'Notes'");
    assert_eq!(app.window_title(), "notes.md");

    assert!(
        app.handed_over().is_empty(),
        "a document in the reader's own folder was handed to the desktop: {:?}",
        app.handed_over(),
    );
    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A link written for GitHub — `file.md#section` — has to find the same section here,
/// which is the whole reason the heading ids follow GitHub's slugs.
#[test]
fn a_link_to_a_section_of_another_document_arrives_at_that_section() {
    let fixture = Fixture::new("link-md-anchor");
    let start = fixture.write(
        "start.md",
        "# Start\n\nGo to [the guide](guide.md#getting-started).\n",
    );
    fixture.write("guide.md", &guide());

    let app = axiomd_e2e::launch(&start);
    app.click("a[href=\"guide.md#getting-started\"]");
    app.wait_until("document.querySelector('h1').textContent === 'Guide'");
    app.wait_until("document.scrollingElement.scrollTop > 0");

    assert_eq!(
        app.dom("document.querySelector('h2').id"),
        "getting-started",
        "the section a GitHub-style link names has a different id here",
    );
    let at = app
        .dom("Math.round(document.querySelector('#getting-started').getBoundingClientRect().top)");
    assert!(
        at.parse::<f64>().expect("a position").abs() < 8.0,
        "the reader arrived {at} pixels away from the section the link named",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// UT-007, second class: an anchor scrolls within the document, without leaving it.
#[test]
fn an_anchor_link_moves_the_reader_within_the_document() {
    let fixture = Fixture::new("link-anchor");
    let app = axiomd_e2e::launch(&fixture.write("guide.md", &guide()));

    assert_eq!(app.dom("document.scrollingElement.scrollTop"), "0");
    app.click("a[href=\"#getting-started\"]");
    app.wait_until("document.scrollingElement.scrollTop > 0");

    assert_eq!(app.dom_text("h1"), "Guide", "the document was left behind");
    assert_eq!(app.window_count(), 1);
    assert!(
        app.handed_over().is_empty(),
        "an anchor was handed to the desktop",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// UT-007, third and fourth classes: neither an external address nor a file axiomd
/// does not render is ever shown in the view; both leave the app, and only on a click.
#[test]
fn an_external_link_and_an_unrenderable_file_leave_the_app_and_the_document_stays() {
    let fixture = Fixture::new("link-out");
    let guide_file = fixture.write("guide.md", &guide());
    fixture.write("report.pdf", "%PDF-1.4\n");

    let app = axiomd_e2e::launch(&guide_file);
    let loads = app.navigation_count();

    app.click("a[href=\"https://example.com/page\"]");
    app.click("a[href=\"report.pdf\"]");
    app.wait_for_handed_over(2);

    // Both of them left, and the order they left in is not asserted: each hand-over is
    // a process of its own that the desktop starts and that writes into the log when
    // it runs (`axiomd-e2e/src/display.rs`), so which of two writes first is the
    // machine's business and not axiomd's. Asserting it made this test fail under a
    // loaded gate run with the two the other way round.
    let handed = app.handed_over();
    assert_eq!(handed.len(), 2, "the desktop was handed {handed:?}");
    assert!(
        handed.iter().any(|out| out == "https://example.com/page"),
        "the external address was not handed to the desktop: {handed:?}",
    );
    assert!(
        handed.iter().any(|out| out.ends_with("report.pdf")),
        "the attachment was not handed to the desktop: {handed:?}",
    );
    assert_eq!(app.dom_text("h1"), "Guide", "the view left the document");
    assert_eq!(
        app.navigation_count(),
        loads,
        "the view navigated somewhere it should never go",
    );
    assert_eq!(app.window_count(), 1);

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The invariant, measured from the far end: a document full of remote images causes
/// no request at all until the reader presses one, and then exactly one.
#[test]
fn a_remote_image_is_fetched_only_when_the_reader_presses_its_card() {
    let origin = Origin::start();
    let fixture = Fixture::new("remote-one");
    let app = axiomd_e2e::launch(&fixture.write(
        "notes.md",
        &format!(
            "# Notes\n\n![A diagram]({})\n\n![Another]({})\n",
            origin.url("/diagram.png"),
            origin.url("/another.png"),
        ),
    ));

    // The invariant first, from the far end, and then what the reader can see of the
    // document while it holds.
    assert_eq!(
        origin.requests(),
        Vec::<String>::new(),
        "showing the document fetched something",
    );
    assert_eq!(
        app.dom("document.querySelectorAll('a.remote-image').length"),
        "2",
        "the remote images are not placeholder cards",
    );
    assert_eq!(app.dom("document.querySelectorAll('img').length"), "0");
    assert_eq!(
        app.dom_text("a.remote-image .remote-image-label"),
        "A diagram",
        "the card does not say what it stands for",
    );
    assert_eq!(
        app.dom("document.querySelectorAll('.remote-banner:not([hidden])').length"),
        "1",
        "there is no inline load-all affordance",
    );

    app.click("a.remote-image[data-remote-src$=\"/diagram.png\"]");
    app.wait_until("document.querySelectorAll('img').length === 1");

    assert_eq!(
        origin.requests(),
        vec!["/diagram.png".to_owned()],
        "pressing one card did not make exactly one request for that image",
    );
    assert_eq!(
        app.dom("document.querySelector('img').naturalWidth"),
        "40",
        "the image was put in the page but never decoded",
    );
    assert_eq!(
        app.dom("document.querySelector('img').getAttribute('alt')"),
        "A diagram",
        "the image lost the alt text its placeholder carried",
    );
    assert!(
        app.dom("document.querySelector('img').src")
            .starts_with("axiomd://img-"),
        "the image is not served from the document's own origin: {}",
        app.dom("document.querySelector('img').src"),
    );
    assert_eq!(
        app.dom("document.querySelectorAll('a.remote-image').length"),
        "1",
        "the other image was loaded too",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The one per-document affordance: every placeholder still standing, in one press.
#[test]
fn load_all_loads_every_placeholder_that_is_left() {
    let origin = Origin::start();
    let fixture = Fixture::new("remote-all");
    let app = axiomd_e2e::launch(&fixture.write(
        "notes.md",
        &format!(
            "# Notes\n\n![one]({})\n\n![two]({})\n\n![three]({})\n",
            origin.url("/1.png"),
            origin.url("/2.png"),
            origin.url("/3.png"),
        ),
    ));

    app.click("a.remote-image[data-remote-src$=\"/2.png\"]");
    app.wait_until("document.querySelectorAll('img').length === 1");
    app.click(".remote-banner-action");
    app.wait_until("document.querySelectorAll('img').length === 3");

    let mut asked: Vec<String> = origin.requests();
    asked.sort();
    assert_eq!(
        asked,
        ["/1.png", "/2.png", "/3.png"],
        "load all did not ask for each image exactly once",
    );
    assert_eq!(
        app.dom("document.querySelectorAll('a.remote-image').length"),
        "0",
        "a placeholder was left behind",
    );
    assert_eq!(
        app.dom("document.querySelectorAll('.remote-banner:not([hidden])').length"),
        "0",
        "the load-all affordance is still offering to load nothing",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The interaction with live reload (UT-004): the file changing must not quietly take
/// back an image the reader asked for.
#[test]
fn an_image_the_reader_loaded_survives_the_file_changing_underneath_it() {
    let origin = Origin::start();
    let fixture = Fixture::new("remote-reload");
    let notes = fixture.write(
        "notes.md",
        &format!("# Notes\n\n![A diagram]({})\n", origin.url("/diagram.png")),
    );

    let app = axiomd_e2e::launch(&notes);
    app.click("a.remote-image");
    app.wait_until("document.querySelectorAll('img').length === 1");

    std::fs::write(
        &notes,
        format!(
            "# Notes\n\nA new paragraph.\n\n![A diagram]({})\n",
            origin.url("/diagram.png")
        ),
    )
    .expect("rewrite the document");
    app.wait_until("document.body.textContent.includes('A new paragraph')");

    assert_eq!(
        app.dom("document.querySelectorAll('img').length"),
        "1",
        "the file changing took back the image the reader had loaded",
    );
    assert_eq!(
        origin.requests(),
        vec!["/diagram.png".to_owned()],
        "the reload fetched the image again",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A load that does not work is a thing the reader is told about where they clicked,
/// and the card stays the button — never a dialog (`ux_decisions.md`).
#[test]
fn an_image_that_will_not_load_says_so_on_the_card_it_was_asked_from() {
    let origin = Origin::start();
    let fixture = Fixture::new("remote-fails");
    let app = axiomd_e2e::launch(&fixture.write(
        "notes.md",
        &format!(
            "# Notes\n\n![Gone]({})\n\n![Not a picture]({})\n",
            origin.url("/gone.jpeg"),
            origin.url("/page.html"),
        ),
    ));

    app.click("a.remote-image[data-remote-src$=\"/gone.jpeg\"]");
    app.click("a.remote-image[data-remote-src$=\"/page.html\"]");
    app.wait_until("document.querySelectorAll('a.remote-image-failed').length === 2");

    assert_eq!(
        app.dom("document.querySelectorAll('img').length"),
        "0",
        "something that is not an image was put in the document",
    );
    assert!(
        app.dom_text("a.remote-image[data-remote-src$=\"/page.html\"] .remote-image-action")
            .contains("not an image"),
        "the reader is not told why the card is still a card: {}",
        app.dom_text("a.remote-image[data-remote-src$=\"/page.html\"] .remote-image-action"),
    );
    assert!(
        app.dom_text("a.remote-image[data-remote-src$=\"/gone.jpeg\"] .remote-image-action")
            .contains("Try again"),
        "a failed card stopped being a button",
    );
    assert_eq!(app.dom_text("h1"), "Notes", "the document was disturbed");

    // And it is a button, not just labelled as one: pressing it asks again.
    app.click("a.remote-image[data-remote-src$=\"/gone.jpeg\"]");
    app.wait_for("the card to ask for its image again", || {
        origin
            .requests()
            .iter()
            .filter(|path| *path == "/gone.jpeg")
            .count()
            == 2
    });

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// An SVG is allowed and is inert: it arrives through `<img>`, which the HTML standard
/// puts in "secure static mode" — no scripts, no external references — so a picture
/// cannot become behaviour whatever it contains.
#[test]
fn a_remote_svg_is_displayed_and_cannot_run_what_is_inside_it() {
    let origin = Origin::start();
    let fixture = Fixture::new("remote-svg");
    let app = axiomd_e2e::launch(&fixture.write(
        "notes.md",
        &format!("# Notes\n\n![Chart]({})\n", origin.url("/chart.svg")),
    ));

    app.click("a.remote-image");
    // Until the picture has been decoded it has no size, and the element standing in
    // the document is there well before that: waiting on the element alone made this
    // test read a width of 0 under a loaded gate run. `complete` is true whether the
    // decode succeeded or failed, so the size below is still what says it succeeded.
    app.wait_until(
        "document.querySelectorAll('img').length === 1 && document.querySelector('img').complete",
    );

    assert_eq!(
        app.dom("document.querySelector('img').naturalWidth"),
        "40",
        "the SVG was not displayed",
    );
    assert_eq!(
        app.dom_text("h1"),
        "Notes",
        "the SVG's script ran inside the document",
    );
    assert_eq!(app.dom("document.querySelectorAll('script').length"), "0");

    assert!(app.close().is_empty(), "the launch left processes behind");
}
