//! Issue #18: the buffer is the document, and there are two ways of looking at it.
//!
//! Every test here drives the shipped binary on a headless compositor, types the way
//! the reader types, and reads the result out of what they would be looking at: the
//! rendered page, the source in the editor, the window's title, the file on disk.
//!
//! Two properties recur, and they are the point of the feature. What is rendered comes
//! from the buffer and not from the file (invariant 11), so an edit is on screen before
//! it is anywhere else. And the reader's place survives every switch between the two
//! modes, mapped through the anchor map rather than through proportional scroll
//! (invariant 5).

use std::path::Path;

use axiomd_e2e::{App, Fixture, Preferences};

/// A document long enough to scroll, with a paragraph on a line the tests can name.
///
/// Paragraph `n` is on source line `2n + 1`, and its rendered block carries that line
/// as its anchor — which is the whole of how a place in the page and a place in the
/// source are the same thing.
fn long_document() -> String {
    let mut source = String::from("# Notes\n\n");
    for paragraph in 1..=120 {
        source.push_str(&format!("Paragraph {paragraph}.\n\n"));
    }
    source
}

/// The same, with every paragraph two lines long, so that a caret can be put *inside*
/// a block rather than only at its first line. Paragraph `n` begins on source line
/// `3n`, runs to `3n + 1`, and is followed by a blank line `3n + 2`.
fn document_of_two_line_paragraphs() -> String {
    let mut source = String::from("# Notes\n\n");
    for paragraph in 1..=120 {
        source.push_str(&format!(
            "Paragraph {paragraph}, first line.\nParagraph {paragraph}, second line.\n\n"
        ));
    }
    source
}

/// Whether the block at the top of the page is the one rendered from `line`, as a
/// JavaScript expression the page can be asked.
fn reading_the_block_on(line: u32) -> String {
    format!(
        "(() => {{ for (const block of document.querySelectorAll('[data-line]')) {{ \
         if (block.getBoundingClientRect().bottom > 0) {{ \
         return Number(block.dataset.line) === {line}; }} }} return false; }})()"
    )
}

/// Waits until `condition` has been true three polls running.
///
/// A page that has just been given a document is still finding its heights: blocks
/// below the fold settle after the ones above them, so a position that is right in one
/// poll can be wrong in the next and right again after that. Asking repeatedly is what
/// tells "the page has come to rest here" from "the page went past here".
fn holds_steadily(app: &App, what: &str, condition: &str) {
    let steady = std::cell::Cell::new(0u32);
    app.wait_for(what, || {
        let now = app.dom(&format!("Boolean({condition})")) == "true";
        steady.set(if now { steady.get() + 1 } else { 0 });
        steady.get() >= 3
    });
}

/// Puts the reader on the block rendered from source `line`, and waits until they have
/// come to rest there.
///
/// The scroll is inside the condition, so a page whose layout shifted underneath the
/// last one is simply scrolled again.
fn read_at_block(app: &App, line: u32) {
    holds_steadily(
        app,
        &format!("the reader to be reading the block on source line {line}"),
        &format!(
            "(() => {{ const wanted = document.querySelector('[data-line=\"{line}\"]'); \
             if (wanted === null) {{ return false; }} wanted.scrollIntoView(true); \
             return {}; }})()",
            reading_the_block_on(line)
        ),
    );
}

/// Sends the page back to its very beginning, so that a test about coming back to a
/// place has somewhere to come back *from*.
fn send_the_page_to_the_top(app: &App) {
    app.dom("document.scrollingElement.scrollTop = 0");
    assert_eq!(
        topmost_block(app),
        1,
        "the page did not go back to its start"
    );
}

/// Waits until the window has put the reader back on the block rendered from `line`.
///
/// Nothing here scrolls: this is the window's own doing, and waiting for it is the
/// assertion. Reading resumes where the caret was when the page next arrives rather
/// than when the stack switches, so this is also the point at which a mode switch is
/// finished.
fn wait_for_topmost_block(app: &App, line: u32) {
    holds_steadily(
        app,
        &format!("the reader to be put back on the block on source line {line}"),
        &reading_the_block_on(line),
    );
}

