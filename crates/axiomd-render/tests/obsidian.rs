//! The Obsidian surface as a reader meets it (issue #12): callouts, wikilinks,
//! footnotes and task lists.
//!
//! The golden fixtures beside these pin whole documents byte for byte. What is here is
//! the behaviour those bytes are *for*, said one requirement at a time — so a failure
//! names the thing that broke rather than a line number in a pinned file.

mod support;

use axiomd_render::Folder;
use support::{render, render_beside};

/// Obsidian's whole vocabulary reaches the document as a kind the stylesheet can
/// colour, and a kind axiomd has never heard of still renders — as a note, wearing its
/// own name.
#[test]
fn every_callout_kind_carries_its_class_and_its_title() {
    for (written, class, title) in [
        ("note", "note", "Note"),
        ("summary", "abstract", "Summary"),
        ("tldr", "abstract", "Tldr"),
        ("info", "info", "Info"),
        ("todo", "todo", "Todo"),
        ("hint", "tip", "Hint"),
        ("important", "tip", "Important"),
        ("done", "success", "Done"),
        ("faq", "question", "Faq"),
        ("attention", "warning", "Attention"),
        ("missing", "failure", "Missing"),
        ("error", "danger", "Error"),
        ("bug", "bug", "Bug"),
        ("example", "example", "Example"),
        ("cite", "quote", "Cite"),
        ("nonsense", "note", "Nonsense"),
    ] {
        let html = render(&format!("> [!{written}]\n> Body.\n"))
            .html()
            .to_owned();
        assert!(
            html.contains(&format!("<blockquote class=\"callout callout-{class}\"")),
            "[!{written}] is not styled as a {class} callout:\n{html}",
        );
        assert!(
            html.contains(&format!("<p class=\"callout-title\">{title}</p>")),
            "[!{written}] is not called {title:?}:\n{html}",
        );
        assert!(
            html.contains("Body."),
            "[!{written}] lost its body:\n{html}"
        );
    }
}

/// A foldable callout folds without a line of JavaScript, because the document cannot
/// run one: it is a `<details>`, and `-` and `+` decide whether it starts shut or open.
#[test]
fn a_foldable_callout_is_a_details_element_that_starts_as_its_author_asked() {
    let shut = render("> [!warning]- Shut\n> Hidden until opened.\n")
        .html()
        .to_owned();
    assert!(
        shut.contains("<details class=\"callout callout-warning\" data-line=\"1\">"),
        "{shut}"
    );
    assert!(
        shut.contains("<summary class=\"callout-title\">Shut</summary>"),
        "{shut}"
    );
    assert!(shut.contains("</details>"), "{shut}");
    assert!(
        !shut.contains("open=\"\""),
        "a callout written with `-` starts open:\n{shut}",
    );

    let open = render("> [!tip]+ Open\n> Shown until closed.\n")
        .html()
        .to_owned();
    assert!(
        open.contains("<details class=\"callout callout-tip\" open=\"\" data-line=\"1\">"),
        "{open}"
    );

    // And an ordinary callout is not a `<details>`: nothing about it can be folded
    // away, so nothing about it invites the reader to try.
    let plain = render("> [!note]\n> Always here.\n").html().to_owned();
    assert!(!plain.contains("<details"), "{plain}");
}

/// Callouts nest, and the inner one is a callout in its own right rather than a quote
/// with brackets in it.
#[test]
fn a_callout_inside_a_callout_is_two_callouts() {
    let html = render("> [!note] Outer\n> Text.\n>\n> > [!bug] Inner\n> > Deep.\n")
        .html()
        .to_owned();

    assert!(html.contains("callout-note"), "{html}");
    assert!(html.contains("callout-bug"), "{html}");
    assert!(
        html.contains("<p class=\"callout-title\">Inner</p>"),
        "{html}"
    );
    assert!(
        !html.contains("[!bug]"),
        "the inner marker is being shown to the reader:\n{html}",
    );
}

/// A resolved wikilink is a link the reader can follow, spelled exactly as a relative
/// Markdown link is — which is what makes it travel the path issue #6 already built.
#[test]
fn a_resolved_wikilink_is_a_relative_link_to_that_document() {
    let beside = Folder::holding(["guide.md", "notes/setup.md"].map(str::to_owned));
    let html = render_beside(
        "[[guide]] and [[notes/setup|the setup]] and [[guide#Getting Started]].\n",
        &beside,
    )
    .html()
    .to_owned();

    assert!(
        html.contains("<a class=\"wikilink\" href=\"guide.md\""),
        "{html}"
    );
    assert!(
        html.contains("<a class=\"wikilink\" href=\"notes/setup.md\""),
        "{html}"
    );
    assert!(
        html.contains(">the setup</a>"),
        "the alias was lost:\n{html}"
    );
    assert!(
        html.contains("<a class=\"wikilink\" href=\"guide.md#getting-started\""),
        "{html}"
    );
}

