//! Block image captions, in the running application (issue #39).
//!
//! The documents this application was written for put a picture alone in a paragraph
//! and the sentence about it in the alt text. Rendered as a bare `<img>` that sentence
//! is invisible, so the reader gets a diagram and no idea what it shows. These tests
//! drive the shipped binary on a headless compositor and assert what the reader ends
//! up looking at: the caption is on the page, it is under the picture it names, it is
//! smaller and dimmer than the document's own voice, and it survives the two ways a
//! document leaves the app.

mod support;

use std::path::Path;

use axiomd_e2e::{App, Fixture};
use support::Origin;

/// The caption of the article's first diagram: a sentence that appears nowhere in the
/// document's own text, so finding it on the page — or in a PDF — can only mean the
/// alt text became a caption.
const FIRST: &str = "Diagram: where latency actually accumulates in an I/O-bound request";

/// And of the second, which is titled rather than captioned by its alt text — the
/// other half of the precedence rule.
const SECOND: &str = "Figure 2: the same request once the queue is explicit";

/// A page of the reference article: two block images written the way Medium, Ghost and
/// every static-site generator write them, and one picture inline in a sentence.
fn article() -> String {
    format!(
        "# Where the time goes\n\n\
         A request that waits is not a request that is slow, and the difference is \
         where the time is spent.\n\n\
         ![{FIRST}](diagram-one.png)\n\n\
         The queue is the part nobody draws, so here it is drawn: \
         ![a small icon](icon.png) marks each place a request can wait.\n\n\
         ![The alt text a screen reader reads](diagram-two.png \"{SECOND}\")\n\n\
         Steady.\n",
    )
}

/// Writes the article and the pictures it names, and returns the document to open.
fn article_in(fixture: &Fixture) -> std::path::PathBuf {
    for picture in ["diagram-one.png", "diagram-two.png", "icon.png"] {
        let path = fixture.write(picture, "");
        std::fs::write(&path, support::png()).expect("write a picture");
    }
    fixture.write("article.md", &article())
}

/// Waits until every picture in the document has been drawn, so a position read off
/// the page is the position it ends up at rather than one on the way there.
fn drawn(app: &App, pictures: usize) {
    app.wait_until(&format!(
        "document.querySelectorAll('figure img').length === {pictures} && \
         [...document.querySelectorAll('img')].every(picture => picture.complete)"
    ));
}