/// The source line of the topmost block still on screen — where the reader is, as the
/// page itself reports it.
fn topmost_block(app: &App) -> u32 {
    app.dom(
        "(() => { for (const block of document.querySelectorAll('[data-line]')) { \
         if (block.getBoundingClientRect().bottom > 0) { return block.dataset.line; } } \
         return '0'; })()",
    )
    .parse()
    .expect("a source line")
}

/// The rendered document, with the whitespace between its blocks levelled out — what
/// "the same document" means when one of the two was patched in and the other loaded.
fn rendered(app: &App) -> String {
    app.dom(
        "document.querySelector('article.markdown').innerHTML \
         .replace(/>\\s+</g, '><').trim()",
    )
}

fn on_disk(file: &Path) -> String {
    std::fs::read_to_string(file).unwrap_or_else(|error| panic!("read {file:?}: {error}"))
}

/// A settings store with autosave switched off, for the tests that are about what
/// happens to work the reader has *not* saved.
fn without_autosave(label: &str) -> Preferences {
    Preferences::with(label, "autosave", "false")
}

/// The whole exit-criterion flow, in one test: open in read mode, edit, come back with
/// the reader's place intact, and save a file that matches the buffer exactly.
#[test]
fn a_document_can_be_read_edited_and_saved_without_the_reader_losing_their_place() {
    let fixture = Fixture::new("edit-round-trip");
    let document = fixture.write("notes.md", &long_document());
    let preferences = without_autosave("edit-round-trip");
    let app = axiomd_e2e::launch_with(&document, &preferences);

    assert_eq!(
        app.mode(),
        "read",
        "opening a file did not start in read mode"
    );
    assert_eq!(app.window_title(), "notes.md");

    // The reader scrolls to paragraph 40, which is on source line 81.
    read_at_block(&app, 81);

    // Ctrl+E.
    app.activate("win.mode");
    app.wait_until_mode("edit");
    assert_eq!(
        app.caret_line(),
        81,
        "editing began somewhere other than where the reader was reading",
    );
    assert_eq!(
        app.source(),
        long_document(),
        "the editor holds another document"
    );

    app.type_text("Edited: ");
    assert!(app.is_modified(), "typing left the document saved");
    assert_eq!(
        app.window_title(),
        "• notes.md",
        "the title says nothing is unsaved"
    );

    // The page behind the editor is already up to date with what was typed — that is
    // what makes coming back to it instant, and what makes the switch below the only
    // thing left that can move the reader.
    app.wait_until("document.body.textContent.includes('Edited: Paragraph 40.')");
    send_the_page_to_the_top(&app);

    // Ctrl+E again.
    app.activate("win.mode");
    app.wait_until_mode("read");
    wait_for_topmost_block(&app, 81);
    assert_eq!(
        on_disk(&document),
        long_document(),
        "the edit reached the file although nobody saved it",
    );

    // Ctrl+S.
    app.activate("win.save");
    app.wait_until_saved();
    assert_eq!(
        on_disk(&document),
        long_document().replace("Paragraph 40.", "Edited: Paragraph 40."),
        "the saved file is not what the reader had in front of them",
    );
    assert_eq!(
        app.window_title(),
        "notes.md",
        "a saved document still says it is not"
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Invariant 11, at the layer where it is visible: what is rendered is the buffer, and
/// nothing waits for a save. This is the retrofit every later feature inherits —
/// outline, search and export all read the same buffer when they arrive.
#[test]
fn an_edit_is_rendered_before_it_is_saved_anywhere() {
    let fixture = Fixture::new("edit-unsaved-render");
    let document = fixture.write("notes.md", "# Notes\n\nThe version on disk.\n");
    let preferences = without_autosave("edit-unsaved-render");
    let app = axiomd_e2e::launch_with(&document, &preferences);

    app.activate("win.mode");
    app.wait_until_mode("edit");
    app.type_text("# Typed, never saved\n\n");

    app.activate("win.mode");
    app.wait_until_mode("read");
    app.wait_until("document.querySelector('h1').textContent === 'Typed, never saved'");

    assert_eq!(app.dom_text("h1"), "Typed, never saved");
    assert_eq!(
        on_disk(&document),
        "# Notes\n\nThe version on disk.\n",
        "rendering the buffer wrote it to the file",
    );
    assert_eq!(
        app.navigation_count(),
        1,
        "editing showed the document by navigating the view rather than patching it",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The page the reader comes back to is the document, not an approximation of it: a
/// document patched in after an edit is the same as one rendered from scratch.
#[test]
fn the_page_after_an_edit_is_what_a_fresh_render_of_it_would_be() {
    let fixture = Fixture::new("edit-same-render");
    let document = fixture.write("notes.md", "# Notes\n\nOne paragraph.\n");
    let edited = "# Notes\n\nOne paragraph.\n\n## Added\n\nWith `code` and a list:\n\n\
                  - first\n- second\n\n";

    let patched = {
        let app = axiomd_e2e::launch(&document);
        app.activate("win.mode");
        app.wait_until_mode("edit");
        app.type_text(edited);
        // The reader typed the whole document over the top of the old one; what is
        // left is the old text after it.
        app.activate("win.mode");
        app.wait_until_mode("read");
        app.wait_until("document.querySelector('h2') !== null");
        app.activate("win.save");
        app.wait_until_saved();
        let page = rendered(&app);
        assert!(app.close().is_empty(), "the launch left processes behind");
        page
    };

    // The same file, opened fresh: one load, one full render, nothing patched.
    let app = axiomd_e2e::launch(&document);
    assert_eq!(
        rendered(&app),
        patched,
        "the page the editor left behind is not the page the file renders to",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The read-to-edit half of the span map, on a crafted fixture: the caret lands on the
/// source line of the topmost block still on screen, wherever in the document that is.
#[test]
fn switching_to_editing_puts_the_caret_where_the_reader_was_reading() {
    let fixture = Fixture::new("edit-caret-from-anchor");
    let document = fixture.write("notes.md", &long_document());
    let app = axiomd_e2e::launch(&document);
    assert_eq!(app.mode(), "read");

    for (anchor, paragraph) in [(21u32, 10), (161, 80), (3, 1)] {
        read_at_block(&app, anchor);

        app.activate("win.mode");
        app.wait_until_mode("edit");
        assert_eq!(
            app.caret_line(),
            anchor,
            "reading paragraph {paragraph} handed editing another line",
        );

        // Away from where the reader was, so that coming back to it is something the
        // window has to do rather than something that is already true — and so that
        // the next round cannot race the scroll this switch asks for.
        send_the_page_to_the_top(&app);
        app.activate("win.mode");
        app.wait_until_mode("read");
        wait_for_topmost_block(&app, anchor);
    }

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The edit-to-read half, and the heuristic it turns on: a caret *inside* a block
/// belongs to that block, not to the one after it, and neither does a caret on the
/// blank line that ends it.
#[test]
fn switching_to_reading_returns_to_the_block_the_caret_is_in() {
    let fixture = Fixture::new("edit-anchor-from-caret");
    let document = fixture.write("notes.md", &document_of_two_line_paragraphs());
    let app = axiomd_e2e::launch(&document);

    // Paragraph 40 begins on line 120, runs to 121, and is followed by a blank 122.
    // All three belong to the block that begins at 120.
    for caret in [120u32, 121, 122] {
        app.activate("win.mode");
        app.wait_until_mode("edit");
        app.place_caret(caret);
        assert_eq!(app.caret_line(), caret);
        // The page is left at the start, so landing on paragraph 40 afterwards is the
        // window putting the reader there rather than the page happening to be there.
        send_the_page_to_the_top(&app);

        app.activate("win.mode");
        app.wait_until_mode("read");
        wait_for_topmost_block(&app, 120);
    }

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Autosave, on by default: the reader stops typing and their work is on disk, without
/// them having asked and without a question. Afterwards the document reads as saved —
/// which is what keeps the external-change matrix on its silent path.
#[test]
fn work_is_written_out_after_the_reader_stops_typing() {
    let fixture = Fixture::new("autosave-on");
    let document = fixture.write("notes.md", "# Notes\n\nOne.\n");
    let app = axiomd_e2e::launch(&document);

    app.activate("win.mode");
    app.wait_until_mode("edit");
    app.type_text("Typed and never saved by hand.\n\n");
    assert!(app.is_modified());

    app.wait_until_saved();

    assert_eq!(
        on_disk(&document),
        "Typed and never saved by hand.\n\n# Notes\n\nOne.\n",
    );
    assert_eq!(app.window_title(), "notes.md");
    assert_eq!(
        app.banner(),
        "",
        "the window's own automatic save came back to it as a change under it",
    );
    assert_eq!(
        app.mode(),
        "edit",
        "the automatic save took the reader out of the editor"
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The exit criterion the reader actually cares about: the session ends without
/// warning, and the file holds the last thing they wrote.
#[test]
fn work_written_out_automatically_survives_a_killed_session() {
    let fixture = Fixture::new("autosave-killed");
    let document = fixture.write("notes.md", "# Notes\n\nOne.\n");
    {
        let app = axiomd_e2e::launch(&document);
        app.activate("win.mode");
        app.wait_until_mode("edit");
        app.type_text("Survives.\n\n");
        app.wait_until_saved();
        // Dropping the harness kills the application where it stands: no close
        // handler, no chance to save on the way out.
    }

    assert_eq!(on_disk(&document), "Survives.\n\n# Notes\n\nOne.\n");
}

/// The reader turned autosave off, so nothing is written behind their back — proved by
/// what the window does on the way out, which is only possible if the work is still
/// unsaved.
#[test]
fn nothing_is_written_behind_the_reader_when_autosave_is_off() {
    let fixture = Fixture::new("autosave-off");
    let document = fixture.write("notes.md", "# Notes\n\nOne.\n");
    let preferences = without_autosave("autosave-off");
    let app = axiomd_e2e::launch_with(&document, &preferences);

    app.activate("win.mode");
    app.wait_until_mode("edit");
    app.type_text("Never written.\n\n");

    // Closing is what proves it: a window with nothing unsaved in it simply closes.
    app.close_window();
    app.wait_for_dialog_saying("Save changes to notes.md before closing?");
    assert_eq!(
        on_disk(&document),
        "# Notes\n\nOne.\n",
        "an edit reached the file although autosave was off",
    );

    app.press("Discard");
    app.wait_until_windows(0);
    assert_eq!(
        on_disk(&document),
        "# Notes\n\nOne.\n",
        "discarding the reader's changes wrote them out",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The other two answers to that question, and the one thing it must never do: close
/// the window with the reader's work still in it and nowhere else.
#[test]
fn the_question_on_the_way_out_is_obeyed_whichever_way_it_is_answered() {
    let fixture = Fixture::new("close-unsaved");
    let document = fixture.write("notes.md", "# Notes\n\nOne.\n");
    let preferences = without_autosave("close-unsaved");
    let app = axiomd_e2e::launch_with(&document, &preferences);

    app.activate("win.mode");
    app.wait_until_mode("edit");
    app.type_text("Kept.\n\n");

    // Cancel: the window stays, and so does the work in it.
    app.close_window();
    app.wait_for_dialog_saying("Save changes to notes.md before closing?");
    app.press("Cancel");
    assert_eq!(
        app.window_count(),
        1,
        "cancelling the question closed the window"
    );
    assert!(app.is_modified(), "cancelling threw the reader's work away");
    assert_eq!(app.source(), "Kept.\n\n# Notes\n\nOne.\n");

    // Save: the work goes to the file and the window closes.
    app.close_window();
    app.wait_for_dialog_saying("Save changes to notes.md before closing?");
    app.press("Save");
    app.wait_until_windows(0);
    assert_eq!(on_disk(&document), "Kept.\n\n# Notes\n\nOne.\n");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// A window with nothing unsaved in it closes without a word — the case autosave makes
/// the common one.
#[test]
fn a_window_with_nothing_unsaved_closes_without_a_question() {
    let fixture = Fixture::new("close-clean");
    let document = fixture.write("notes.md", "# Notes\n\nOne.\n");
    let app = axiomd_e2e::launch(&document);

    app.activate("win.mode");
    app.wait_until_mode("edit");
    app.type_text("Saved automatically.\n\n");
    app.wait_until_saved();

    // Closing is the assertion: a window with unsaved work in it stops on a question
    // and stays open, so a window that simply goes had nothing to ask about.
    app.close_window();
    app.wait_until_windows(0);

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The clean half of the external-change matrix, in the editor: the buffer follows the
/// file silently, and the reader is asked nothing (`ux_decisions.md`).
#[test]
fn a_clean_buffer_follows_the_file_while_the_reader_is_in_the_editor() {
    let fixture = Fixture::new("external-clean");
    let document = fixture.write("notes.md", "# Notes\n\nOne.\n");
    let app = axiomd_e2e::launch(&document);

    app.activate("win.mode");
    app.wait_until_mode("edit");

    std::fs::write(&document, "# Notes\n\nTwo.\n").expect("save over the document");

    app.wait_until_source("# Notes\n\nTwo.\n");
    assert_eq!(
        app.banner(),
        "",
        "a clean buffer following its file said something"
    );
    assert_eq!(
        app.visible_dialog(),
        "",
        "following the file asked a question"
    );
    assert!(!app.is_modified());

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// The dirty half: nothing is overwritten in either direction and the whole choice is
/// on screen, beside the document, with no dialog anywhere (invariant 12).
#[test]
fn unsaved_work_meeting_a_changed_file_offers_the_reader_the_choice() {
    let fixture = Fixture::new("external-conflict");
    let document = fixture.write("notes.md", "# Notes\n\nOne.\n");
    let preferences = without_autosave("external-conflict");
    let app = axiomd_e2e::launch_with(&document, &preferences);

    app.activate("win.mode");
    app.wait_until_mode("edit");
    app.type_text("Mine.\n\n");

    std::fs::write(&document, "Theirs.\n\n# Notes\n\nOne.\n").expect("save over the document");
    app.wait_for_banner("changed on disk");
    assert_eq!(
        app.visible_dialog(),
        "",
        "a change under the reader interrupted them with a dialog",
    );
    assert_eq!(
        app.source(),
        "Mine.\n\n# Notes\n\nOne.\n",
        "the reader's work was replaced without being asked",
    );

    // Keeping mine leaves the buffer alone and the file alone.
    app.press("Keep Mine");
    app.wait_until_no_banner();
    assert_eq!(app.source(), "Mine.\n\n# Notes\n\nOne.\n");
    assert_eq!(on_disk(&document), "Theirs.\n\n# Notes\n\nOne.\n");
    assert!(app.is_modified());

    // And the next change offers the choice again, with the other answer taken.
    std::fs::write(&document, "Theirs, again.\n\n# Notes\n\nOne.\n").expect("save again");
    app.wait_for_banner("changed on disk");
    app.press("Reload");
    app.wait_until_no_banner();
    app.wait_until_source("Theirs, again.\n\n# Notes\n\nOne.\n");
    assert!(
        !app.is_modified(),
        "taking the file's version left unsaved work behind"
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Undo and redo are the buffer's, and they reach exactly the reader's own edits.
#[test]
fn undo_and_redo_go_back_over_the_readers_edits() {
    let fixture = Fixture::new("edit-undo");
    let document = fixture.write("notes.md", "original\n");
    let preferences = without_autosave("edit-undo");
    let app = axiomd_e2e::launch_with(&document, &preferences);

    app.activate("win.mode");
    app.wait_until_mode("edit");
    app.type_text("typed ");
    assert_eq!(app.source(), "typed original\n");

    app.activate("win.undo");
    assert_eq!(app.source(), "original\n");

    app.activate("win.undo");
    assert_eq!(
        app.source(),
        "original\n",
        "undo reached past the document the reader was given",
    );

    app.activate("win.redo");
    assert_eq!(app.source(), "typed original\n");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// An untitled document has nowhere to be saved, so `Ctrl+S` asks rather than writes —
/// and once it has been told where, the window is that file's from then on.
#[test]
fn an_untitled_document_is_given_a_name_before_it_is_written() {
    let fixture = Fixture::new("untitled-save");
    let preferences = without_autosave("untitled-save");
    let app = axiomd_e2e::launch_without_document_with(&preferences);

    app.type_text("# A new document\n");
    assert!(app.is_modified());
    assert_eq!(app.window_title(), "• Untitled");

    // `Ctrl+S` on a document with no file asks where it goes; nothing is written and
    // the work is still unsaved afterwards.
    app.activate("win.save");
    assert!(
        app.is_modified(),
        "an untitled document was written somewhere without being given a name",
    );

    let chosen = fixture.write("chosen.md", "");
    app.save_as(&chosen);
    app.wait_until_saved();

    assert_eq!(on_disk(&chosen), "# A new document\n");
    assert_eq!(app.window_title(), "chosen.md");

    // And the window follows its new file: a change to it reaches the reader.
    app.activate("win.mode");
    app.wait_until_mode("read");
    app.wait_until("document.querySelector('h1') !== null");
    std::fs::write(&chosen, "# Renamed by somebody else\n").expect("save over the document");
    app.wait_until("document.querySelector('h1').textContent === 'Renamed by somebody else'");

    assert!(app.close().is_empty(), "the launch left processes behind");
}

/// Typing costs a keystroke, whatever the document costs to render.
///
/// The budget is asserted by the harness's own deadline: a document this size takes
/// hundreds of milliseconds to parse and render, so an editor that rendered per key
/// press could not finish this test at all — three hundred of them would be minutes of
/// work. What it does assert positively is that every keystroke reached the buffer,
/// and that the whole burst became a handful of renders rather than three hundred.
#[test]
fn typing_never_waits_for_the_render_of_a_large_document() {
    let fixture = Fixture::new("edit-latency");
    let mut source = String::from("# Large\n\n");
    while source.len() < 1_000_000 {
        source.push_str(
            "A paragraph of a large document, with `code` and *emphasis* in it, long \
             enough that a thousand of them add up to something worth rendering.\n\n",
        );
    }
    let document = fixture.write("large.md", &source);
    let app = axiomd_e2e::launch(&document);

    app.activate("win.mode");
    app.wait_until_mode("edit");
    let before = app.render_count();

    const KEYSTROKES: usize = 300;
    for _ in 0..KEYSTROKES {
        app.type_text("x");
    }

    assert_eq!(
        app.source().chars().take_while(|key| *key == 'x').count(),
        KEYSTROKES,
        "keystrokes were lost between the keyboard and the buffer",
    );
    let renders = app.render_count() - before;
    assert!(
        renders < 30,
        "{KEYSTROKES} keystrokes produced {renders} renders, so typing is paying for \
         rendering",
    );

    assert!(app.close().is_empty(), "the launch left processes behind");
}
