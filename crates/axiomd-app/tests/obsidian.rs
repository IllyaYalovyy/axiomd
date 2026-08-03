//! UT-009 against the running application: the Obsidian surface a reader can press.
//!
//! Callouts, footnotes and wikilink resolution are rendering, and the render suite
//! pins them. What has to be asserted against the real app is what the reader *does*:
//! following a wikilink, and ticking a task off — which is the one thing a rendered
//! document does to the source it was rendered from (issue #12).

use axiomd_e2e::{App, Fixture};

/// A note with two identical items in it, which is the case that says the toggle is
/// mapped through the span map rather than found by searching the text.
const TASKS: &str = "# Tasks\n\n- [ ] same\n- [ ] same\n- [x] done\n";

/// What the boxes on screen say, as `"  x "`-style text: one character per box, in
/// document order.
fn boxes(app: &App) -> String {
    app.dom(
        "Array.from(document.querySelectorAll('li.task-list-item input[type=checkbox]'))\
         .map((box) => box.checked ? 'x' : ' ').join('')",
    )
}

/// Whether the reader can actually read `text` — in the document *and* on the page.
///
/// `textContent` is not that question: a folded callout's body is in the document and
/// nobody can read it.
fn shown(text: &str) -> String {
    format!(
        "Array.from(document.querySelectorAll('p')).some((p) => \
         p.textContent.includes({text:?}) && p.checkVisibility())"
    )
}

/// The reader presses the `nth` box, counting from zero.
fn press(app: &App, nth: usize) {
    app.click(&format!(
        "li.task-list-item:nth-of-type({}) > a.task-toggle",
        nth + 1
    ));
}

