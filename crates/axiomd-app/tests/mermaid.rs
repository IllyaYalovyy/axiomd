//! UT-009 (diagrams): a ```` ```mermaid ```` fence is a picture (issue #13).
//!
//! Every test here drives the shipped binary on a headless compositor and reads the
//! result out of the page the reader is looking at. A drawn diagram lives in a shadow
//! root attached to its block, so the assertions reach through `shadowRoot` — which is
//! also how a test can tell a diagram that was *drawn* from one that is still the
//! source the author wrote.
//!
//! The document is not made less inert by any of this. It is still displayed with
//! scripting off and under `default-src 'none'`; what draws is a file compiled into the
//! application, run beside the document in a world of the app's own, on the documents
//! that have a diagram in them and no others — which is what most of this file is
//! about.

mod support;

use axiomd_e2e::{App, Fixture, Preferences};
use support::Origin;

/// The switch the reader turns, as preferences names it.
const ROW: &str = "Mermaid diagrams";

/// Every diagram block in the document, drawn or not.
const BLOCKS: &str = "document.querySelectorAll('article.markdown div.plugin-mermaid').length";

/// How many of them the reader is looking at as pictures.
const DRAWN: &str = "Array.from(document.querySelectorAll('article.markdown div.plugin-mermaid')) \
     .filter(block => block.shadowRoot !== null && block.shadowRoot.querySelector('svg') !== null) \
     .length";

/// Whether the library reached this page at all.
const LIBRARY: &str = "String(document.documentElement.dataset.axiomdMermaid)";

/// Whether the page is asking for the plugin's styling.
const STYLESHEET: &str =
    "document.querySelectorAll('link[href=\"axiomd://assets/plugin/mermaid/mermaid.css\"]').length";

/// A `mermaid` fence holding `source`.
fn fence(source: &str) -> String {
    format!("```mermaid\n{source}\n```\n\n")
}

/// The diagram types the tiny build draws, one of each — the common set the issue
/// names, plus the three beyond it that this build turned out to know.
fn every_common_type() -> String {
    let mut document = String::from("# Diagrams\n\n");
    for source in [
        "flowchart TD\n  A[Start] --> B{Choice}\n  B -->|yes| C[Do]\n  B -->|no| D[Skip]",
        "sequenceDiagram\n  Alice->>Bob: Hello\n  Bob-->>Alice: Hi",
        "classDiagram\n  Animal <|-- Duck\n  Animal : +int age",
        "stateDiagram-v2\n  [*] --> Still\n  Still --> Moving",
        "erDiagram\n  CUSTOMER ||--o{ ORDER : places",
        "gantt\n  title A schedule\n  dateFormat YYYY-MM-DD\n  section One\n  Task :a1, 2024-01-01, 30d",
        "pie title Pets\n  \"Dogs\" : 386\n  \"Cats\" : 85",
        "journey\n  title My day\n  section Go\n    Wake: 5: Me",
        "gitGraph\n  commit\n  branch dev\n  commit",
    ] {
        document.push_str(&fence(source));
    }
    document
}

/// The fill the reader sees on a flowchart node — the one number that says which
/// palette a diagram was drawn in, and that it was styled at all.
fn node_fill(app: &App) -> String {
    app.dom(
        "(() => { const root = document.querySelector('div.plugin-mermaid').shadowRoot; \
         const node = root.querySelector('.node rect, .node path, .node polygon'); \
         return node === null ? 'undrawn' : getComputedStyle(node).fill; })()",
    )
}

