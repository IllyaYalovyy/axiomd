//! Issue #21: what the source looks like while the reader is editing it.
//!
//! Every test here drives the shipped binary on a headless compositor and reads the
//! editor back the way the reader sees it — how a piece of the source is coloured,
//! which of its words are underlined as misspelled — with the page's load and render
//! counts beside it. Highlighting is a way of *drawing* the buffer and nothing else:
//! it never rewrites the source, never reloads the document and never re-renders the
//! page, which is the same rule the rendered document's own restyling is held to
//! (invariant 9).
//!
//! The markup the fixture below carries is the minimum the owner asked for: a
//! heading, an emphasis, a code span. There is deliberately nothing about live
//! preview or inline rendering here — the ruling on #21 is that the editor shows
//! markup, plainly.

use std::path::Path;

use axiomd_e2e::{App, ContextMenu, Fixture, Preferences};

/// The document every test opens: one heading, one emphasis, one code span, one
/// paragraph of plain prose, and one word nobody can spell.
const NOTES: &str = "\
# Chapter one

Plain words, then *important* emphasis, then `printf` in a code span.

A sentance of prose to read.
";

/// A word in the heading, in the emphasis, in the code span, and in plain prose —
/// each unique in the document, so asking how it is drawn asks about that one place.
const HEADING: &str = "Chapter one";
const EMPHASIS: &str = "important";
const CODE: &str = "printf";
const PROSE: &str = "Plain words";

/// Opens the fixture and puts the reader in edit mode, which is the only mode any of
/// this is about.
fn editing(label: &str, preferences: &Preferences) -> (Fixture, App) {
    let fixture = Fixture::new(label);
    let app = axiomd_e2e::launch_with(&fixture.write("notes.md", NOTES), preferences);
    app.activate("win.mode");
    app.wait_until_mode("edit");
    (fixture, app)
}

