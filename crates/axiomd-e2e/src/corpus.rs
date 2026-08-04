//! The documents the performance budgets are measured on.
//!
//! A budget is only as honest as the document it is measured on, and the three here
//! are the three shapes the budgets in issue #9 are about:
//!
//! * [`typical`] — what a README or a page of notes is. The startup budget is about
//!   this document and no other: "cold start to rendered typical file < 300 ms"
//!   (VISION) means this one.
//! * [`of_size`] — the same vocabulary grown to any size, which is how the 10 MB
//!   document exists without a 10 MB file in the repository.
//! * [`with_headings`] — a thousand sections and almost nothing else, which is what
//!   the outline sidebar costs money on (issue #35).
//! * [`deeply_nested`] — the pathological case: block structure nested far past
//!   anything a person writes, which is where a recursive parser or renderer goes
//!   quadratic if it is going to.
//!
//! # Why generated rather than checked in
//!
//! A 10 MB fixture in git is a 10 MB clone for everyone forever, and a fixture nobody
//! can read is a fixture nobody notices going stale. These are written from one
//! template, so what is in them is the few lines below rather than a blob — and every
//! run of every budget measures byte-identical text, because nothing here is random.
//!
//! # What is in them
//!
//! Everything the pipeline actually costs money on: headings (the outline reads them),
//! prose with inline markup, lists, links, tables, block quotes, and fenced code —
//! which is the expensive one, because a fence goes through the syntax highlighter.
//! The proportions are a document a developer would really have: about a sixth of it
//! code.

/// What [`typical`] is: a page of notes, a middling README.
const TYPICAL: usize = 50 * 1024;

/// How deep [`deeply_nested`] goes. Far past anything a person writes and far enough
/// that a parser or renderer that is quadratic in depth shows it.
const DEPTH: usize = 200;

/// A document of the size a README or a page of notes is.
///
/// The one the startup budget is about.
pub fn typical() -> String {
    of_size(TYPICAL)
}

/// A document of at least `bytes`, built from the same blocks as [`typical`].
///
/// Every section is written once with its own number, so the headings are distinct
/// (an outline of 20 000 identical entries would not be an outline) and no two blocks
/// are byte-identical — which matters for anything that might otherwise cache or
/// deduplicate its way to a flattering number.
pub fn of_size(bytes: usize) -> String {
    let mut document = String::with_capacity(bytes + 1024);
    document.push_str("# Performance corpus\n\nA generated document, written to be measured.\n\n");
    let mut section = 1;
    while document.len() < bytes {
        write_section(&mut document, section);
        section += 1;
    }
    document
}

/// A document of `headings` sections, nested three levels deep — the shape the outline
/// sidebar itself costs money on.
///
/// Every other kind of block is left out on purpose: this is the document that says
/// what a thousand rows in the sidebar cost, and prose around them would only bury that
/// number under the renderer's.
///
/// The levels cycle `##`, `###`, `###`, so most sections have sections under them and
/// the tree is a tree rather than a list. Every heading is distinct, because a thousand
/// identical rows would not be an outline.
pub fn with_headings(headings: usize) -> String {
    let mut document =
        String::from("# Heading corpus\n\nA generated document, written to be measured.\n\n");
    for section in 1..=headings.saturating_sub(1) {
        let level = match section % 3 {
            1 => "##",
            _ => "###",
        };
        document.push_str(&format!(
            "{level} Section {section}\n\nWhat section {section} says.\n\n"
        ));
    }
    document
}

/// Block structure nested [`DEPTH`] deep — a list inside a quote inside a list, all
/// the way down, and then closed again by the text that follows it.
///
/// Nothing a person writes; everything a parser that recurses per level has to
/// survive.
pub fn deeply_nested() -> String {
    let mut document = String::from("# Deeply nested\n\n");
    for level in 0..DEPTH {
        // Two characters of indent per level, alternating the two containers that
        // nest, so the depth is real block nesting rather than one long list.
        let indent = "  ".repeat(level);
        match level % 2 {
            0 => document.push_str(&format!("{indent}- level {level}\n")),
            _ => document.push_str(&format!("{indent}> level {level}\n\n")),
        }
    }
    document.push_str(&format!("{}Bottom.\n", "  ".repeat(DEPTH)));
    document.push_str("\nBack at the top level.\n");
    document
}