/// The three-diagram document the issue asks for, drawn: three fences in, three
/// pictures out, each of them a diagram rather than an empty frame.
#[test]
fn every_diagram_in_a_document_is_drawn_as_a_picture() {
    let fixture = Fixture::new("mermaid-three");
    let document = format!(
        "# Notes\n\n{}{}{}",
        fence("flowchart TD\n  A[Start] --> B[Stop]"),
        fence("sequenceDiagram\n  Alice->>Bob: Hello"),
        fence("pie title Pets\n  \"Dogs\" : 386\n  \"Cats\" : 85"),
    );
    let app = axiomd_e2e::launch(&fixture.write("notes.md", &document));

    app.wait_until(&format!("{DRAWN} === 3"));

    assert_eq!(app.dom(BLOCKS), "3");
    assert_eq!(
        app.dom(STYLESHEET),
        "1",
        "the diagram styling did not arrive"
    );
    // Not empty frames: the first diagram has the two boxes its source names, and the
    // reader can read what is written in them.
    assert_eq!(
        app.dom(
            "document.querySelector('div.plugin-mermaid').shadowRoot \
             .querySelectorAll('.node').length"
        ),
        "2",
    );
    let labels = app.dom("document.querySelector('div.plugin-mermaid').shadowRoot.textContent");
    assert!(
        labels.contains("Start") && labels.contains("Stop"),
        "{labels}"
    );
    // And the styling the library wrote came with it, through the one road the page's
    // policy leaves open.
    assert_ne!(node_fill(&app), "undrawn");
    assert_eq!(
        node_fill(&app),
        "rgb(236, 236, 255)",
        "the diagram is unstyled"
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The common diagram set, verified against the running application rather than
/// against a list somebody wrote down.
#[test]
fn the_common_diagram_types_all_draw() {
    let fixture = Fixture::new("mermaid-types");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", &every_common_type()));

    app.wait_until(&format!("{DRAWN} === 9"));

    assert_eq!(app.dom(BLOCKS), "9");
    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A type this build does not know is not a blank box either: the reader keeps the
/// source and is told, beside it, that it could not be drawn.
#[test]
fn a_diagram_type_this_build_does_not_know_keeps_its_source_and_says_so() {
    let fixture = Fixture::new("mermaid-unknown");
    let document = format!(
        "# Notes\n\n{}{}",
        fence("mindmap\n  root((mind))\n    first\n    second"),
        fence("flowchart TD\n  A --> B"),
    );
    let app = axiomd_e2e::launch(&fixture.write("notes.md", &document));

    // The one that can be drawn is drawn: a capability that fails on one block loses
    // that block and nothing else (invariant 13).
    app.wait_until(&format!("{DRAWN} === 1"));
    app.wait_until(&badge_of(0));

    let badge = app.dom(&format!("({}) ?? ''", badge_text(0)));
    assert!(
        badge.starts_with("Mermaid diagrams could not draw this diagram:"),
        "the badge said {badge:?}",
    );
    assert!(badge.contains("No diagram type detected"), "{badge}");
    // And the source is still there to read, not hidden behind the message.
    assert_eq!(
        app.dom(
            "document.querySelectorAll('div.plugin-mermaid')[0] \
             .querySelector('code').checkVisibility()"
        ),
        "true",
        "the reader lost the source of a diagram that could not be drawn",
    );
    assert!(
        app.dom("document.querySelectorAll('div.plugin-mermaid')[0].textContent")
            .contains("root((mind))"),
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A diagram the library cannot parse, in a document that is otherwise unaffected.
#[test]
fn a_diagram_that_cannot_be_parsed_keeps_its_source_and_says_why() {
    let fixture = Fixture::new("mermaid-broken");
    let document = format!(
        "# Notes\n\n{}{}\nAfter.\n",
        fence("flowchart TD\n  A --> ->> B"),
        fence("flowchart TD\n  A --> B"),
    );
    let app = axiomd_e2e::launch(&fixture.write("notes.md", &document));

    app.wait_until(&format!("{DRAWN} === 1"));
    app.wait_until(&badge_of(0));

    let badge = app.dom(&format!("({}) ?? ''", badge_text(0)));
    assert!(
        badge.starts_with("Mermaid diagrams could not draw this diagram: Parse error"),
        "the badge said {badge:?}",
    );
    // The rest of the document is exactly what it would have been.
    assert_eq!(app.dom_text("h1"), "Notes");
    assert_eq!(
        app.dom("document.querySelector('article.markdown > p').textContent"),
        "After.",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The whole of "lazily": a diagram far below the page is not in the document at all
/// until the reader comes to it, and then it is.
#[test]
fn a_diagram_below_the_page_is_not_drawn_until_the_reader_comes_to_it() {
    let fixture = Fixture::new("mermaid-lazy");
    let mut document = format!("# Notes\n\n{}", fence("flowchart TD\n  A --> B"));
    for paragraph in 1..=200 {
        document.push_str(&format!("Paragraph {paragraph}.\n\n"));
    }
    document.push_str(&fence("flowchart TD\n  Far --> Below"));
    let app = axiomd_e2e::launch(&fixture.write("notes.md", &document));

    // The one at the top is drawn, which is what says the library is running and has
    // had its chance at the other one.
    app.wait_until(&format!("{DRAWN} === 1"));
    assert_eq!(app.dom(BLOCKS), "2");
    assert_eq!(
        app.dom("String(document.querySelectorAll('div.plugin-mermaid')[1].shadowRoot)"),
        "null",
        "a diagram the reader cannot see was drawn anyway",
    );

    app.dom("document.querySelectorAll('div.plugin-mermaid')[1].scrollIntoView(true); 'scrolled'");
    app.wait_until(&format!("{DRAWN} === 2"));
    assert!(
        app.dom("document.querySelectorAll('div.plugin-mermaid')[1].shadowRoot.textContent")
            .contains("Below"),
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The other half of "only when it is needed": a document with no diagram in it never
/// loads the library, and the same window loading one that does proves the absence is
/// a decision rather than a race.
#[test]
fn a_document_with_no_diagram_never_loads_the_library() {
    let fixture = Fixture::new("mermaid-absent");
    let plain = fixture.write("plain.md", "# Plain\n\n```rust\nfn main() {}\n```\n");
    let drawn = fixture.write(
        "drawn.md",
        &format!("# Drawn\n\n{}", fence("flowchart TD\n  A --> B")),
    );
    let app = axiomd_e2e::launch(&plain);

    settled(&app, "Plain", 0);
    assert_eq!(app.dom(LIBRARY), "undefined");
    assert_eq!(app.dom(STYLESHEET), "0");
    assert_eq!(app.dom(BLOCKS), "0", "an ordinary fence was claimed");

    // The same window, a document that does need it: the library arrives.
    app.open_here(&drawn);
    app.wait_until(&format!("{DRAWN} === 1"));
    assert_eq!(app.dom(LIBRARY), "drawing");

    // And back to the one that does not: a page load leaves nothing behind.
    let reported = app.section_reports();
    app.open_here(&plain);
    settled(&app, "Plain", reported);
    assert_eq!(
        app.dom(LIBRARY),
        "undefined",
        "the library followed a document that has no diagram in it",
    );
    assert_eq!(app.dom(STYLESHEET), "0");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Drawing a diagram asks the network for nothing — not for the library, which is
/// compiled in, and not for anything a hostile diagram names.
#[test]
fn drawing_a_diagram_asks_the_network_for_nothing() {
    let origin = Origin::start();
    let fixture = Fixture::new("mermaid-egress");
    let document = format!(
        "# Notes\n\n{}{}",
        fence(&format!(
            "flowchart TD\n  A[\"<img src='{}'>\"] --> B[\"<a href='{}'>link</a>\"]\n  \
             click A \"{}\" _blank",
            origin.url("/label.png"),
            origin.url("/anchor"),
            origin.url("/click"),
        )),
        fence(&format!(
            "sequenceDiagram\n  Alice->>Bob: {}",
            origin.url("/message"),
        )),
    );
    let app = axiomd_e2e::launch(&fixture.write("notes.md", &document));

    app.wait_until(&format!("{DRAWN} === 2"));
    // Clicking what the diagram offered as a click target, in case the library kept it.
    app.dom(
        "(() => { const root = document.querySelector('div.plugin-mermaid').shadowRoot; \
         for (const node of root.querySelectorAll('.node, a, [class*=clickable]')) { \
           node.dispatchEvent(new MouseEvent('click', {bubbles: true})); } return 'clicked'; })()",
    );
    app.wait_until(&format!("{DRAWN} === 2"));

    assert_eq!(
        origin.requests(),
        Vec::<String>::new(),
        "a diagram reached the network",
    );
    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A diagram is drawn in the palette the reader is reading in, and follows them when
/// the desktop changes its mind — without the document being parsed or loaded again
/// (invariant 9).
#[test]
fn a_diagram_follows_the_desktop_from_light_to_dark() {
    let fixture = Fixture::new("mermaid-theme");
    let preferences = Preferences::new("mermaid-theme");
    let app = axiomd_e2e::launch_with(
        &fixture.write(
            "notes.md",
            &format!("# Notes\n\n{}", fence("flowchart TD\n  A --> B")),
        ),
        &preferences,
    );

    app.wait_until(&format!("{DRAWN} === 1"));
    assert_eq!(node_fill(&app), "rgb(236, 236, 255)");
    let loads = app.navigation_count();
    let renders = app.render_count();

    app.activate("app.preferences");
    app.set_preference("Theme", "Dark");
    app.wait_until(&format!(
        "({}) !== 'rgb(236, 236, 255)'",
        node_fill_script()
    ));

    assert_eq!(
        node_fill(&app),
        "rgb(31, 32, 32)",
        "the diagram did not move to the dark palette",
    );
    assert_eq!(
        app.navigation_count(),
        loads,
        "changing the theme reloaded the page",
    );
    assert_eq!(
        app.render_count(),
        renders,
        "changing the theme re-rendered the document",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A document that changes under the reader keeps the diagrams it had already drawn:
/// the block's own markup never changed, so the patch has nothing to replace.
#[test]
fn a_drawn_diagram_survives_the_document_changing_underneath_it() {
    let fixture = Fixture::new("mermaid-reload");
    let document = fixture.write(
        "notes.md",
        &format!("# Notes\n\n{}\nFirst.\n", fence("flowchart TD\n  A --> B")),
    );
    let app = axiomd_e2e::launch(&document);

    app.wait_until(&format!("{DRAWN} === 1"));
    let loads = app.navigation_count();
    let drawn_at = app.dom(
        "String(document.querySelector('div.plugin-mermaid').shadowRoot.querySelector('svg').id)",
    );

    std::fs::write(
        &document,
        format!("# Notes\n\n{}\nSecond.\n", fence("flowchart TD\n  A --> B")),
    )
    .expect("save the document");
    app.wait_until(
        "Array.from(document.querySelectorAll('article.markdown > p')) \
         .some(block => block.textContent === 'Second.')",
    );

    assert_eq!(
        app.dom(DRAWN),
        "1",
        "the reader's diagram went back to source"
    );
    assert_eq!(
        app.dom("String(document.querySelector('div.plugin-mermaid').shadowRoot.querySelector('svg').id)"),
        drawn_at,
        "the diagram was drawn again for an edit that did not touch it",
    );
    assert_eq!(app.navigation_count(), loads, "the page was loaded again");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A search counts what the reader can read. The source a drawn diagram keeps is not
/// on the page — they are looking at a picture — so it is not among the matches.
#[test]
fn a_search_does_not_count_the_source_under_a_drawn_diagram() {
    let fixture = Fixture::new("mermaid-search");
    let document = format!(
        "# Notes\n\n{}\nA paragraph naming Bob once.\n",
        fence("sequenceDiagram\n  Alice->>Bob: Hello"),
    );
    let app = axiomd_e2e::launch(&fixture.write("notes.md", &document));

    app.wait_until(&format!("{DRAWN} === 1"));
    app.activate("win.find");
    app.search_for("Bob");
    app.wait_until_counter("1 of 1");

    assert_eq!(
        app.dom("document.querySelectorAll('mark.axiomd-find').length"),
        "1",
        "the search marked words the reader cannot see",
    );
    assert_eq!(
        app.dom(
            "document.querySelector('mark.axiomd-find').closest('div.plugin-mermaid') === null"
        ),
        "true",
        "the one mark is inside the diagram's hidden source",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Switching the capability off gives the reader the source they wrote, where they
/// stand, without the page being loaded again (invariants 5 and 14).
#[test]
fn switching_diagrams_off_gives_the_reader_the_source_back() {
    let fixture = Fixture::new("mermaid-toggle");
    let preferences = Preferences::new("mermaid-toggle");
    let app = axiomd_e2e::launch_with(
        &fixture.write(
            "notes.md",
            &format!("# Notes\n\n{}", fence("flowchart TD\n  A --> B")),
        ),
        &preferences,
    );

    app.wait_until(&format!("{DRAWN} === 1"));
    let loads = app.navigation_count();

    app.activate("app.preferences");
    app.set_preference(ROW, "false");
    app.wait_until(&format!("{BLOCKS} === 0"));

    assert_eq!(
        app.dom("document.querySelector('article.markdown pre.sy-code code').textContent"),
        "flowchart TD\n  A --> B\n",
        "the reader did not get the source they wrote back",
    );
    assert_eq!(app.dom(STYLESHEET), "0", "the styling stayed behind");
    assert_eq!(
        app.navigation_count(),
        loads,
        "switching it off reloaded the page"
    );
    preferences.wait_until("disabled-plugins", "['mermaid']");

    // And back on again: the same page, drawing again.
    app.set_preference(ROW, "true");
    app.wait_until(&format!("{DRAWN} === 1"));
    assert_eq!(
        app.navigation_count(),
        loads,
        "switching it on reloaded the page"
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The visual specification of a drawn diagram on a light desktop.
///
/// Ignored until a human has looked at the picture and pinned it: approving a rendered
/// surface for the first time is theirs to do, not the harness's (`docs/TESTING.md`).
/// To pin it, look at `target/debug/e2e-artifacts/mermaid-light.actual.png` from a
/// failing run and, if it is right, re-run this test with `AXIOMD_PIN_GOLDENS=1` set,
/// then remove the `#[ignore]`.
#[test]
#[ignore = "awaiting the first human visual approval; see the comment above"]
fn a_drawn_diagram_still_looks_the_way_it_was_approved() {
    let fixture = Fixture::new("mermaid-golden-light");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", &every_common_type()));

    app.wait_until(&format!("{DRAWN} === 9"));
    app.screenshot().assert_matches("mermaid-light");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The same diagrams on a dark desktop, which is a different drawing and not merely a
/// different background. Pinned the same way, from `mermaid-dark.actual.png`.
#[test]
#[ignore = "awaiting the first human visual approval; see the comment above"]
fn a_drawn_diagram_in_the_dark_still_looks_the_way_it_was_approved() {
    let fixture = Fixture::new("mermaid-golden-dark");
    let dark = Preferences::with("mermaid-golden-dark", "theme", "'dark'");
    let app = axiomd_e2e::launch_with(&fixture.write("notes.md", &every_common_type()), &dark);

    app.wait_until(&format!("{DRAWN} === 9"));
    app.screenshot().assert_matches("mermaid-dark");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Waits until the document headed `title` is on screen *and* the app has finished
/// with it — which is what an assertion about something the page does **not** have
/// needs, and what waiting for the heading alone would not give.
///
/// The page says which section the reader is in at the end of every update, after the
/// blocks are patched and after anything the document needs run has been run. So one
/// more of those reports than there were before is the app saying it is done, and an
/// absence asserted after it is an absence rather than a race.
///
/// `reported` is how many reports the window had made *before* the document was asked
/// for, and it has to be read before asking: a page says where the reader is once, on
/// its own, as soon as it is watched (`track.js`), and nothing but scrolling makes it
/// say so again. A baseline read after the document arrived would therefore be waiting
/// for a second report that is never coming — which is what made this suite fail one
/// run in three under load.
fn settled(app: &App, title: &str, reported: u32) {
    app.wait_until(&format!(
        "document.querySelector('h1').textContent === {title:?}"
    ));
    app.wait_for("the page to say where the reader is", || {
        app.section_reports() > reported
    });
}

/// The badge beside the `nth` diagram, as a condition to wait for.
fn badge_of(nth: usize) -> String {
    format!("({}) !== undefined", badge_text(nth))
}

/// What that badge says, as an expression.
fn badge_text(nth: usize) -> String {
    format!(
        "((document.querySelectorAll('div.plugin-mermaid')[{nth}].shadowRoot ?? {{}}) \
         .querySelector?.('.plugin-badge') ?? {{}}).textContent"
    )
}

/// The node fill, as an expression a wait can be written on.
fn node_fill_script() -> &'static str {
    "(() => { const root = document.querySelector('div.plugin-mermaid').shadowRoot; \
     const node = root === null ? null : root.querySelector('.node rect, .node path, .node polygon'); \
     return node === null ? 'undrawn' : getComputedStyle(node).fill; })()"
}