/// The whole of the interactive checkbox ruling: a press in read mode changes the
/// source at the right place, the page shows it, and the reader is still reading.
#[test]
fn pressing_a_task_box_ticks_that_item_off_in_the_source() {
    let fixture = Fixture::new("task-toggle");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", TASKS));
    let loads = app.navigation_count();

    assert_eq!(
        boxes(&app),
        "  x",
        "the document did not start as it was written"
    );

    // What the reader's finger lands on is the link, not the box inside it. A press on
    // the box itself would toggle a checkbox in the page and tell the app nothing, so
    // this is asked of the page's own hit testing rather than of the stylesheet.
    assert_eq!(
        app.dom(
            "(() => { const box = document.querySelector('li.task-list-item input'); \
             const at = box.getBoundingClientRect(); \
             const hit = document.elementFromPoint(at.left + at.width / 2, \
             at.top + at.height / 2); \
             return hit === null ? 'nothing' : hit.className; })()"
        ),
        "task-toggle",
        "pressing a task box does not reach the link that tells the app about it",
    );

    press(&app, 0);
    app.wait_until_source("# Tasks\n\n- [x] same\n- [ ] same\n- [x] done\n");
    app.wait_until(
        "Array.from(document.querySelectorAll('li.task-list-item input')).map((b) => \
         b.checked ? 'x' : ' ').join('') === 'x x'",
    );

    assert_eq!(
        app.mode(),
        "read",
        "pressing a box left the reader in the editor"
    );
    assert!(app.is_modified(), "the press left the document saved");
    assert_eq!(
        app.navigation_count(),
        loads,
        "pressing a box reloaded the page, which costs the reader their place",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Two items that say exactly the same thing are two different boxes. A toggle that
/// found its line by searching the text would tick the first one whichever was pressed.
#[test]
fn two_identical_items_are_told_apart_by_the_one_that_was_pressed() {
    let fixture = Fixture::new("task-twins");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", TASKS));

    press(&app, 1);
    app.wait_until_source("# Tasks\n\n- [ ] same\n- [x] same\n- [x] done\n");

    // And back the other way: a ticked box unticks.
    press(&app, 2);
    app.wait_until_source("# Tasks\n\n- [ ] same\n- [x] same\n- [ ] done\n");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The edit is the reader's own, so it is one step of their undo history — not
/// something that happened to their document behind their back.
#[test]
fn undo_puts_a_ticked_task_back() {
    let fixture = Fixture::new("task-undo");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", TASKS));

    press(&app, 0);
    app.wait_until_source("# Tasks\n\n- [x] same\n- [ ] same\n- [x] done\n");

    app.activate("win.undo");
    app.wait_until_source(TASKS);
    app.wait_until(
        "Array.from(document.querySelectorAll('li.task-list-item input')).map((b) => \
         b.checked ? 'x' : ' ').join('') === '  x'",
    );

    app.activate("win.redo");
    app.wait_until_source("# Tasks\n\n- [x] same\n- [ ] same\n- [x] done\n");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A press is an edit like any other, so the reader's own autosave setting applies to
/// it — no separate path, no separate rule (invariant 14).
#[test]
fn a_ticked_task_is_saved_the_way_every_other_edit_is() {
    let fixture = Fixture::new("task-save");
    let file = fixture.write("notes.md", TASKS);
    let app = axiomd_e2e::launch(&file);

    press(&app, 0);
    app.wait_until_source("# Tasks\n\n- [x] same\n- [ ] same\n- [x] done\n");
    app.activate("win.save");
    app.wait_until_saved();

    assert_eq!(
        std::fs::read_to_string(&file).expect("read the document back"),
        "# Tasks\n\n- [x] same\n- [ ] same\n- [x] done\n",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A resolved wikilink is followed exactly as a relative link is (issue #6): the
/// document opens in the same window, and back works.
#[test]
fn a_wikilink_opens_the_document_it_resolves_to() {
    let fixture = Fixture::new("wikilink-follow");
    let notes = fixture.write(
        "notes.md",
        "# Notes\n\nSee [[guide]] and [[deep/setup|the setup]].\n\n\
         And [[release notes]], whose name has a space in it.\n\n\
         And [[nowhere]], which is nothing.\n",
    );
    fixture.write("guide.md", "# Guide\n\nThe guide.\n");
    fixture.write("deep/setup.md", "# Setup\n\nThe setup.\n");
    fixture.write("release notes.md", "# Release\n\nWhat changed.\n");

    let app = axiomd_e2e::launch(&notes);

    app.click("a.wikilink[href=\"guide.md\"]");
    app.wait_until("document.querySelector('h1').textContent === 'Guide'");
    assert_eq!(app.window_count(), 1, "the wikilink opened a second window");

    app.activate("win.back");
    app.wait_until("document.querySelector('h1').textContent === 'Notes'");

    app.click("a.wikilink[href=\"deep/setup.md\"]");
    app.wait_until("document.querySelector('h1').textContent === 'Setup'");

    // A name with a space in it travels as a link like any other: the page encodes it,
    // the app decodes it, and the reader lands on the document they meant.
    app.activate("win.back");
    app.wait_until("document.querySelector('h1').textContent === 'Notes'");
    app.click("a.wikilink[href=\"release notes.md\"]");
    app.wait_until("document.querySelector('h1').textContent === 'Release'");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A wikilink that resolves to nothing is not a link: the reader can see it, it says
/// what the author wrote, and nothing happens to it.
#[test]
fn an_unresolved_wikilink_is_shown_and_leads_nowhere() {
    let fixture = Fixture::new("wikilink-unresolved");
    let notes = fixture.write("notes.md", "# Notes\n\nSee [[nowhere]] and [[twin]].\n");
    fixture.write("one/twin.md", "# One\n");
    fixture.write("two/twin.md", "# Two\n");

    let app = axiomd_e2e::launch(&notes);

    assert_eq!(
        app.dom(
            "Array.from(document.querySelectorAll('span.wikilink-unresolved'))\
             .map((link) => link.textContent).join('|')"
        ),
        "nowhere|twin",
        "an ambiguous or missing wikilink is not shown as unresolved",
    );
    assert_eq!(
        app.dom("document.querySelectorAll('a.wikilink').length"),
        "0",
        "a wikilink that resolves to nothing is still a link",
    );

    // Pressing one does nothing at all: the reader stays on the document they are on.
    app.click("span.wikilink-unresolved");
    assert_eq!(app.dom_text("h1"), "Notes");
    assert_eq!(app.window_count(), 1);

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The reader's own folder is the root, and a document that appears in it while they
/// are reading resolves at the next render — there is no vault to configure and
/// nothing to reload (`ux_decisions.md`).
#[test]
fn a_document_that_appears_beside_this_one_makes_its_wikilink_resolve() {
    let fixture = Fixture::new("wikilink-appears");
    let notes = fixture.write("notes.md", "# Notes\n\nSee [[later]].\n");
    let app = axiomd_e2e::launch(&notes);

    assert_eq!(
        app.dom("document.querySelectorAll('a.wikilink').length"),
        "0"
    );

    fixture.write("later.md", "# Later\n\nHere now.\n");
    // Anything that renders the document again: the reader typing is the ordinary one.
    app.activate("win.mode");
    app.wait_until_mode("edit");
    app.type_text(" ");
    app.activate("win.mode");
    app.wait_until_mode("read");

    app.wait_until("document.querySelectorAll('a.wikilink[href=\"later.md\"]').length === 1");
    app.click("a.wikilink[href=\"later.md\"]");
    app.wait_until("document.querySelector('h1').textContent === 'Later'");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A foldable callout folds where the reader is, without the page being reloaded and
/// without a line of JavaScript in the document — the browser's own `<details>`.
#[test]
fn a_foldable_callout_opens_and_shuts_where_the_reader_is() {
    let fixture = Fixture::new("callout-fold");
    let app = axiomd_e2e::launch(&fixture.write(
        "notes.md",
        "# Notes\n\n> [!warning]- Shut\n> The hidden part.\n\n> [!tip]+ Open\n> The shown part.\n",
    ));
    let loads = app.navigation_count();

    assert_eq!(
        app.dom(
            "Array.from(document.querySelectorAll('details.callout'))\
             .map((fold) => fold.open ? 'open' : 'shut').join('|')"
        ),
        "shut|open",
        "the callouts did not start the way their authors asked",
    );
    // Folded is hidden, not lost: the words are in the document, and a reader who has
    // not opened it cannot read them — which is also what keeps the search from
    // counting matches nobody can see (`find.js` reads only what is visible).
    assert!(
        app.dom("document.querySelector('details.callout').textContent")
            .contains("The hidden part."),
        "a folded callout lost its body",
    );
    assert_eq!(
        app.dom(&shown("The hidden part.")),
        "false",
        "a callout the author folded shut is showing its body",
    );

    app.click("details.callout > summary.callout-title");
    app.wait_until("document.querySelectorAll('details.callout')[0].open === true");
    app.wait_until(&shown("The hidden part."));

    assert_eq!(
        app.navigation_count(),
        loads,
        "folding a callout reloaded the page",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A footnote reference takes the reader to its definition, and the way back takes
/// them to the reference they came from — the second one, when that is where they were.
#[test]
fn a_footnote_and_its_way_back_are_both_links_that_lead_somewhere() {
    let fixture = Fixture::new("footnote-jump");
    let app = axiomd_e2e::launch(&fixture.write(
        "notes.md",
        "# Notes\n\nA[^one] and again[^one].\n\n[^one]: The definition.\n",
    ));

    assert_eq!(
        app.dom(
            "Array.from(document.querySelectorAll('sup.footnote-ref a'))\
             .map((ref_) => ref_.textContent + '->' + ref_.getAttribute('href')).join('|')"
        ),
        "1->#fn-one|1->#fn-one",
        "the references are not numbered or do not point at the definition",
    );
    // Every way back lands on a reference that is really in the document.
    assert_eq!(
        app.dom(
            "Array.from(document.querySelectorAll('a.footnote-backref'))\
             .every((back) => document.querySelector(back.getAttribute('href')) !== null)"
        ),
        "true",
        "a way back points at a reference that is not there",
    );
    assert_eq!(
        app.dom("document.querySelectorAll('a.footnote-backref').length"),
        "2",
        "a footnote referred to twice has two ways back",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The visual specification of the Obsidian surface: every callout kind, a folded one,
/// a task list and a footnote, on a light desktop.
///
/// Ignored until a human has looked at the picture and pinned it: approving a rendered
/// surface for the first time is theirs to do, not the harness's (`docs/TESTING.md`).
/// To pin it, look at `target/debug/e2e-artifacts/obsidian-light.actual.png` from a
/// failing run and, if it is right, re-run this test with `AXIOMD_PIN_GOLDENS=1` set,
/// then remove the `#[ignore]`.
#[test]
#[ignore = "awaiting the first human visual approval; see the comment above"]
fn the_obsidian_surface_still_looks_the_way_it_was_approved() {
    let fixture = Fixture::new("obsidian-golden-light");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", &every_obsidian_construct()));

    app.screenshot().assert_matches("obsidian-light");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The same on a dark desktop, which is a different drawing of the same document: the
/// callout colours and their icons both follow the palette.
#[test]
#[ignore = "awaiting the first human visual approval; see the comment above"]
fn the_obsidian_surface_in_the_dark_still_looks_the_way_it_was_approved() {
    let fixture = Fixture::new("obsidian-golden-dark");
    let dark = axiomd_e2e::Preferences::with("obsidian-golden-dark", "theme", "'dark'");
    let app = axiomd_e2e::launch_with(
        &fixture.write("notes.md", &every_obsidian_construct()),
        &dark,
    );

    app.screenshot().assert_matches("obsidian-dark");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The fixture both screenshot goldens are taken of: one of everything issue #12 adds.
fn every_obsidian_construct() -> String {
    let mut note = String::from("# Obsidian\n\n");
    for kind in [
        "note", "abstract", "info", "todo", "tip", "success", "question", "warning", "failure",
        "danger", "bug", "example", "quote", "nonsense",
    ] {
        note.push_str(&format!("> [!{kind}]\n> A {kind} callout.\n\n"));
    }
    note.push_str(
        "> [!warning]- Folded shut\n> Hidden until opened.\n\n\
         > [!tip]+ Folded open\n> Shown until closed.\n\n\
         > [!note] Outer\n> Text.\n>\n> > [!bug] Inner\n> > Nested.\n\n\
         - [ ] not done\n- [x] done\n  - [ ] nested\n\n\
         A footnote[^a] and another[^b].\n\n\
         [^a]: The first definition.\n\n[^b]: The second definition.\n",
    );
    note
}
