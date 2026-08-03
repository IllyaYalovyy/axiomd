//! The outline: the document's headings, as the sidebar reads them.
//!
//! The outline is not a second parse and not a scan of the markup — it is the part of
//! the anchor map that happens to be a heading. That is what makes clicking an entry
//! and following the reader's scroll the same mapping, and what keeps them working
//! after a live reload has moved every source line in the document (invariant 3).

mod support;

use support::{fixtures, render};

/// The whole shape of the model: what the entry says, how deeply it nests, and which
/// block it names.
#[test]
fn the_outline_is_every_heading_with_its_level_and_the_line_it_starts_on() {
    let rendered = render(
        "# Guide\n\
         \n\
         Intro.\n\
         \n\
         ## Getting started\n\
         \n\
         ### Requirements\n\
         \n\
         ## Reference\n\
         \n\
         Setext\n\
         ======\n",
    );

    let outline: Vec<(u8, &str, u32)> = rendered
        .outline()
        .iter()
        .map(|heading| (heading.level, heading.text.as_str(), heading.line))
        .collect();

    assert_eq!(
        outline,
        vec![
            (1, "Guide", 1),
            (2, "Getting started", 5),
            (3, "Requirements", 7),
            (2, "Reference", 9),
            (1, "Setext", 11),
        ],
    );
}

/// A heading is words, however they were written: emphasis, code and a link are all
/// read out as the text the reader sees, and a picture's alt text is not part of it —
/// the same rule the heading's own anchor id follows.
#[test]
fn an_entry_reads_out_the_headings_words_and_not_its_markup() {
    let rendered = render(
        "# The *quick* `brown` [fox](f.md)\n\
         \n\
         ## Logo ![alt text](logo.png) here\n",
    );

    let text: Vec<&str> = rendered
        .outline()
        .iter()
        .map(|heading| heading.text.as_str())
        .collect();
    assert_eq!(text, vec!["The quick brown fox", "Logo here"]);
}

/// Two sections can be called the same thing, and clicking the second one has to go to
/// the second one. What keeps them apart is the anchor, never the words.
#[test]
fn headings_with_the_same_words_stay_distinct_entries() {
    let rendered = render("## Notes\n\nOne.\n\n## Notes\n\nTwo.\n\n## Notes\n");

    let outline = rendered.outline();
    assert_eq!(outline.len(), 3, "repeated headings were collapsed");
    assert_eq!(
        outline
            .iter()
            .map(|heading| heading.line)
            .collect::<Vec<_>>(),
        vec![1, 5, 9],
    );
    assert!(
        outline.iter().all(|heading| heading.text == "Notes"),
        "the entries are no longer all called Notes",
    );
}

/// The non-happy path the sidebar has to show an empty state for.
#[test]
fn a_document_with_no_headings_has_an_empty_outline() {
    let rendered = render("Just a paragraph.\n\n- and a list\n\n> and a quote\n");
    assert!(rendered.outline().is_empty());
}

/// A heading inside a container is not a place the reader can be sent: only top-level
/// blocks carry an anchor, so an entry for one would be a row that does nothing.
#[test]
fn a_heading_nested_inside_a_container_is_not_an_entry() {
    let rendered = render("# Top\n\n> ## Quoted\n\n- ### Listed\n");

    assert_eq!(
        rendered
            .outline()
            .iter()
            .map(|heading| heading.text.as_str())
            .collect::<Vec<_>>(),
        vec!["Top"],
    );
}

/// The load-bearing claim: every entry names a block the anchor map has, so clicking
/// it lands on the block the page carries under that `data-line`. Asserted over every
/// golden fixture rather than over one hand-written document.
#[test]
fn every_outline_entry_names_a_block_the_anchor_map_has() {
    let mut checked = 0usize;
    for (name, source) in fixtures() {
        let rendered = render(&source);
        let anchored: Vec<u32> = rendered
            .anchors()
            .iter()
            .map(|anchor| anchor.line)
            .collect();
        let mut previous = 0;
        for heading in rendered.outline() {
            assert!(
                anchored.contains(&heading.line),
                "{name}: the outline entry {:?} is on line {} and no block is anchored there",
                heading.text,
                heading.line,
            );
            assert!(
                heading.line > previous,
                "{name}: outline entry {:?} is out of document order",
                heading.text,
            );
            previous = heading.line;
            assert!(
                (1..=6).contains(&heading.level),
                "{name}: {:?} has heading level {}",
                heading.text,
                heading.level,
            );
            checked += 1;
        }
    }
    assert!(
        checked > 5,
        "only {checked} outline entries checked; the fixtures have no headings",
    );
}