/// The exit criterion, in read mode: the reader sees the caption of each diagram,
/// under the diagram, and the alt text is still on the picture for a screen reader.
#[test]
fn the_articles_diagrams_are_captioned_under_the_pictures() {
    let fixture = Fixture::new("figure-read");
    let app = axiomd_e2e::launch(&article_in(&fixture));
    drawn(&app, 2);

    assert_eq!(
        app.dom("document.querySelectorAll('figure').length"),
        "2",
        "the block images are not figures",
    );
    assert_eq!(
        app.dom("[...document.querySelectorAll('figcaption')].map(c => c.textContent).join('|')"),
        format!("{FIRST}|{SECOND}"),
        "the captions are not the ones the author wrote",
    );
    // The title captioned the second picture; its alt text stayed where a screen
    // reader looks for it.
    assert_eq!(
        app.dom("document.querySelectorAll('figure img')[1].getAttribute('alt')"),
        "The alt text a screen reader reads",
        "the caption was taken out of the alt text",
    );
    assert_eq!(
        app.dom("document.querySelectorAll('figure img')[0].naturalWidth"),
        "40",
        "the diagram itself is not on the page",
    );

    // Under the picture, and drawn: a caption with no height is a caption nobody sees.
    for at in 0..2 {
        let below = app.dom(&format!(
            "(() => {{ const figure = document.querySelectorAll('figure')[{at}]; \
             const picture = figure.querySelector('img').getBoundingClientRect(); \
             const caption = figure.querySelector('figcaption').getBoundingClientRect(); \
             return `${{Math.round(caption.top - picture.bottom)}}:${{Math.round(caption.height)}}`; \
             }})()"
        ));
        let (gap, height) = below.split_once(':').expect("a gap and a height");
        assert!(
            gap.parse::<f64>().expect("a gap") >= 0.0,
            "caption {at} is not under its picture: it starts {gap}px below its bottom",
        );
        assert!(
            height.parse::<f64>().expect("a height") > 0.0,
            "caption {at} is drawn {height}px tall, so nobody can read it",
        );
    }

    // And a picture with words beside it is untouched: still inline, still in its
    // paragraph, still with no caption of its own.
    assert_eq!(
        app.dom("document.querySelectorAll('p img').length"),
        "1",
        "the inline picture left its paragraph",
    );
    assert_eq!(
        app.dom("document.querySelectorAll('p img')[0].closest('figure')"),
        "null",
        "the inline picture became a figure",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A caption is an aside about the picture, not a line of the document: smaller than
/// the prose around it, dimmer than the document's own ink, and centred under what it
/// names — on a light desktop and on a dark one, out of the same palette.
#[test]
fn a_caption_is_smaller_dimmer_and_centred_in_both_readings() {
    for theme in ["light", "dark"] {
        let fixture = Fixture::new(&format!("figure-style-{theme}"));
        let preferences = axiomd_e2e::Preferences::with(
            &format!("figure-style-{theme}"),
            "theme",
            &format!("'{theme}'"),
        );
        let app = axiomd_e2e::launch_with(&article_in(&fixture), &preferences);
        drawn(&app, 2);

        let style = |property: &str, selector: &str| {
            app.dom(&format!(
                "getComputedStyle(document.querySelector('{selector}')).{property}"
            ))
        };
        let size = |selector: &str| {
            style("fontSize", selector)
                .trim_end_matches("px")
                .parse::<f64>()
                .expect("a font size in pixels")
        };

        assert!(
            size("figcaption") < size(".markdown p"),
            "in the {theme} reading a caption is {}px against {}px of prose",
            size("figcaption"),
            size(".markdown p"),
        );
        assert_ne!(
            style("color", "figcaption"),
            style("color", ".markdown p"),
            "in the {theme} reading a caption is written in the document's own ink",
        );
        assert_eq!(
            style("textAlign", "figcaption"),
            "center",
            "the {theme} caption is not centred under its picture",
        );

        assert!(app.close().is_empty(), "the launch left processes behind");
    }
}

/// D4 and the caption compose: the card standing in for a picture nobody has asked for
/// sits inside the figure with the caption already under it, the document still fetches
/// nothing, and when the reader presses the card the picture arrives *in the card's
/// place* — so the caption is where it was rather than jumping.
#[test]
fn a_remote_pictures_caption_is_there_before_it_loads_and_stays_when_it_arrives() {
    let origin = Origin::start();
    let fixture = Fixture::new("figure-remote");
    let app = axiomd_e2e::launch(&fixture.write(
        "notes.md",
        &format!("# Notes\n\n![{FIRST}]({})\n", origin.url("/diagram.png")),
    ));
    app.wait_until("document.querySelectorAll('figure').length === 1");

    assert_eq!(
        origin.requests(),
        Vec::<String>::new(),
        "showing the captioned figure fetched something",
    );
    assert_eq!(
        app.dom("document.querySelectorAll('figure > a.remote-image').length"),
        "1",
        "the placeholder card is not inside the figure",
    );
    assert_eq!(
        app.dom_text("figcaption"),
        FIRST,
        "the caption is not under the card the reader has not pressed",
    );

    app.click("a.remote-image[data-remote-src$=\"/diagram.png\"]");
    app.wait_until(
        "document.querySelectorAll('figure > img').length === 1 && \
         document.querySelector('figure > img').complete",
    );

    assert_eq!(
        origin.requests(),
        vec!["/diagram.png".to_owned()],
        "pressing the card did not make exactly one request",
    );
    assert_eq!(
        app.dom_text("figcaption"),
        FIRST,
        "the caption did not survive the picture arriving",
    );
    assert_eq!(
        app.dom("document.querySelectorAll('figure').length"),
        "1",
        "the figure was rebuilt around the picture instead of the card being replaced",
    );
    assert_eq!(
        app.dom(
            "(() => { const figure = document.querySelector('figure'); \
             return [...figure.children].map(child => child.tagName).join(','); })()"
        ),
        "IMG,FIGCAPTION",
        "the picture did not arrive in the card's place, above the caption",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// UT-004 and the caption together: an edit elsewhere in the file leaves the figure
/// the very element it was — so the diagram is not fetched and drawn again, the
/// caption does not flash, and the reader is still looking at the same place.
#[test]
fn a_captioned_figure_is_kept_in_place_when_the_file_changes() {
    let fixture = Fixture::new("figure-reload");
    let document = article_in(&fixture);
    let app = axiomd_e2e::launch(&document);
    drawn(&app, 2);

    let navigations = app.navigation_count();
    // The reader has the caption selected — something they can only still have
    // afterwards if the element carrying it is the same element (`reload.rs`).
    app.dom(
        "(() => { const range = document.createRange(); \
         range.selectNodeContents(document.querySelector('figcaption')); \
         const selected = window.getSelection(); \
         selected.removeAllRanges(); selected.addRange(range); })()",
    );
    assert_eq!(
        app.dom("window.getSelection().toString()"),
        FIRST,
        "the test could not select the caption to begin with",
    );
    let was_at =
        app.dom("Math.round(document.querySelector('figure').getBoundingClientRect().top)");

    std::fs::write(&document, article().replace("Steady.", "Steadier."))
        .expect("save the document");
    app.wait_until("[...document.querySelectorAll('p')].some(p => p.textContent === 'Steadier.')");

    assert_eq!(
        app.dom("window.getSelection().toString()"),
        FIRST,
        "the figure was thrown away and built again for an edit somewhere else, \
         taking the reader's selection with it",
    );
    assert_eq!(
        app.dom_text("figcaption"),
        FIRST,
        "the caption did not survive the file changing",
    );
    assert_eq!(
        app.dom("document.querySelector('figure img').naturalWidth"),
        "40",
        "the diagram was left broken by the reload",
    );
    assert_eq!(
        app.dom("Math.round(document.querySelector('figure').getBoundingClientRect().top)"),
        was_at,
        "the reader's place moved when the file changed",
    );
    assert_eq!(
        app.navigation_count(),
        navigations,
        "the document was reloaded as a page, which costs the reader their place",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The other two exit criteria: the captions the reader sees are the captions on paper
/// and in the page they mail to somebody. Both come out of the one print path, so both
/// are asserted on the file that path produced.
#[test]
fn the_captions_leave_the_app_with_the_document() {
    let fixture = Fixture::new("figure-export");
    let document = article_in(&fixture);
    let app = axiomd_e2e::launch(&document);
    drawn(&app, 2);

    let page = document.with_file_name("article.html");
    export(&app, &page);
    let exported = std::fs::read_to_string(&page).expect("read the exported page");
    assert!(
        exported.contains(&format!("<figcaption>{FIRST}</figcaption>")),
        "the exported page lost the first caption:\n{exported}",
    );
    assert!(
        exported.contains(&format!("<figcaption>{SECOND}</figcaption>")),
        "the exported page lost the second caption:\n{exported}",
    );

    let pdf = document.with_file_name("article.pdf");
    export(&app, &pdf);
    let text = pdf_extract::extract_text_by_pages(&pdf)
        .unwrap_or_else(|error| panic!("{} is not a readable PDF: {error}", pdf.display()))
        .join("\n");
    for caption in [FIRST, SECOND] {
        assert!(
            squeezed(&text).contains(&squeezed(caption)),
            "the printed page lost the caption {caption:?}:\n{text}",
        );
    }

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Exports `document` to `file` and waits until the window says it is done — the whole
/// sentence, because a failed export says "Exported nothing — …" (`print.rs`).
fn export(app: &App, file: &Path) {
    let name = file
        .file_name()
        .and_then(|name| name.to_str())
        .expect("a file to export to");
    app.export_to(file);
    let said = app.wait_for_banner("Exported");
    assert_eq!(said, format!("Exported {name}"));
    assert!(file.is_file(), "{} was never written", file.display());
}

/// `text` with every space, tab and newline taken out: paper breaks a line wherever
/// the page ends, and a caption long enough to be a caption is a caption that wraps.
fn squeezed(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

/// The captioned article as a human approved it, on a light desktop.
///
/// Ignored until a human has looked at the picture and pinned it: approving a rendered
/// surface for the first time is theirs to do, not the harness's (`docs/TESTING.md`).
/// To pin it, look at `target/debug/e2e-artifacts/figures-light.actual.png` from a
/// failing run and, if it is right, re-run this test with `AXIOMD_PIN_GOLDENS=1` set,
/// then remove the `#[ignore]`.
#[test]
#[ignore = "awaiting the first human visual approval; see the comment above"]
fn a_captioned_article_still_looks_the_way_it_was_approved() {
    let fixture = Fixture::new("figure-golden-light");
    let app = axiomd_e2e::launch(&article_in(&fixture));
    drawn(&app, 2);

    app.screenshot().assert_matches("figures-light");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The same on a dark desktop.
#[test]
#[ignore = "awaiting the first human visual approval; see the comment above"]
fn a_captioned_article_in_the_dark_still_looks_the_way_it_was_approved() {
    let fixture = Fixture::new("figure-golden-dark");
    let dark = axiomd_e2e::Preferences::with("figure-golden-dark", "theme", "'dark'");
    let app = axiomd_e2e::launch_with(&article_in(&fixture), &dark);
    drawn(&app, 2);

    app.screenshot().assert_matches("figures-dark");

    assert!(app.close().is_empty(), "the launch left processes behind");
}