/// One section of [`of_size`]: the blocks a real document is made of, numbered so this
/// one is unlike every other.
fn write_section(document: &mut String, section: usize) {
    document.push_str(&format!(
        "## Section {section}\n\
         \n\
         Prose with **bold**, *emphasis*, `inline code` and a [link](./section-{section}.md)\n\
         so that the paragraph is a paragraph rather than a line, and so the renderer has\n\
         inline work to do inside block {section} as well as around it.\n\
         \n\
         - The first thing about section {section}\n\
         - The second, with `code` in it\n\
         - The third, which links to [the next one](./section-{next}.md)\n\
         \n\
         | Field | Value |\n\
         | --- | ---: |\n\
         | number | {section} |\n\
         | doubled | {doubled} |\n\
         \n\
         > What section {section} is remembered for.\n\
         \n\
         ```rust\n\
         fn section_{section}(input: u32) -> u32 {{\n\
         \x20   // A comment, a string and a number, so the highlighter has all three.\n\
         \x20   let label = \"section {section}\";\n\
         \x20   input * {section} + label.len() as u32\n\
         }}\n\
         ```\n\
         \n",
        next = section + 1,
        doubled = section * 2,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A budget measured on a document that is not the size it claims is not a budget.
    #[test]
    fn a_document_is_at_least_the_size_it_was_asked_for() {
        assert!(typical().len() >= TYPICAL, "{}", typical().len());
        // And not wildly over it: one section of overshoot, not one document of it.
        assert!(typical().len() < TYPICAL * 2, "{}", typical().len());

        let huge = of_size(4 * 1024 * 1024);
        assert!(huge.len() >= 4 * 1024 * 1024, "{}", huge.len());
    }

    /// The same measurement twice must be the same measurement. Nothing here may vary
    /// between two runs, two processes or two machines.
    #[test]
    fn the_same_document_is_generated_every_time() {
        assert_eq!(typical(), typical());
        assert_eq!(of_size(200_000), of_size(200_000));
        assert_eq!(deeply_nested(), deeply_nested());
    }

    /// What a budget is measured on: real blocks, distinct headings, and code for the
    /// highlighter to spend time on.
    #[test]
    fn a_generated_document_holds_the_blocks_a_real_one_does() {
        let document = typical();

        assert!(document.contains("## Section 1\n"), "no headings");
        assert!(
            document.contains("## Section 2\n"),
            "headings do not differ"
        );
        assert!(document.contains("```rust\n"), "no code to highlight");
        assert!(document.contains("| Field | Value |"), "no tables");
        assert!(document.contains("> What section 1 is"), "no block quotes");
        assert!(document.contains("- The first thing"), "no lists");
    }

    /// A budget on a thousand rows has to be measured on a thousand rows, nested.
    #[test]
    fn the_heading_corpus_holds_the_headings_it_claims_at_the_levels_it_claims() {
        let document = with_headings(1_000);
        let headings: Vec<&str> = document
            .lines()
            .filter(|line| line.starts_with('#'))
            .collect();

        assert_eq!(headings.len(), 1_000);
        assert_eq!(headings[0], "# Heading corpus");
        assert_eq!(headings[1], "## Section 1");
        assert_eq!(headings[2], "### Section 2");
        assert_eq!(headings[3], "### Section 3");
        assert_eq!(headings[4], "## Section 4");
        assert_eq!(headings[999], "### Section 999");
    }

    /// The pathological document has to actually be deep, or it is only a document.
    #[test]
    fn the_pathological_document_nests_as_deep_as_it_claims() {
        let document = deeply_nested();

        assert!(document.contains(&format!("{}Bottom.", "  ".repeat(DEPTH))));
        assert!(document.contains(&format!("{}- level {}", "  ".repeat(DEPTH - 2), DEPTH - 2)));
    }
}