/// An unresolved wikilink is inert by construction rather than by a policy that has to
/// refuse it: there is no link there to press. It still says what the author wrote.
#[test]
fn an_unresolved_wikilink_is_not_a_link_at_all() {
    let beside = Folder::holding(["notes/draft.md", "archive/draft.md"].map(str::to_owned));
    let html = render_beside("[[missing]] and [[draft]] and ![[picture.png]].\n", &beside)
        .html()
        .to_owned();

    for target in ["missing", "draft", "picture.png"] {
        assert!(
            html.contains(&format!(
                "<span class=\"wikilink wikilink-unresolved\">{target}</span>"
            )),
            "{target} is not shown as an unresolved wikilink:\n{html}",
        );
    }
    assert!(
        !html.contains("href=\"missing"),
        "an unresolved wikilink is still a link:\n{html}",
    );
}

/// The reader sees numbers in the order the document refers to its footnotes, and
/// every reference has a way back to itself.
#[test]
fn footnotes_are_numbered_by_first_reference_and_link_both_ways() {
    let html = render(
        "A[^b] then B[^a] then A again[^b].\n\n[^a]: First written.\n\n[^b]: Second written.\n",
    )
    .html()
    .to_owned();

    // `b` is referred to first, so `b` is footnote 1 however the definitions are laid
    // out in the source.
    assert!(
        html.contains("<a id=\"fnref-fn-b-1\" href=\"#fn-b\" rel=\"noopener noreferrer\">1</a>"),
        "{html}"
    );
    assert!(
        html.contains("<a id=\"fnref-fn-a-1\" href=\"#fn-a\" rel=\"noopener noreferrer\">2</a>"),
        "{html}"
    );
    // The second reference to the same footnote keeps the number and gets its own id.
    assert!(
        html.contains("<a id=\"fnref-fn-b-2\" href=\"#fn-b\" rel=\"noopener noreferrer\">1</a>"),
        "{html}"
    );

    assert!(
        html.contains("<div class=\"footnote-definition\" id=\"fn-b\""),
        "{html}"
    );
    assert!(
        html.contains("<sup class=\"footnote-label\">1</sup>"),
        "{html}"
    );
    // Two references, two ways back — and the second one says which it is.
    assert!(html.contains("href=\"#fnref-fn-b-1\""), "{html}");
    assert!(html.contains("href=\"#fnref-fn-b-2\""), "{html}");
    assert!(html.contains("↩<sup>2</sup>"), "{html}");
    assert_eq!(
        html.matches("class=\"footnote-backref\"").count(),
        3,
        "one way back per reference, and no more:\n{html}",
    );
}

/// A definition that does not end in a paragraph still gets its way back — on a line
/// of its own, because an arrow inside a list or a code block would change what that
/// block is.
#[test]
fn a_definition_that_ends_in_a_list_still_has_a_way_back() {
    let html = render("A[^a].\n\n[^a]: See:\n\n    - one\n    - two\n")
        .html()
        .to_owned();

    assert!(
        html.contains("<p class=\"footnote-backrefs\"> <a class=\"footnote-backref\""),
        "{html}"
    );
    assert!(html.contains("<li>two</li>"), "the list was lost:\n{html}");
}

/// A task list item's box is something the reader can press, and pressing it says
/// exactly which box: the offset of its own marker in the source, never its text.
#[test]
fn every_task_box_is_a_press_that_names_its_own_place_in_the_source() {
    let source = "- [ ] same\n- [ ] same\n- [x] done\n";
    let html = render(source).html().to_owned();

    let presses: Vec<usize> = html
        .match_indices("href=\"axiomd://request/task?at=")
        .map(|(at, marker)| {
            let rest = &html[at + marker.len()..];
            rest[..rest.find('"').expect("a closed attribute")]
                .parse()
                .expect("a source offset")
        })
        .collect();

    assert_eq!(presses.len(), 3, "{html}");
    assert_eq!(
        presses[0], 3,
        "the first box does not name its own marker: {html}"
    );
    assert_ne!(
        presses[0], presses[1],
        "two items that say the same thing press the same box",
    );
    for at in presses {
        assert_eq!(
            &source[at - 1..at + 2],
            if at == 25 { "[x]" } else { "[ ]" }
        );
    }

    // And the box itself is enabled, which is what says it can be pressed.
    assert!(
        html.contains("<input type=\"checkbox\"></a>"),
        "the box is not a box a reader can press:\n{html}",
    );
    assert!(
        !html.contains("disabled"),
        "a box a reader can press is still disabled:\n{html}",
    );
}

/// An exported file has no application behind it, so its boxes say what the author
/// wrote and offer nothing: a button with nothing behind it is worse than no button.
#[test]
fn a_task_box_in_an_exported_file_is_not_a_button() {
    let exported = axiomd_render::standalone(
        &support::parse("- [ ] not done\n- [x] done\n"),
        "notes",
        &axiomd_render::Plugins::builtin(&[]),
        &Folder::empty(),
        &|_| None,
    );

    assert!(
        exported.contains("<input disabled=\"\" type=\"checkbox\">"),
        "{exported}"
    );
    // The document itself, without the stylesheet it carries — which names the class
    // because the same sheet styles the screen.
    let body = &exported[exported.find("<article").expect("the document")..];
    assert!(
        !body.contains("task-toggle"),
        "the exported file offers a press nothing can answer:\n{body}",
    );
    assert!(!exported.contains("axiomd:"), "{exported}");
}
