//! UT-009 (math): `$inline$` and `$$display$$` are typeset equations (issue #11).
//!
//! The pipeline's half of this is asserted in `axiomd-render`, where the MathML is
//! pinned byte for byte. What is asserted here is the half only a running WebKitGTK
//! can answer: that the markup really is laid out as mathematics, in the face the
//! application carries, without a script and without a fetch — and that a formula the
//! renderer could not read still leaves the reader everything else.

mod support;

use axiomd_e2e::{App, Fixture, Preferences};
use support::Origin;

/// The switch the reader turns, as preferences names it.
const ROW: &str = "Math";

/// The equations on the page.
const EQUATIONS: &str = "document.querySelectorAll('article.markdown math').length";

/// Whether the page is asking for the plugin's styling.
const STYLESHEET: &str =
    "document.querySelectorAll('link[href=\"axiomd://assets/plugin/math/math.css\"]').length";

/// Whether the face the application carries reached this page and was accepted. A
/// `loaded` status is the whole chain: the `@font-face` was parsed, the content policy
/// let the request through, the `axiomd://` handler answered it, and WebKit read the
/// bytes as a font.
const FONT_LOADED: &str = "Array.from(document.fonts).some(face => face.family === 'axiomd-math' \
     && face.status === 'loaded')";

/// Every test that measures an equation waits for this first, and none of them sleeps
/// to do it: the face is declared `font-display: block`, so until it has loaded the
/// glyphs are not drawn and every box inside a `<math>` measures zero. Waiting on the
/// equations alone made those measurements a race (seen: `mfrac` at 0×0 pixels on a
/// page whose MathML was already complete).
fn showing_equations(app: &App, count: usize) {
    app.wait_until(&format!("{EQUATIONS} === {count}"));
    app.wait_until(FONT_LOADED);
}

/// A document with one of everything the issue's corpus names.
fn corpus() -> String {
    String::from(
        "# Equations\n\n\
         The area of a circle is $A = \\pi r^2$, near enough.\n\n\
         $$\n\\int_0^\\infty e^{-x^2} \\, dx = \\frac{\\sqrt{\\pi}}{2}\n$$\n\n\
         $$\n\\sum_{k=1}^{n} k = \\frac{n(n+1)}{2}\n$$\n\n\
         $$\n\\begin{aligned} (a + b)^2 &= (a + b)(a + b) \\\\ &= a^2 + 2ab + b^2 \
         \\end{aligned}\n$$\n\n\
         $$\n\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}\n$$\n\n\
         Greek and scripts: $\\alpha_{i}^{2} + \\Gamma \\leq \\sum_{k=1}^{n} k$.\n",
    )
}

/// Where a block equation's contents sit inside the box they are drawn in, as
/// `(space to the left, space to the right)` — which is how "centred" is measured
/// without asking the stylesheet what it says.
fn margins_around(app: &App, nth: usize) -> (i32, i32) {
    let script = format!(
        "(() => {{ const m = document.querySelectorAll('math[display=\"block\"]')[{nth}]; \
         const box = m.getBoundingClientRect(); \
         const drawn = m.firstElementChild.getBoundingClientRect(); \
         return `${{Math.round(drawn.left - box.left)}} ${{Math.round(box.right - drawn.right)}}`; \
         }})()"
    );
    let measured = app.dom(&script);
    let mut sides = measured.split_whitespace();
    let left = sides.next().and_then(|n| n.parse().ok());
    let right = sides.next().and_then(|n| n.parse().ok());
    match (left, right) {
        (Some(left), Some(right)) => (left, right),
        _ => panic!("block equation {nth} is not on the page: {measured:?}"),
    }
}

fn scroll_offset(app: &App) -> i32 {
    app.dom("Math.round(document.scrollingElement.scrollTop)")
        .parse()
        .expect("a scroll offset")
}