/// The whole of the highlighting the owner asked for: markup is drawn differently
/// from prose, and prose is drawn plainly.
///
/// The values are asserted as a reader would describe them — bold, coloured, slanted
/// — rather than as the exact colours of GtkSourceView's Adwaita scheme, which are
/// that project's to change and not a promise axiomd makes.
#[test]
fn the_markup_in_the_source_is_drawn_apart_from_the_prose() {
    let preferences = Preferences::new("editor-highlighting");
    let (_fixture, app) = editing("editor-highlighting", &preferences);

    assert_eq!(
        app.source_style(PROSE),
        "",
        "plain prose is drawn as something other than the editor's own ink",
    );

    let heading = app.source_style(HEADING);
    assert!(
        heading.contains("weight=bold") && heading.contains("colour=#"),
        "a heading is not drawn apart from the prose around it: {heading:?}",
    );

    assert_eq!(
        app.source_style(EMPHASIS),
        "slant=italic",
        "an emphasis is not slanted in the source",
    );

    let code = app.source_style(CODE);
    assert!(
        code.contains("colour=#"),
        "a code span is not drawn apart from the prose around it: {code:?}",
    );
    assert_ne!(
        code, heading,
        "a code span and a heading are drawn identically, so neither says what it is",
    );

    // Drawing is all it is. The reader's document is the bytes they opened.
    assert_eq!(
        app.source(),
        NOTES,
        "highlighting the source changed the source",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The reader turning the lights out: the source is repainted where it stands, and
/// the document behind it is neither reloaded nor rendered again (invariant 9).
#[test]
fn the_source_takes_the_dark_palette_without_being_loaded_again() {
    let preferences = Preferences::new("editor-dark");
    let (_fixture, app) = editing("editor-dark", &preferences);

    app.place_caret(3);
    let light = app.source_style(HEADING);
    assert!(
        light.contains("colour=#"),
        "the heading was not coloured on a light desktop: {light:?}",
    );
    let loads = app.navigation_count();
    let pages = app.render_count();

    app.activate("app.preferences");
    app.set_preference("Theme", "Dark");
    app.wait_for("the source to take the dark palette", || {
        app.source_style(HEADING) != light
    });

    let dark = app.source_style(HEADING);
    assert!(
        dark.contains("weight=bold") && dark.contains("colour=#"),
        "the dark palette left the heading undrawn: {dark:?}",
    );
    assert_eq!(
        app.navigation_count(),
        loads,
        "changing the palette reloaded the document",
    );
    assert_eq!(
        app.render_count(),
        pages,
        "changing the palette rendered the document again",
    );
    assert_eq!(
        app.source(),
        NOTES,
        "changing the palette changed the source"
    );
    assert_eq!(
        app.caret_line(),
        3,
        "changing the palette moved the reader's caret",
    );

    // And back, so the reader who tries it and changes their mind is where they were.
    app.set_preference("Theme", "System");
    app.wait_for("the source to take the light palette again", || {
        app.source_style(HEADING) == light
    });

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The word nobody can spell, and one the checker has no quarrel with.
const MISSPELT: &str = "sentance";

/// Spell checking, which is the reader's to switch off and the editor's alone: it
/// marks what they are typing, and never what they are reading.
#[test]
fn misspelled_words_are_underlined_while_the_reader_is_editing_and_never_while_reading() {
    let fixture = Fixture::new("editor-spelling");
    let preferences = Preferences::new("editor-spelling");
    let app = axiomd_e2e::launch_with(&fixture.write("notes.md", NOTES), &preferences);

    assert_eq!(
        app.mode(),
        "read",
        "the document did not open in reading mode",
    );
    assert_eq!(
        app.misspelled(),
        Vec::<String>::new(),
        "a document being read is being spell checked",
    );

    app.activate("win.mode");
    app.wait_until_mode("edit");
    app.wait_for("the misspelt word to be marked", || {
        !app.misspelled().is_empty()
    });
    assert_eq!(
        app.misspelled(),
        vec![MISSPELT.to_owned()],
        "the editor marked something other than the one word that is misspelt",
    );
    assert!(
        app.source_style(MISSPELT).contains("underline="),
        "the misspelt word is not underlined where the reader would see it: {:?}",
        app.source_style(MISSPELT),
    );
    assert_eq!(
        app.source(),
        NOTES,
        "spell checking the source changed the source",
    );

    // And back to reading: the marks are the editor's, and reading is untouched.
    app.activate("win.mode");
    app.wait_until_mode("read");
    app.wait_for("the marks to come off with the editor", || {
        app.misspelled().is_empty()
    });

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A document of `characters`, misspelt word first, then prose the checker has
/// nothing to say about.
fn document_of(characters: usize) -> String {
    let mut source = format!("# Chapter one\n\nA {MISSPELT} of prose to read.\n\n");
    while source.len() < characters {
        source.push_str("The quick brown fox jumps over the lazy dog.\n\n");
    }
    source
}

/// The one document spell checking will not touch: one too long for checking it to be
/// free.
///
/// Checking is the only thing the editor does whose cost is the whole document's
/// rather than the screen's, and past about a megabyte it is paid out of a main loop
/// that has stopped answering (the measurements are in `editor.rs`). So a very long
/// document is highlighted, edited and searched like any other and simply not
/// checked — and one just under the mark is checked exactly as any other is, which is
/// the half of this that stops the rule from quietly swallowing every document.
#[test]
fn a_document_too_long_to_check_without_stalling_is_not_checked() {
    let fixture = Fixture::new("editor-spelling-long");
    let modest = fixture.write("modest.md", &document_of(900_000));
    let enormous = fixture.write("enormous.md", &document_of(1_100_000));
    let preferences = Preferences::new("editor-spelling-long");

    // One window and both documents, so that "not checked" is read in a window that
    // has just been seen checking: the same process, the same buffer, the same
    // preference, and nothing different but the length.
    let app = axiomd_e2e::launch_with(&modest, &preferences);

    // What makes "nothing was marked" mean something rather than "nothing was marked
    // yet": one keystroke, and the page catching up with it. That is the debounce, a
    // parse and render of a megabyte on the worker, and the patch — far longer than
    // the tenth of a second libspelling waits before it checks (its own
    // INVALIDATE_DELAY_MSECS, read at 0.4.9). The document under the mark is marked
    // within exactly this barrier, three lines below, which is what says the barrier
    // is long enough.
    let edit_until_the_page_catches_up = |what: &str| {
        app.activate("win.mode");
        app.wait_until_mode("edit");
        let pages = app.render_count();
        app.type_text("\n");
        app.wait_for(
            &format!("the page to catch up with an edit to {what}"),
            || app.render_count() > pages,
        );
    };

    app.wait_until_source(&document_of(900_000));
    edit_until_the_page_catches_up("a 900 KB document");
    app.wait_for(
        "the misspelt word in a 900 KB document to be marked",
        || app.misspelled() == vec![MISSPELT.to_owned()],
    );

    // Saved before the next document takes over the window, so that opening it is the
    // ordinary path and not the one about unsaved work.
    app.activate("win.save");
    app.wait_until_saved();
    app.open_here(&enormous);
    app.wait_until_source(&document_of(1_100_000));
    edit_until_the_page_catches_up("a 1.1 MB document");
    assert_eq!(
        app.misspelled(),
        Vec::<String>::new(),
        "a document too long to check without stalling the application was checked",
    );

    // Back under the mark, in the same window: the marks come back, so what stopped
    // them was the length and nothing else.
    app.activate("win.save");
    app.wait_until_saved();
    app.open_here(&modest);
    app.activate("win.mode");
    app.wait_until_mode("edit");
    app.wait_for("the marks to come back with a document that fits", || {
        app.misspelled() == vec![MISSPELT.to_owned()]
    });

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The preferences row behind it (#20), turned while the reader is typing: the marks
/// go, and come back, without anything being reloaded (invariant 14).
#[test]
fn the_reader_can_switch_spell_checking_off_while_they_are_typing() {
    let preferences = Preferences::new("editor-spelling-off");
    let (_fixture, app) = editing("editor-spelling-off", &preferences);
    app.wait_for("the misspelt word to be marked", || {
        !app.misspelled().is_empty()
    });
    let loads = app.navigation_count();

    app.activate("app.preferences");
    app.set_preference("Check Spelling", "false");
    app.wait_for("the marks to come off the source", || {
        app.misspelled().is_empty()
    });
    preferences.wait_until("spellcheck", "false");
    assert_eq!(
        app.navigation_count(),
        loads,
        "switching spell checking off reloaded the document",
    );

    // And on again, for the reader who changes their mind.
    app.set_preference("Check Spelling", "true");
    app.wait_for("the marks to come back", || {
        app.misspelled() == vec![MISSPELT.to_owned()]
    });

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A search match inside highlighted markup is still a search match: the reader sees
/// the found word marked, not the syntax colour it happens to sit under.
///
/// The one interaction the two ways of colouring the buffer have — both are text tags
/// over the same text, and whichever is applied last is the one on top.
#[test]
fn a_match_found_in_a_heading_is_marked_over_its_highlighting() {
    let preferences = Preferences::new("editor-find-over-syntax");
    let (_fixture, app) = editing("editor-find-over-syntax", &preferences);

    let heading = app.source_style(HEADING);

    app.activate("win.find");
    app.search_for("Chapter");
    app.wait_until_counter("1 of 1");

    let marked = app.source_style("Chapter");
    assert!(
        marked.contains("behind=#"),
        "the match in the heading is not marked at all: {marked:?}",
    );
    assert_ne!(
        marked.split(' ').find(|part| part.starts_with("colour=")),
        heading.split(' ').find(|part| part.starts_with("colour=")),
        "the heading's own colour is painted over the mark, so the reader cannot \
         tell the found word from the rest of the heading: {marked:?}",
    );

    // And closing the search puts the heading back the way it was drawn.
    app.activate("win.find-close");
    app.wait_for("the mark to come back off the source", || {
        app.source_style(HEADING) == heading
    });

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// What the word nobody can spell should have been.
const CORRECTED: &str = "sentence";

/// A word in the fixture the checker has no quarrel with.
const SPELT: &str = "prose";

/// The keys a reader opens the source's context menu with: GtkTextView's own
/// `menu.popup` bindings.
const CONTEXT_MENU_KEYS: [&str; 2] = ["<Shift>F10", "Menu"];

/// The rows of a context menu that are libspelling's opinion about the word under the
/// caret: its corrections, Add to Dictionary and Ignore.
fn spelling_rows(menu: &ContextMenu) -> Vec<(&str, &str)> {
    menu.offers()
        .into_iter()
        .filter(|(_, action)| {
            action.starts_with("spelling.correct")
                || ["spelling.add", "spelling.ignore"].contains(action)
        })
        .collect()
}

/// Opens the source's context menu from the keyboard with the caret inside `word`,
/// and answers what it offers.
fn menu_on(app: &App, word: &str, key: &str) -> ContextMenu {
    app.caret_on(word);
    assert!(
        app.press_key(key),
        "{key} did not open the source's context menu"
    );
    let menu = app.context_menu();
    assert!(menu.open, "{key} left the context menu closed: {menu:?}");
    menu
}

/// Issue #59: the underline was only half of it. The menu the reader opens on a
/// misspelt word offers what it could be instead, and the two things they can do about
/// a word that is spelt the way they meant.
#[test]
fn the_context_menu_on_a_misspelt_word_offers_its_corrections() {
    let preferences = Preferences::new("editor-suggestions");
    let (_fixture, app) = editing("editor-suggestions", &preferences);
    app.wait_for("the misspelt word to be marked", || {
        app.misspelled() == vec![MISSPELT.to_owned()]
    });

    for key in CONTEXT_MENU_KEYS {
        let menu = menu_on(&app, MISSPELT, key);
        let offered = menu.offers();
        assert!(
            offered.contains(&(CORRECTED, "spelling.correct::sentence")),
            "{key} on {MISSPELT:?} did not offer {CORRECTED:?}: {offered:?}",
        );
        let corrections = offered
            .iter()
            .filter(|(_, action)| action.starts_with("spelling.correct::"))
            .count();
        assert!(
            (1..=5).contains(&corrections),
            "libspelling offers at most five corrections, and this offered {corrections}: \
             {offered:?}",
        );
        for wanted in [
            ("Add to Dictionary", "spelling.add"),
            ("Ignore", "spelling.ignore"),
            ("Languages", ""),
        ] {
            assert!(
                offered.contains(&wanted),
                "{key} on {MISSPELT:?} did not offer {wanted:?}: {offered:?}",
            );
        }
        // Closed by a row that changes nothing but the selection, so the next key is
        // opening the menu rather than finding it already open.
        app.press("Select All");
        assert!(
            !app.context_menu().open,
            "pressing a row left the menu open"
        );
    }

    assert_eq!(app.source(), NOTES, "opening the menu changed the source");
    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Choosing a correction is an edit like any other: the word is replaced, the underline
/// goes with it, the document is unsaved, and one undo puts back exactly what was there.
#[test]
fn choosing_a_correction_replaces_the_word_as_one_step_of_undo() {
    let preferences = Preferences::new("editor-correct");
    let (_fixture, app) = editing("editor-correct", &preferences);
    app.wait_for("the misspelt word to be marked", || {
        app.misspelled() == vec![MISSPELT.to_owned()]
    });
    assert!(
        !app.is_modified(),
        "the document was unsaved before anything was chosen"
    );

    menu_on(&app, MISSPELT, "<Shift>F10");
    app.press(CORRECTED);

    let corrected = NOTES.replace(MISSPELT, CORRECTED);
    app.wait_until_source(&corrected);
    assert!(
        !app.context_menu().open,
        "choosing a correction left the menu open"
    );
    app.wait_for("the underline to go with the misspelling", || {
        app.misspelled().is_empty()
    });
    assert!(
        app.is_modified(),
        "a corrected document does not say it is unsaved"
    );

    assert!(app.press_key("<Control>z"), "Ctrl+Z did nothing");
    app.wait_until_source(NOTES);
    app.wait_for("the underline to come back with the misspelling", || {
        app.misspelled() == vec![MISSPELT.to_owned()]
    });

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The menu is about the word it opens on, and never the last word the caret was on:
/// libspelling learns which word that is a tenth of a second after the caret moves, and
/// a reader opens the menu in the same moment as they click.
#[test]
fn the_context_menu_never_offers_corrections_for_a_word_the_caret_has_left() {
    let preferences = Preferences::new("editor-suggestions-fresh");
    let (_fixture, app) = editing("editor-suggestions-fresh", &preferences);
    app.wait_for("the misspelt word to be marked", || {
        app.misspelled() == vec![MISSPELT.to_owned()]
    });

    let misspelt = menu_on(&app, MISSPELT, "<Shift>F10");
    assert!(
        !spelling_rows(&misspelt).is_empty(),
        "nothing was offered for {MISSPELT:?}, so there is nothing to go stale: {misspelt:?}",
    );
    app.press("Select All");

    // Straight on to a word spelt right, with nothing in between to let libspelling
    // catch up.
    let spelt = menu_on(&app, SPELT, "<Shift>F10");
    assert_eq!(
        spelling_rows(&spelt),
        Vec::<(&str, &str)>::new(),
        "the menu on {SPELT:?} offered what it offered for {MISSPELT:?}",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Every file under `folder`, with what it holds, so a dictionary can be compared before
/// and after — empty when there is no such folder.
fn contents_of(folder: &Path) -> Vec<(std::path::PathBuf, String)> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut found: Vec<_> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .map(|path| {
            let held =
                String::from_utf8_lossy(&std::fs::read(&path).unwrap_or_default()).into_owned();
            (path, held)
        })
        .collect();
    found.sort();
    found
}

/// The developer's own enchant dictionaries — the ones a test must never write a word
/// into.
fn the_developers_dictionaries() -> Vec<(std::path::PathBuf, String)> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".config")))
        .expect("a home to find the developer's configuration in");
    contents_of(&config.join("enchant"))
}

/// Add to Dictionary is the reader's for good: the word stops being marked, and it is
/// still known the next time they open axiomd — in their own personal dictionary, which
/// for a launch under test is the store's and never the developer's.
#[test]
fn a_word_added_to_the_dictionary_is_still_known_after_a_relaunch() {
    let developers = the_developers_dictionaries();
    let preferences = Preferences::new("editor-spelling-add");
    let (fixture, app) = editing("editor-spelling-add", &preferences);
    app.wait_for("the misspelt word to be marked", || {
        app.misspelled() == vec![MISSPELT.to_owned()]
    });

    menu_on(&app, MISSPELT, "<Shift>F10");
    app.press("Add to Dictionary");
    app.wait_for("the added word to stop being marked", || {
        app.misspelled().is_empty()
    });
    assert_eq!(
        app.source(),
        NOTES,
        "adding a word to the dictionary changed the source"
    );
    assert!(app.close().is_empty(), "the launch left processes behind");

    let kept = contents_of(&preferences.personal_dictionary());
    assert!(
        kept.iter()
            .any(|(_, words)| words.lines().any(|word| word == MISSPELT)),
        "{MISSPELT:?} is not in the personal dictionary under the launch's own \
         configuration: {kept:?}",
    );
    assert_eq!(
        the_developers_dictionaries(),
        developers,
        "adding a word under test wrote into the developer's own dictionary",
    );

    let again = axiomd_e2e::launch_with(&fixture.write("notes.md", NOTES), &preferences);
    again.activate("win.mode");
    again.wait_until_mode("edit");
    // Asked of the menu rather than of the underline, because an underline that has
    // not arrived yet looks exactly like one that never will: the menu is worked out
    // from the dictionary the moment it opens.
    let menu = menu_on(&again, MISSPELT, "<Shift>F10");
    assert_eq!(
        spelling_rows(&menu),
        Vec::<(&str, &str)>::new(),
        "a word added to the dictionary was a misspelling again after a relaunch",
    );
    assert_eq!(
        again.misspelled(),
        Vec::<String>::new(),
        "a word was marked after a relaunch in a document whose one misspelling was \
         added to the dictionary",
    );

    assert!(again.close().is_empty(), "the launch left processes behind");
}

/// Ignore is for now: the word stops being marked while axiomd runs, and nothing is
/// written anywhere — enchant keeps an ignored word for the session
/// (`enchant_dict_add_to_session`), so the next launch marks it again.
#[test]
fn an_ignored_word_is_ignored_until_axiomd_is_opened_again() {
    let developers = the_developers_dictionaries();
    let preferences = Preferences::new("editor-spelling-ignore");
    let (fixture, app) = editing("editor-spelling-ignore", &preferences);
    app.wait_for("the misspelt word to be marked", || {
        app.misspelled() == vec![MISSPELT.to_owned()]
    });

    menu_on(&app, MISSPELT, "<Shift>F10");
    app.press("Ignore");
    app.wait_for("the ignored word to stop being marked", || {
        app.misspelled().is_empty()
    });
    assert!(app.close().is_empty(), "the launch left processes behind");

    let kept = contents_of(&preferences.personal_dictionary());
    assert!(
        !kept
            .iter()
            .any(|(_, words)| words.lines().any(|word| word == MISSPELT)),
        "ignoring a word wrote it into the personal dictionary: {kept:?}",
    );
    assert_eq!(
        the_developers_dictionaries(),
        developers,
        "ignoring a word under test wrote into the developer's own dictionary",
    );

    let again = axiomd_e2e::launch_with(&fixture.write("notes.md", NOTES), &preferences);
    again.activate("win.mode");
    again.wait_until_mode("edit");
    again.wait_for(
        "the ignored word to be marked again after a relaunch",
        || again.misspelled() == vec![MISSPELT.to_owned()],
    );
    assert!(
        spelling_rows(&menu_on(&again, MISSPELT, "<Shift>F10"))
            .contains(&("Ignore", "spelling.ignore")),
        "the menu on a word ignored in an earlier launch did not offer to ignore it",
    );

    assert!(again.close().is_empty(), "the launch left processes behind");
}

/// Nothing about spelling in the menu while nothing is being checked: with the reader's
/// preference off, and over a document too long to check.
#[test]
fn the_context_menu_offers_no_corrections_while_spelling_is_not_checked() {
    let preferences = Preferences::with("editor-suggestions-off", "spellcheck", "false");
    let (_fixture, app) = editing("editor-suggestions-off", &preferences);
    let menu = menu_on(&app, MISSPELT, "<Shift>F10");
    assert_eq!(
        spelling_rows(&menu),
        Vec::<(&str, &str)>::new(),
        "the menu offered corrections with spell checking switched off",
    );
    assert!(app.close().is_empty(), "the launch left processes behind");

    let fixture = Fixture::new("editor-suggestions-long");
    let enormous = fixture.write("enormous.md", &document_of(1_100_000));
    let preferences = Preferences::new("editor-suggestions-long");
    let app = axiomd_e2e::launch_with(&enormous, &preferences);
    app.wait_until_source(&document_of(1_100_000));
    app.activate("win.mode");
    app.wait_until_mode("edit");
    let menu = menu_on(&app, MISSPELT, "<Shift>F10");
    assert_eq!(
        spelling_rows(&menu),
        Vec::<(&str, &str)>::new(),
        "the menu offered corrections in a document too long to check",
    );
    assert!(app.close().is_empty(), "the launch left processes behind");
}
