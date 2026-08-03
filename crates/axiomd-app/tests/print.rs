//! UT-013 and UT-014: the document on paper, in a PDF, and in a page anybody can
//! open.
//!
//! Every test here drives the shipped binary on a headless compositor and then reads
//! what came out the way the person receiving it would: the PDF is opened and its text
//! extracted page by page, the exported page is read as a browser reads it. Nothing
//! asserts that a function was called — the assertions are what is on the page.
//!
//! The launch's printers are pinned to the file backend (`axiomd-e2e/src/display.rs`),
//! so a suite that prints cannot reach a real printer.

use std::path::Path;

use axiomd_e2e::{App, Fixture};

/// A document with one of everything printing has an opinion about: headings, a
/// paragraph, a link that has to say where it goes, code, a table, and a picture from
/// the internet that nobody has loaded.
const REPORT: &str = "\
# Quarterly Report

Revenue rose in every region we measure.

## Method

Numbers come from the ledger, checked against [the public filing](https://example.com/filing).

```rust
fn total(rows: &[i64]) -> i64 {
    rows.iter().sum()
}
```

| Region | Revenue |
| ------ | ------- |
| North  | 41      |
| South  | 27      |

![The regional map](https://cdn.example.com/map.png)

## Outlook

Steady.
";

/// The text of an exported PDF, one string per page, in page order.
fn pdf_pages(file: &Path) -> Vec<String> {
    pdf_extract::extract_text_by_pages(file)
        .unwrap_or_else(|error| panic!("{} is not a readable PDF: {error}", file.display()))
}

/// Every strings' position in `text`, failing with what was actually there when one
/// of them is missing.
fn positions(text: &str, wanted: &[&str]) -> Vec<usize> {
    wanted
        .iter()
        .map(|needle| {
            text.find(needle)
                .unwrap_or_else(|| panic!("{needle:?} is not in the exported document:\n{text}"))
        })
        .collect()
}

/// Exports `document` to `file` and waits until the window says it is done.
///
/// "Done" is the whole sentence beside the document, not a word inside it: the window
/// says "Exported nothing — …" when an export fails, and a helper that waited for
/// "Exported" alone would read a failure as a success.
fn export(app: &App, file: &Path) {
    let name = file
        .file_name()
        .and_then(|name| name.to_str())
        .expect("a file to export to");
    app.export_to(file);
    let said = app.wait_for_banner("Exported");
    assert_eq!(
        said,
        format!("Exported {name}"),
        "the window did not say the document was exported",
    );
    assert!(file.is_file(), "{} was never written", file.display());
}

/// UT-014: the reader exports a PDF and gets the document they were reading — in order,
/// with the addresses their links carried, and without the buttons only axiomd could
/// have answered.
#[test]
fn an_exported_pdf_reads_as_the_document_on_screen_reads() {
    let fixture = Fixture::new("print-pdf");
    let document = fixture.write("report.md", REPORT);
    let pdf = document.with_file_name("report.pdf");
    let app = axiomd_e2e::launch(&document);

    export(&app, &pdf);

    let text = pdf_pages(&pdf).join("\n");
    let order = positions(
        &text,
        &[
            "Quarterly Report",
            "Revenue rose",
            "Method",
            "public filing",
            "fn total",
            "Region",
            "Outlook",
        ],
    );
    assert!(
        order.windows(2).all(|pair| pair[0] < pair[1]),
        "the document came out in the wrong order: {order:?}\n{text}",
    );
    // Paper cannot be clicked, so the address is written out beside the words.
    assert!(
        text.contains("https://example.com/filing"),
        "a printed link does not say where it goes:\n{text}",
    );
    // And what cannot be pressed is not offered: the load buttons are the app's, not
    // the document's, and they stay in the app.
    assert!(
        !text.contains("Load image") && !text.contains("Load all"),
        "a button nobody can press was printed:\n{text}",
    );
    assert!(
        text.contains("cdn.example.com"),
        "the reader is not told what picture is missing:\n{text}",
    );

    assert_eq!(
        app.visible_dialog(),
        "",
        "exporting interrupted the reader with a dialog",
    );
    assert!(app.close().is_empty(), "something outlived the application");
}

/// `text` with every space, tab and newline taken out.
///
/// How a PDF's text is compared with the document it came from: paper breaks a
/// paragraph wherever the line ends, and the page it was made from breaks it
/// somewhere else, so whitespace is the one thing the two cannot be expected to
/// agree on. Nothing else is normalised — the words and their order still have to
/// match exactly.
fn squeezed(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

/// `text` without the `<address>` the print stylesheet writes after every link, so
/// that what is left can be compared with the page the reader was looking at.
///
/// Only an address is taken out — an opening bracket followed by a scheme the
/// stylesheet writes addresses for. A document's own angle brackets, `->` in a code
/// block among them, are left where they are.
fn without_link_addresses(text: &str) -> String {
    let mut kept = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(['<']) {
        let (before, from) = rest.split_at(at);
        kept.push_str(before);
        let address = ["<http://", "<https://", "<mailto:"]
            .iter()
            .any(|scheme| from.starts_with(scheme));
        match from.find('>').filter(|_| address) {
            Some(end) => rest = &from[end + 1..],
            None => {
                kept.push('<');
                rest = &from[1..];
            }
        }
    }
    kept.push_str(rest);
    kept
}

/// UT-013 and UT-014 are one machine, seen from where the reader sees it: the
/// exported file is the page on screen, put through the print stylesheet.
///
/// The two halves are what make this more than "a PDF came out". Everything the
/// reader can see in the window is in the file — so nothing renders the document a
/// second way — and the file also carries what *only* `@media print` adds and the
/// window never shows, so it was produced by a print job rather than by a separate
/// exporter that happens to agree today.
#[test]
fn the_exported_pdf_is_the_page_on_screen_after_the_print_stylesheet() {
    let fixture = Fixture::new("print-one-path");
    let document = fixture.write("report.md", REPORT);
    let app = axiomd_e2e::launch(&document);

    let on_screen = app.dom_text("article.markdown");
    assert!(
        on_screen.contains("Load image") && on_screen.contains("Load all"),
        "the window is not showing the affordances only it can answer:\n{on_screen}",
    );
    assert!(
        !on_screen.contains("https://example.com/filing"),
        "the window already writes link addresses out, so printing adds nothing to \
         tell apart:\n{on_screen}",
    );

    let first = document.with_file_name("first.pdf");
    let again = document.with_file_name("again.pdf");
    export(&app, &first);
    export(&app, &again);

    let printed = squeezed(&pdf_pages(&first).join("\n"));
    assert_eq!(
        squeezed(&pdf_pages(&again).join("\n")),
        printed,
        "two exports of one unchanged document came out different",
    );

    // What the window never shows: the address of a link, which is in the file only
    // because a print stylesheet wrote it there.
    assert!(
        printed.contains("<https://example.com/filing>"),
        "the exported file was not made by a print job — the print stylesheet's own \
         output is not in it:\n{printed}",
    );

    // Every paragraph the reader can see is in the file, and in the same order. The
    // addresses printing added are taken back out first, so what is left has to be
    // the page on screen word for word.
    let printed = without_link_addresses(&printed);
    let mut reached = 0;
    for paragraph in on_screen
        .lines()
        .map(str::trim)
        .filter(|line| line.len() > 20 && !line.contains("Load"))
    {
        let at = printed.find(&squeezed(paragraph)).unwrap_or_else(|| {
            panic!("the exported PDF is missing what the window shows: {paragraph:?}\n{printed}")
        });
        assert!(
            at >= reached,
            "{paragraph:?} came out of order in the exported PDF",
        );
        reached = at;
    }

    assert!(app.close().is_empty(), "something outlived the application");
}

/// Delivering the document is not something that happens *to* the document: the
/// reader stays where they were reading, on the page they were reading it on.
///
/// Printing lays the document out again, in the same web process that is showing it,
/// and a print job that came back having scrolled the reader elsewhere — or having
/// reloaded or re-rendered the page underneath them — would be a defect in every
/// feature that rests on the reader's place staying put (invariant 5).
#[test]
fn exporting_leaves_the_reader_where_they_were_reading() {
    let fixture = Fixture::new("print-place");
    let document = fixture.write("handbook.md", &handbook(26, &|_| 3));
    let pdf = document.with_file_name("handbook.pdf");
    let page = document.with_file_name("handbook.html");
    let app = axiomd_e2e::launch(&document);

    // The reader is a long way down, on a block the page can be asked about by name.
    app.wait_until("document.querySelector('[data-line=\"77\"]') !== null");
    app.dom("document.querySelector('[data-line=\"77\"]').scrollIntoView(true)");
    let where_they_are = app.dom("Math.round(document.scrollingElement.scrollTop)");
    assert_ne!(where_they_are, "0", "the reader never left the top");
    let (navigations, renders) = (app.navigation_count(), app.render_count());

    export(&app, &pdf);
    export(&app, &page);

    assert_eq!(
        app.dom("Math.round(document.scrollingElement.scrollTop)"),
        where_they_are,
        "delivering the document moved the reader",
    );
    assert_eq!(
        app.dom_text("[data-line=\"77\"]").trim(),
        "Paragraph 1 of section 10, long enough to take a line of its own on the page \
         and then some more.",
        "the document under the reader is not the one that was there",
    );
    assert_eq!(
        (app.navigation_count(), app.render_count()),
        (navigations, renders),
        "delivering the document reloaded or re-rendered the page the reader is on",
    );
    assert_eq!(app.banner(), format!("Exported {}", "handbook.html"));
    assert!(app.close().is_empty(), "something outlived the application");
}

/// A handbook of `sections`, each of `paragraphs` paragraphs a line long — the shape
/// that decides where on the page each heading lands.
fn handbook(sections: u32, paragraphs: &dyn Fn(u32) -> u32) -> String {
    let mut source = String::from("# Handbook\n\n");
    for section in 1..=sections {
        source.push_str(&format!("## Section {section}\n\n"));
        for line in 1..=paragraphs(section) {
            source.push_str(&format!(
                "Paragraph {line} of section {section}, long enough to take a line of \
                 its own on the page and then some more.\n\n"
            ));
        }
    }
    source
}

/// UT-013's outcome, on the crafted break-rule fixture: the break rule that matters
/// most on paper is that a heading is the start of what follows it, so it never ends a
/// page with its section overleaf.
///
/// Two shapes, because where a heading falls on the page is decided entirely by what
/// came before it: sections of even length walk the headings down the page a fixed
/// step at a time and land one at the very bottom, and sections of uneven length put
/// them at an unrepeating spread of offsets. Both are needed — the uneven fixture
/// alone passed while the rule was not working at all.
#[test]
fn no_page_ends_with_a_heading_stranded_at_the_bottom() {
    for (name, source) in [
        ("even", handbook(26, &|_| 3)),
        ("uneven", handbook(26, &|section| 2 + section % 5)),
    ] {
        let fixture = Fixture::new(&format!("print-breaks-{name}"));
        let document = fixture.write("handbook.md", &source);
        let pdf = document.with_file_name("handbook.pdf");
        let app = axiomd_e2e::launch(&document);

        export(&app, &pdf);

        let pages = pdf_pages(&pdf);
        assert!(
            pages.len() >= 3,
            "the {name} fixture no longer spans enough pages to say anything: {} page(s)",
            pages.len(),
        );
        for (at, page) in pages.iter().enumerate() {
            let last = page
                .lines()
                .rfind(|line| !line.trim().is_empty())
                .unwrap_or_default()
                .trim();
            assert!(
                !last.starts_with("Section ") && !last.starts_with("Handbook"),
                "page {} of the {name} PDF ends with the heading {last:?}, and its \
                 section is overleaf",
                at + 1,
            );
        }
        assert!(app.close().is_empty(), "something outlived the application");
    }
}

/// UT-014: the reader exports a page, mails it to somebody, and it opens on a machine
/// that has never heard of axiomd and may have no network at all.
#[test]
fn an_exported_page_carries_the_whole_document_inside_it() {
    let fixture = Fixture::new("print-html");
    let document = fixture.write(
        "notes.md",
        "# Notes\n\n![The logo](logo.png)\n\n![Far](https://cdn.example.com/far.png)\n\n\
         See [the site](https://example.com/page).\n",
    );
    // A real PNG, so "the picture is in the file" is asserted on picture bytes.
    let png = fixture.write("logo.png", "");
    std::fs::write(&png, PIXEL_PNG).expect("write the picture");
    let page = document.with_file_name("notes.html");
    let app = axiomd_e2e::launch(&document);

    export(&app, &page);

    let exported = std::fs::read_to_string(&page).expect("read the exported page");
    assert!(
        exported.contains("<style>") && !exported.contains("<link"),
        "the exported page does not carry its styling: {exported}",
    );
    assert!(
        exported.contains("src=\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUg==\""),
        "the picture did not travel with the document",
    );
    assert!(
        !exported.contains("axiomd:"),
        "the exported page still speaks to the app it left",
    );
    assert!(
        !exported.contains("Load image"),
        "the exported page offers a button that cannot do anything",
    );
    assert!(
        exported.contains("href=\"https://example.com/page\""),
        "the reader's own link did not survive",
    );
    assert!(app.close().is_empty(), "something outlived the application");
}

/// The bytes of a real one-pixel PNG header, matched against its own base64 above.
const PIXEL_PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR";

/// UT-013's first step: `Ctrl+P` puts the reader's own print dialog up — and changing
/// their mind leaves
/// them exactly where they were, with the document still on screen.
#[test]
fn the_print_dialog_opens_when_the_reader_asks_and_leaves_quietly() {
    let fixture = Fixture::new("print-dialog");
    let document = fixture.write("report.md", REPORT);
    let app = axiomd_e2e::launch(&document);

    app.activate("win.print");
    app.wait_for_dialog_saying("Print");

    app.press("Cancel");
    app.wait_for("the print dialog to go away", || {
        app.visible_dialog().is_empty()
    });

    // The window is still the window: the document is there, and nothing was said
    // about a print nobody asked to finish.
    assert_eq!(app.dom_text("h1"), "Quarterly Report");
    assert_eq!(app.banner(), "");
    assert!(app.close().is_empty(), "something outlived the application");
}