/// Every equation in the corpus is laid out as mathematics rather than shown as its
/// source: a fraction has a bar with something above and below it, a sum has its
/// limits over and under, and a matrix is a grid.
#[test]
fn every_equation_in_a_document_is_typeset() {
    let fixture = Fixture::new("math-corpus");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", &corpus()));

    showing_equations(&app, 6);
    assert_eq!(
        app.dom("getComputedStyle(document.querySelector('math')).display"),
        "inline",
        "an inline equation is not laid out in the line it was written in",
    );
    assert_eq!(
        app.dom("getComputedStyle(document.querySelector('math[display=\"block\"]')).display"),
        "block",
        "a display equation is not a block of its own",
    );

    // A fraction is two dimensional: the numerator is above the denominator, and both
    // are inside the box the fraction draws.
    assert_eq!(
        app.dom(
            "(() => { const f = document.querySelector('mfrac'); \
             const [over, under] = f.children; \
             return String(over.getBoundingClientRect().bottom \
             <= under.getBoundingClientRect().top \
             && f.getBoundingClientRect().height > over.getBoundingClientRect().height); })()"
        ),
        "true",
        "the fraction is not drawn as a fraction",
    );
    // A sum carries its limits above and below the sign, which is what display style
    // means and what an inline fallback could not produce.
    assert_eq!(
        app.dom(
            "(() => { const s = document.querySelector('munderover'); \
             const [sign, under, over] = s.children; \
             return String(over.getBoundingClientRect().bottom <= sign.getBoundingClientRect().top \
             && under.getBoundingClientRect().top >= sign.getBoundingClientRect().bottom); })()"
        ),
        "true",
        "the sum's limits are not above and below it",
    );
    // The matrix is a grid of two rows and two columns, drawn between two brackets.
    assert_eq!(
        app.dom(
            "(() => { const rows = document.querySelectorAll('mtable.menv-arraylike mtr'); \
             return `${rows.length}x${rows[0].children.length}`; })()"
        ),
        "2x2",
    );
    // And nowhere on the page is the LaTeX itself standing where an equation should be.
    assert_eq!(
        app.dom("document.querySelectorAll('article.markdown .math').length"),
        "0",
        "the source is still showing where the equation should be",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The equations are set in the face the application carries, which is what makes two
/// machines show one document the same way — and which arrives through the app's own
/// scheme, under a policy that admits nothing else.
#[test]
fn equations_are_set_in_the_face_the_application_carries() {
    let fixture = Fixture::new("math-font");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", &corpus()));

    app.wait_until(FONT_LOADED);
    assert_eq!(
        app.dom("getComputedStyle(document.querySelector('math')).fontFamily"),
        "axiomd-math, math",
        "an equation is set in whatever the machine happened to have",
    );
    assert_eq!(app.dom(STYLESHEET), "1");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// An equation is written in the ink the reader is reading in, and follows them when
/// the desktop changes its mind — without the document being parsed or loaded again
/// (invariant 9).
#[test]
fn an_equation_follows_the_desktop_from_light_to_dark() {
    let ink = "getComputedStyle(document.querySelector('mi')).color";
    let fixture = Fixture::new("math-theme");
    let preferences = Preferences::new("math-theme");
    let app = axiomd_e2e::launch_with(
        &fixture.write("notes.md", "# Notes\n\nAn equation: $a + b$.\n"),
        &preferences,
    );

    showing_equations(&app, 1);
    let light = app.dom(ink);
    assert_eq!(
        light, "rgba(0, 0, 6, 0.8)",
        "the light ink is not the page's"
    );
    let loads = app.navigation_count();
    let renders = app.render_count();

    app.activate("app.preferences");
    app.set_preference("Theme", "Dark");
    app.wait_until(&format!("({ink}) !== '{light}'"));

    assert_eq!(
        app.dom(ink),
        "rgba(255, 255, 255, 0.9)",
        "the equation stayed in the light palette",
    );
    assert_eq!(
        app.navigation_count(),
        loads,
        "changing the theme reloaded the page",
    );
    assert_eq!(
        app.render_count(),
        renders,
        "changing the theme rendered the document again",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A display equation is centred, and an equation wider than the measure scrolls where
/// it stands rather than making the whole document scroll sideways.
#[test]
fn a_display_equation_is_centred_and_a_wide_one_scrolls_in_place() {
    let fixture = Fixture::new("math-width");
    let wide = (1..=24)
        .map(|n| format!("a_{{{n}}}"))
        .collect::<Vec<_>>()
        .join(" & ");
    let document = format!(
        "# Wide\n\n$$\n\\frac{{a}}{{b}}\n$$\n\n$$\n\\begin{{pmatrix}} {wide} \\end{{pmatrix}}\n$$\n"
    );
    let app = axiomd_e2e::launch(&fixture.write("notes.md", &document));

    showing_equations(&app, 2);

    let (left, right) = margins_around(&app, 0);
    assert!(
        left > 0 && (left - right).abs() <= 2,
        "the equation is not centred: {left}px to its left, {right}px to its right",
    );

    // The wide one is wider than the column it is in, and that is the column's
    // problem — not the document's.
    assert_eq!(
        app.dom(
            "(() => { const p = document.querySelectorAll('math[display=\"block\"]')[1]\
             .parentElement; return String(p.scrollWidth > p.clientWidth); })()"
        ),
        "true",
        "the wide equation did not overflow the block it stands in, so nothing is being \
         asserted about how it scrolls",
    );
    assert_eq!(
        app.dom(
            "String(document.scrollingElement.scrollWidth \
             <= document.scrollingElement.clientWidth)"
        ),
        "true",
        "a wide equation made the whole document scroll sideways",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// An inline equation is a word among words: it sits on the line it was written in
/// and does not push the lines around it apart.
#[test]
fn an_inline_equation_sits_on_the_line_it_was_written_in() {
    let fixture = Fixture::new("math-inline");
    let app = axiomd_e2e::launch(&fixture.write(
        "notes.md",
        "# Inline\n\nA line of prose with x in it.\n\nA line of prose with $x$ in it.\n",
    ));

    showing_equations(&app, 1);
    assert_eq!(
        app.dom(
            "(() => { const [plain, withMath] = document.querySelectorAll('article.markdown p'); \
             return String(Math.round(plain.getBoundingClientRect().height) \
             === Math.round(withMath.getBoundingClientRect().height)); })()"
        ),
        "true",
        "an inline equation changed the height of the line it is in",
    );
    // And it is on the same baseline as the words beside it: the equation's box sits
    // inside the line box rather than beside it.
    assert_eq!(
        app.dom(
            "(() => { const p = document.querySelectorAll('article.markdown p')[1]; \
             const m = p.querySelector('math').getBoundingClientRect(); \
             const line = p.getBoundingClientRect(); \
             return String(m.top >= line.top && m.bottom <= line.bottom); })()"
        ),
        "true",
        "the inline equation is not inside the line it was written in",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Typesetting asks the network for nothing — not for the library, which is compiled
/// in, not for the face, which is compiled in, and not for anything a document says.
#[test]
fn typesetting_reaches_the_network_for_nothing() {
    let origin = Origin::start();
    let fixture = Fixture::new("math-egress");
    let document = format!(
        "# Notes\n\n{corpus}\n\nAnd a formula that names a server: \
         $\\text{{{url}}}$ and $\\href{{{url}}}{{link}}$.\n\n\
         <math display=\"block\"><mglyph src=\"{png}\"></mglyph>\
         <mtext>{url}</mtext></math>\n",
        corpus = corpus(),
        url = origin.url("/latex"),
        png = origin.url("/glyph.png"),
    );
    let app = axiomd_e2e::launch(&fixture.write("notes.md", &document));

    app.wait_until(FONT_LOADED);
    assert_eq!(
        origin.requests(),
        Vec::<String>::new(),
        "an equation reached the network",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The reason MathML was chosen: a page full of equations still runs nothing at all.
#[test]
fn a_page_of_equations_runs_no_script() {
    let fixture = Fixture::new("math-inert");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", &corpus()));

    app.wait_until(&format!("{EQUATIONS} === 6"));
    assert_eq!(
        app.dom("document.querySelectorAll('script').length"),
        "0",
        "a script reached a document that only has equations in it",
    );
    assert_eq!(
        app.dom(
            "String(document.querySelector('meta[http-equiv=\"Content-Security-Policy\"]')\
             .content)"
        ),
        "default-src 'none'; img-src axiomd:; style-src axiomd:; font-src axiomd:; \
         base-uri 'none'; form-action 'none'",
        "the page carrying equations is displayed under a looser policy than any other",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// LaTeX the renderer cannot read costs the reader that formula and nothing else:
/// their own source stands where they wrote it, marked, with the reason beside it,
/// and every other equation on the page is still an equation.
#[test]
fn unreadable_latex_is_marked_where_it_stands_and_the_rest_of_the_page_survives() {
    let fixture = Fixture::new("math-error");
    let app = axiomd_e2e::launch(&fixture.write(
        "notes.md",
        "# Notes\n\nGood: $a + b$.\n\nBroken: $a^b^c$ here.\n\nAfter.\n",
    ));

    app.wait_until(&format!("{EQUATIONS} === 1"));
    assert_eq!(
        app.dom_text("article.markdown .math-error-source"),
        "a^b^c",
        "the reader lost the source of a formula nobody could read",
    );
    assert_eq!(
        app.dom_text("article.markdown .math-error-reason"),
        "trying to add a superscript twice to the same element",
    );
    assert_eq!(
        app.dom("String(document.querySelector('.math-error').checkVisibility())"),
        "true",
        "the marked source is in the document but not on the page",
    );
    // The document around it is whole, and no dialog interrupted the reader
    // (`ux_decisions.md`).
    assert_eq!(app.dom_text("article.markdown h1"), "Notes");
    assert_eq!(
        app.dom("document.querySelectorAll('article.markdown p')[2].textContent"),
        "After.",
    );
    assert_eq!(app.visible_dialog(), "");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The source an equation carries is for the document, not for the reader: a search
/// finds the words they can see and never the LaTeX underneath them.
#[test]
fn the_source_an_equation_carries_is_not_something_the_search_finds() {
    let fixture = Fixture::new("math-search");
    let app = axiomd_e2e::launch(&fixture.write(
        "notes.md",
        "# Notes\n\nThe integral $\\int_0^1 x \\, dx$ is a half.\n\nThe integral again.\n",
    ));

    app.wait_until(&format!("{EQUATIONS} === 1"));
    app.activate("win.find");

    app.search_for("integral");
    app.wait_until_counter("1 of 2");

    app.search_for("\\int_0");
    app.wait_until_counter("No results");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// An equation in a heading is still a heading: the sidebar names the section by the
/// source the author wrote, which is what it named it by before equations were
/// typeset at all.
#[test]
fn an_equation_in_a_heading_still_names_its_section() {
    let fixture = Fixture::new("math-outline");
    let app = axiomd_e2e::launch(&fixture.write(
        "notes.md",
        "# Notes\n\nText.\n\n## The limit $\\lim_{n \\to \\infty} a_n$\n\nMore text.\n",
    ));

    app.wait_until(&format!("{EQUATIONS} === 1"));
    let outline = app.outline();

    assert!(outline.shown, "the sidebar is not beside the document");
    assert_eq!(
        outline.headings,
        vec![
            "h1 Notes".to_owned(),
            "h2 The limit \\lim_{n \\to \\infty} a_n".to_owned(),
        ],
        "the section with an equation in its title lost its name",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The whole of the toggle: the reader gets their LaTeX back where they stand, the
/// page is never loaded again, and their place is kept (invariants 5 and 14).
#[test]
fn switching_math_off_gives_the_reader_their_latex_back_where_they_stand() {
    let fixture = Fixture::new("math-toggle");
    let preferences = Preferences::new("math-toggle");
    let mut document = String::from("# Notes\n\n");
    for paragraph in 1..=120 {
        document.push_str(&format!("Paragraph {paragraph}.\n\n"));
    }
    document.push_str("Einstein wrote $E = mc^2$ on a board.\n");
    let app = axiomd_e2e::launch_with(&fixture.write("notes.md", &document), &preferences);

    app.wait_until(&format!("{EQUATIONS} === 1"));

    // The reader is looking at paragraph 40, well above the equation.
    app.dom("document.querySelector('[data-line=\"81\"]').scrollIntoView(true)");
    let scrolled = scroll_offset(&app);
    assert!(scrolled > 0, "the document did not scroll");
    let loads = app.navigation_count();

    app.activate("app.preferences");
    assert_eq!(app.preference(ROW), "true", "math is not on by default");
    app.set_preference(ROW, "false");
    app.wait_until(&format!("{EQUATIONS} === 0"));

    assert_eq!(
        app.dom("document.querySelector('article.markdown p:last-of-type').textContent"),
        "Einstein wrote E = mc^2 on a board.",
        "the reader did not get the source they wrote back",
    );
    assert_eq!(app.dom(STYLESHEET), "0", "the styling stayed behind");
    preferences.wait_until("disabled-plugins", "['math']");
    assert_eq!(
        app.navigation_count(),
        loads,
        "switching the capability reloaded the page",
    );
    assert_eq!(scroll_offset(&app), scrolled, "the reader lost their place");

    // And back on again, with the styling and the face it needs.
    app.set_preference(ROW, "true");
    app.wait_until(&format!("{EQUATIONS} === 1"));
    app.wait_until(FONT_LOADED);
    assert_eq!(
        app.navigation_count(),
        loads,
        "coming back reloaded the page"
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The visual specification of the corpus on a light desktop.
///
/// Ignored until a human has looked at the picture and pinned it: approving a rendered
/// surface for the first time is theirs to do, not the harness's (`docs/TESTING.md`).
/// To pin it, look at `target/debug/e2e-artifacts/math-light.actual.png` from a failing
/// run and, if it is right, re-run this test with `AXIOMD_PIN_GOLDENS=1` set, then
/// remove the `#[ignore]`.
#[test]
#[ignore = "awaiting the first human visual approval; see the comment above"]
fn the_math_corpus_still_looks_the_way_it_was_approved_in_light() {
    let fixture = Fixture::new("math-golden-light");
    let app = axiomd_e2e::launch(&fixture.write("notes.md", &corpus()));

    app.wait_until(FONT_LOADED);
    app.screenshot().assert_matches("math-light");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The same corpus on a dark one, which is the picture that says an equation follows
/// the reader's palette rather than staying black on white.
///
/// Pinned the same way, from `math-dark.actual.png`.
#[test]
#[ignore = "awaiting the first human visual approval; see the comment above"]
fn the_math_corpus_still_looks_the_way_it_was_approved_in_dark() {
    let fixture = Fixture::new("math-golden-dark");
    let dark = Preferences::with("math-golden-dark", "theme", "'dark'");
    let app = axiomd_e2e::launch_with(&fixture.write("notes.md", &corpus()), &dark);

    app.wait_until(FONT_LOADED);
    app.screenshot().assert_matches("math-dark");

    assert!(app.close().is_empty(), "the launch left processes behind");
}
