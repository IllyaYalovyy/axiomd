//! UT-006: finding a word in the document, in both ways of looking at it (issue #8).
//!
//! Every test here drives the shipped binary on a headless compositor and reads the
//! search back the way the reader sees it: the counter beside the entry, the marks in
//! the rendered page, the highlights in the source, where the caret ended up. The
//! counter is asserted everywhere because it is the one thing a search can silently lie
//! about — a highlight the reader can see is either there or not, but a count that is
//! one out looks exactly like a count that is right.
//!
//! The document below is chosen so that the two surfaces honestly disagree. Reading, a
//! link is the word it shows; editing, it is the word plus the address behind it. So
//! `needle` is in the page five times and in the source six, and `example.com` is
//! nowhere on the page and once in the source — which is what "search the rendered text,
//! not the markup" means when it is true.

use axiomd_e2e::{App, Fixture, Preferences};

/// The test document. Five `needle`s to read, six to edit.
const DOCUMENT: &str = "\
# Search

The needle is here.

A paragraph mentioning needle twice: needle.

Also [needle](https://example.com/needle) as a link.

## Needle section

Nothing else here.
";

/// How many marks the rendered page is showing.
fn marks(app: &App) -> usize {
    app.dom("String(document.querySelectorAll('mark.axiomd-find').length)")
        .parse()
        .expect("a number of marks")
}

/// Which of them is the current one, counting from zero in document order, or `-1` when
/// none is — what the reader sees as the one highlight that looks different.
fn current_mark(app: &App) -> i32 {
    app.dom(
        "String(Array.from(document.querySelectorAll('mark.axiomd-find')) \
         .findIndex(mark => mark.classList.contains('current')))",
    )
    .parse()
    .expect("a mark position")
}

/// Opens the search and looks for `text`, exactly as `Ctrl+F` and typing do.
fn search(app: &App, text: &str) {
    app.activate("win.find");
    app.search_for(text);
}

/// The whole of UT-006's first two steps: `Ctrl+F`, a word, and the document says how
/// many of them there are and shows the reader the first.
#[test]
fn searching_marks_every_occurrence_and_counts_them() {
    let fixture = Fixture::new("search-counts");
    let app = axiomd_e2e::launch(&fixture.write("guide.md", DOCUMENT));
    assert!(
        !app.search().shown,
        "the bar was up before it was asked for"
    );

    search(&app, "needle");

    app.wait_until_counter("1 of 5");
    let bar = app.search();
    assert!(bar.shown, "the search bar never came up");
    assert_eq!(bar.query, "needle");
    assert!(!bar.cased, "case was being matched without being asked for");
    assert_eq!(
        marks(&app),
        5,
        "the page is not showing the number of matches the counter claims",
    );
    assert_eq!(
        current_mark(&app),
        0,
        "the first match was not the one shown"
    );
    // The heading's `Needle` is one of the five: case is ignored until it is asked for.
    assert_eq!(
        app.dom(
            "Array.from(document.querySelectorAll('mark.axiomd-find')) \
             .map(mark => mark.textContent).join(',')"
        ),
        "needle,needle,needle,needle,Needle",
    );
    // Opening a document and reading it is never interrupted by a question, and asking
    // to search it is not either (invariant 12).
    assert_eq!(app.visible_dialog(), "");
    assert_eq!(app.navigation_count(), 1, "searching reloaded the page");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Walking the matches: in document order, and round the ends in both directions, with
/// the bar saying so when it has just carried the reader past one.
#[test]
fn the_matches_are_walked_in_document_order_and_wrap_at_both_ends() {
    let fixture = Fixture::new("search-cycles");
    let app = axiomd_e2e::launch(&fixture.write("guide.md", DOCUMENT));

    search(&app, "needle");
    app.wait_until_counter("1 of 5");

    for expected in 2..=5 {
        app.activate("win.find-next");
        app.wait_until_counter(&format!("{expected} of 5"));
        assert_eq!(
            current_mark(&app),
            expected - 1,
            "the {expected}th match of five was not the one highlighted",
        );
        assert_eq!(
            app.search().wrap,
            "",
            "the bar claimed a wrap that never happened",
        );
    }

    // Past the end, and round to the top — where the reader is told, because a counter
    // that goes back to 1 without a word is a counter that looks broken.
    app.activate("win.find-next");
    app.wait_until_counter("1 of 5");
    assert_eq!(current_mark(&app), 0);
    assert_eq!(app.search().wrap, "Wrapped to the top");

    // And back the other way, round to the bottom.
    app.activate("win.find-previous");
    app.wait_until_counter("5 of 5");
    assert_eq!(current_mark(&app), 4);
    assert_eq!(app.search().wrap, "Wrapped to the bottom");

    app.activate("win.find-previous");
    app.wait_until_counter("4 of 5");
    assert_eq!(
        app.search().wrap,
        "",
        "the wrap was still being reported a step later",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Case is ignored until the reader presses the toggle, and then it is not.
///
/// The non-happy path lives here too: a search that matches nothing says so rather than
/// silently showing a counter of one.
#[test]
fn the_case_toggle_narrows_the_search_to_what_the_reader_typed() {
    let fixture = Fixture::new("search-case");
    let app = axiomd_e2e::launch(&fixture.write("guide.md", DOCUMENT));

    search(&app, "NEEDLE");
    app.wait_until_counter("1 of 5");
    assert_eq!(marks(&app), 5);

    app.press("Aa");

    app.wait_until_counter("No results");
    assert!(app.search().cased, "the case toggle did not stay pressed");
    assert_eq!(
        marks(&app),
        0,
        "a search with no matches still marked the document",
    );

    // The same document, the same toggle, a query that does occur as typed: the
    // heading's `Needle` is the one of the five that drops out.
    app.search_for("needle");
    app.wait_until_counter("1 of 4");
    assert_eq!(marks(&app), 4);

    app.press("Aa");
    app.wait_until_counter("1 of 5");
    assert!(!app.search().cased);

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Closing takes the search back out of the document, down to the text nodes it split —
/// the page is the page it was before anybody searched it.
#[test]
fn closing_the_search_leaves_no_trace_of_it_in_the_document() {
    let fixture = Fixture::new("search-close");
    let app = axiomd_e2e::launch(&fixture.write("guide.md", DOCUMENT));
    let before = app.dom("document.querySelector('article.markdown').innerHTML");

    search(&app, "needle");
    app.wait_until_counter("1 of 5");
    assert_eq!(marks(&app), 5);

    // What Escape runs, and what the bar's own close button runs.
    app.activate("win.find-close");

    app.wait_for("the marks to go", || marks(&app) == 0);
    let bar = app.search();
    assert!(!bar.shown, "the bar stayed up");
    assert_eq!(bar.counter, "", "the counter was still counting");
    assert_eq!(
        app.dom("document.querySelector('article.markdown').innerHTML"),
        before,
        "the document did not come back to what it was before the search",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The owner's ruling, and the point of the whole feature: search always works, and in
/// edit mode it searches the source with the same bar, the same counter and the same
/// keys.
#[test]
fn the_same_bar_searches_the_source_when_the_reader_is_editing() {
    let fixture = Fixture::new("search-editing");
    let app = axiomd_e2e::launch(&fixture.write("guide.md", DOCUMENT));

    search(&app, "needle");
    app.wait_until_counter("1 of 5");

    app.activate("win.mode");
    app.wait_until_mode("edit");

    // Six, not five: the address behind the link is source the reader can edit.
    app.wait_until_counter("1 of 6");
    let bar = app.search();
    assert!(bar.shown, "the bar was lost on the way to the editor");
    assert_eq!(bar.query, "needle");
    assert_eq!(
        app.source_highlights(),
        [">needle", "needle", "needle", "needle", "needle", "Needle"],
        "the source is not showing what the counter is counting",
    );
    // Where the mode switch put them, and not where the search would have: switching
    // modes owns the reader's place (invariant 5), and the caret is at the top of the
    // document they were reading from the top of.
    assert_eq!(
        app.caret_line(),
        1,
        "the search moved the reader instead of the mode switch",
    );
    // The page they left is not still marked up behind them.
    app.wait_for("the page they left to lose its marks", || marks(&app) == 0);

    for (step, line) in [(2, 5), (3, 5), (4, 7), (5, 7), (6, 9)] {
        app.activate("win.find-next");
        app.wait_until_counter(&format!("{step} of 6"));
        assert_eq!(
            app.caret_line(),
            line,
            "the {step}th match of six is not on source line {line}",
        );
    }
    assert_eq!(
        app.source_highlights().last().map(String::as_str),
        Some(">Needle"),
        "the last match was not the highlighted one",
    );

    app.activate("win.find-next");
    app.wait_until_counter("1 of 6");
    assert_eq!(app.search().wrap, "Wrapped to the top");
    assert_eq!(app.caret_line(), 3);

    // Back to reading, and the count is the page's again. Coming back re-renders the
    // document, so the page settles into its marks rather than having them already —
    // waited for rather than slept past.
    app.activate("win.mode");
    app.wait_until_mode("read");
    app.wait_until_counter("1 of 5");
    app.wait_for("the page to show its five matches", || marks(&app) == 5);
    assert_eq!(app.search().counter, "1 of 5");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// What "the rendered text, not the markup" means when it is true: an address that is
/// only in the source is unfindable on the page and findable in the editor.
#[test]
fn the_page_is_searched_by_its_words_and_the_source_by_its_markup() {
    let fixture = Fixture::new("search-markup");
    let app = axiomd_e2e::launch(&fixture.write("guide.md", DOCUMENT));

    search(&app, "example.com");
    app.wait_until_counter("No results");
    assert_eq!(marks(&app), 0);

    app.activate("win.mode");
    app.wait_until_mode("edit");

    app.wait_until_counter("1 of 1");
    assert_eq!(app.source_highlights(), [">example.com"]);

    // A document with exactly one match: walking is a wrap onto the same match, and it
    // still takes the reader to it — source line 7, where the link is written.
    app.activate("win.find-next");
    app.wait_for("the reader to be taken to the match", || {
        app.caret_line() == 7
    });
    assert_eq!(app.search().counter, "1 of 1");
    assert_eq!(app.search().wrap, "Wrapped to the top");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Live reload (#5) meets the search: the file gains a match under the reader, the
/// counter follows it, and the page is still the one page it has always been.
///
/// This is also what keeps the patch honest. The marks stand inside the blocks the
/// patch compares, so a search that did not take them off first would have every marked
/// paragraph rebuilt as changed — and a search that did not put them back would leave
/// the reader counting matches that are no longer highlighted.
#[test]
fn a_search_left_open_follows_the_document_when_it_changes_underneath() {
    let fixture = Fixture::new("search-reload");
    let document = fixture.write("guide.md", DOCUMENT);
    let app = axiomd_e2e::launch(&document);

    search(&app, "needle");
    app.wait_until_counter("1 of 5");

    std::fs::write(
        &document,
        DOCUMENT.replace(
            "Nothing else here.\n",
            "Nothing else here.\n\nOne more needle, added later.\n",
        ),
    )
    .expect("save the document");

    app.wait_until_counter("1 of 6");
    assert_eq!(
        marks(&app),
        6,
        "the page is not showing the number of matches the counter claims",
    );
    assert_eq!(
        current_mark(&app),
        0,
        "the reader's place in the search was lost when the document changed",
    );
    assert!(
        app.dom("document.querySelector('article.markdown').textContent")
            .contains("One more needle, added later."),
        "the document did not follow the file",
    );
    assert_eq!(
        app.navigation_count(),
        1,
        "the document was reloaded rather than patched",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A word the document does not have says so, and says it beside the entry rather than
/// in a dialog.
#[test]
fn a_word_the_document_does_not_have_says_so_and_marks_nothing() {
    let fixture = Fixture::new("search-nothing");
    let app = axiomd_e2e::launch(&fixture.write("guide.md", DOCUMENT));

    search(&app, "zebra");

    app.wait_until_counter("No results");
    assert_eq!(marks(&app), 0);
    assert_eq!(app.visible_dialog(), "");
    // And walking matches that do not exist does nothing rather than something wrong.
    app.activate("win.find-next");
    assert_eq!(app.search().counter, "No results");

    // Emptying the entry is not a failed search: there is simply nothing to count.
    app.search_for("");
    app.wait_for("the counter to go quiet", || {
        app.search().counter.is_empty()
    });
    assert_eq!(marks(&app), 0);

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The highlight has to be visible in both colour schemes and the current match has to
/// be told from the others — the exit criterion, asserted as the reader's own eyes would
/// settle it: the colours actually painted on the page.
#[test]
fn the_marks_stand_out_from_the_page_in_light_and_in_dark() {
    let fixture = Fixture::new("search-colours");
    let document = fixture.write("guide.md", DOCUMENT);

    for scheme in ["light", "dark"] {
        let preferences = Preferences::with("search-colours", "theme", &format!("'{scheme}'"));
        let app = axiomd_e2e::launch_with(&document, &preferences);

        search(&app, "needle");
        app.wait_until_counter("1 of 5");

        let colour = |selector: &str| {
            app.dom(&format!(
                "getComputedStyle(document.querySelector({selector:?})).backgroundColor"
            ))
        };
        let page = colour("body");
        let other = colour("mark.axiomd-find:not(.current)");
        let current = colour("mark.axiomd-find.current");

        assert_ne!(
            other, page,
            "in {scheme}, a match is the colour of the page"
        );
        assert_ne!(
            current, other,
            "in {scheme}, the match the counter is on looks like every other match",
        );
        assert_ne!(current, page);
        // And the words are still the words: a highlight that hid its text would be
        // worse than none.
        assert_eq!(app.dom_text("mark.axiomd-find"), "needle");

        assert!(app.close().is_empty(), "the launch left processes behind");
    }
}

/// The bar belongs to the document in front of the reader. Following a link to another
/// one puts it away rather than leaving a count of a document nobody is looking at.
#[test]
fn opening_another_document_puts_the_search_away() {
    let fixture = Fixture::new("search-another");
    let app = axiomd_e2e::launch(&fixture.write("guide.md", DOCUMENT));
    let other = fixture.write("other.md", "# Other\n\nNo such word here.\n");

    search(&app, "needle");
    app.wait_until_counter("1 of 5");

    app.open_here(&other);

    app.wait_for("the search to be put away", || !app.search().shown);
    assert_eq!(app.search().counter, "");
    assert_eq!(marks(&app), 0);
    // The bar is put away the moment the window is given another document, which is
    // before that document's page has finished arriving — so the page is waited for
    // rather than assumed to be there. Under load it is not, and this assertion read
    // the document being left.
    app.wait_until("document.querySelector('h1').textContent === 'Other'");
    assert_eq!(app.dom_text("h1"), "Other");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A search open while the reader types costs them nothing per keystroke (issue #18's
/// budget, met again here).
///
/// Searching a document is work proportional to the document, and this one is a
/// megabyte. If every key press paid for a scan of it, three hundred of them would be
/// minutes rather than the seconds the harness allows — and what the reader would feel
/// is an editor that stutters whenever the bar is open. The count still has to be right
/// once they stop, which is the other half of the assertion.
#[test]
fn typing_with_the_search_open_still_costs_a_keystroke() {
    let fixture = Fixture::new("search-latency");
    let mut source = String::from("# Large\n\n");
    while source.len() < 1_000_000 {
        source.push_str(
            "A paragraph of a large document, with `code` and *emphasis* in it, long \
             enough that a thousand of them add up to something worth searching.\n\n",
        );
    }
    let paragraphs = source.matches("A paragraph").count();
    let app = axiomd_e2e::launch(&fixture.write("large.md", &source));

    app.activate("win.mode");
    app.wait_until_mode("edit");
    search(&app, "paragraph");
    app.wait_until_counter(&format!("1 of {paragraphs}"));

    // At the top of the buffer, which is where the caret starts, so the typing below
    // lands before every one of the matches.
    app.place_caret(1);
    const KEYSTROKES: usize = 300;
    let started = std::time::Instant::now();
    for _ in 0..KEYSTROKES {
        app.type_text("x");
    }
    let typing = started.elapsed();

    assert_eq!(
        app.source().chars().take_while(|key| *key == 'x').count(),
        KEYSTROKES,
        "keystrokes were lost between the keyboard and the buffer",
    );
    assert!(
        typing < std::time::Duration::from_secs(10),
        "{KEYSTROKES} keystrokes with the search open took {typing:?}, so typing is \
         paying for searching",
    );
    // And the search is right again once they stop: the document still holds every
    // match it held, and the reader is still on the first of them.
    app.wait_until_counter(&format!("1 of {paragraphs}"));

    assert!(app.close().is_empty(), "the launch left processes behind");
}
