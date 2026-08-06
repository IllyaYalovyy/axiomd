//! The number at the foot of every printed page.
//!
//! # Why the number is stamped into the PDF rather than laid out with the document
//!
//! It cannot be laid out with the document. Measured for issue #19 and recorded in
//! `axiomd.css`: WebKitGTK draws neither the `@page` margin boxes CSS reserves for page
//! furniture nor a repeating fixed footer — a `position: fixed` element is painted on
//! the first page and nowhere else. There is no stylesheet that puts a number on page
//! seven, and the owner ruled on 2026-08-05 that page numbers are required.
//!
//! So the number is written after pagination, onto the paginated thing itself. Every
//! delivery — a PDF the reader exported and a job on its way to a printer alike — is a
//! PDF by the time it gets here (`export.rs`), which is what makes one mechanism serve
//! both: what axiomd prints *is* the PDF it exports.
//!
//! Nothing here converts anything and nothing is started: this reads the file WebKit
//! just wrote, appends one short content stream to each page, and writes it back. It
//! runs on a worker, never on the main loop (invariant 4).
//!
//! # What is added, and nothing else
//!
//! The owner's furniture ruling of 2026-08-02, reaffirmed on 2026-08-05: the footer is
//! the page number and nothing besides — no header, no date, no file name. One number,
//! centred, in the bottom margin the page setup reserved for it.

use std::path::Path;

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, dictionary};

/// The type the number is set in, in points.
const SIZE: f64 = 9.0;

/// How far its baseline sits above the foot of the sheet, in points — inside the
/// bottom margin `export.rs` reserves, clear of the last line of the document.
const BASELINE: f64 = 25.5;

/// How wide a digit of the face below is, in ems. Every glyph this writes is a digit,
/// so one number is the whole of the metrics it needs to centre a number.
const DIGIT: f64 = 0.556;

/// How dark the number is drawn, as a grey level: the dimmed ink the print stylesheet
/// gives anything that is not the document itself.
const INK: f64 = 0.27;

/// The name the font is added to each page's resources under. Long enough that no
/// document's own font can already be called it.
const FONT: &str = "AxiomdPageNumber";

/// Numbers every page of the PDF at `file`, in place.
///
/// The file is read, stamped and written back; on any trouble it is left exactly as it
/// was and the reason is returned, so a delivery that could not be numbered is a
/// delivery that failed rather than one that quietly came out bare.
pub(crate) fn number_the_pages(file: &Path) -> Result<(), String> {
    let mut pdf = Document::load(file).map_err(|trouble| trouble.to_string())?;
    let face = pdf.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });

    for (number, page) in pdf.get_pages() {
        let sheet = sheet(&pdf, page)?;
        let number = number.to_string();
        let across = sheet.0 + (sheet.2 - number.chars().count() as f64 * DIGIT * SIZE) / 2.0;
        let stamp = footer(&number, across, sheet.1 + BASELINE)?;
        let stamp = pdf.add_object(Stream::new(Dictionary::new(), stamp));
        lend_the_face(&mut pdf, page, face)?;
        under_a_clean_hand(&mut pdf, page, stamp)?;
    }

    pdf.save(file)
        .map(|_| ())
        .map_err(|trouble| trouble.to_string())
}

/// The sheet page `page` is drawn on: the left and bottom edge of its media box, and
/// how wide it is.
///
/// A page inherits its media box from the tree above it when it does not carry one,
/// which is how most PDFs are written — so the parents are walked rather than assumed.
fn sheet(pdf: &Document, page: ObjectId) -> Result<(f64, f64, f64), String> {
    let mut at = Some(page);
    while let Some(id) = at {
        let node = pdf
            .get_dictionary(id)
            .map_err(|trouble| format!("a page of the document is unreadable: {trouble}"))?;
        if let Ok(box2d) = node.get(b"MediaBox").and_then(Object::as_array) {
            let edges = box2d
                .iter()
                .map(|edge| edge.as_float().map(f64::from))
                .collect::<Result<Vec<f64>, _>>()
                .map_err(|trouble| format!("a page's size is unreadable: {trouble}"))?;
            let [left, bottom, right, top] = edges[..] else {
                return Err(format!("a page's size has {} edges", edges.len()));
            };
            return Ok((left.min(right), bottom.min(top), (right - left).abs()));
        }
        at = node.get(b"Parent").and_then(Object::as_reference).ok();
    }
    Err("a page of the document has no size".to_owned())
}

/// The content stream that draws `number` with its left edge at `across` and its
/// baseline at `up`, both in points from the foot of the sheet's left corner.
fn footer(number: &str, across: f64, up: f64) -> Result<Vec<u8>, String> {
    Content {
        operations: vec![
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec![FONT.into(), SIZE.into()]),
            Operation::new("g", vec![INK.into()]),
            Operation::new("Td", vec![across.into(), up.into()]),
            Operation::new("Tj", vec![Object::string_literal(number)]),
            Operation::new("ET", vec![]),
        ],
    }
    .encode()
    .map_err(|trouble| format!("the page number could not be drawn: {trouble}"))
}

/// Puts the number's font among the resources `page` may draw with.
///
/// The page's own resource dictionary, made if it has none: a page that inherited its
/// resources from the tree above keeps them, and gains one font of its own.
fn lend_the_face(pdf: &mut Document, page: ObjectId, face: ObjectId) -> Result<(), String> {
    let inherited = inherited_resources(pdf, page);
    let node = pdf
        .get_dictionary_mut(page)
        .map_err(|trouble| format!("a page of the document is unreadable: {trouble}"))?;
    let mut resources = match node.get(b"Resources") {
        Ok(Object::Dictionary(own)) => own.clone(),
        Ok(Object::Reference(_)) | Err(_) => inherited,
        Ok(_) => Dictionary::new(),
    };
    let mut fonts = match resources.get(b"Font") {
        Ok(Object::Dictionary(own)) => own.clone(),
        _ => Dictionary::new(),
    };
    fonts.set(FONT, Object::Reference(face));
    resources.set("Font", Object::Dictionary(fonts));
    node.set("Resources", Object::Dictionary(resources));
    Ok(())
}

/// Everything `page` may already draw with, resolved through however many references
/// and parents it takes to reach a dictionary.
fn inherited_resources(pdf: &Document, page: ObjectId) -> Dictionary {
    let mut at = Some(page);
    let mut seen = 0;
    while let (Some(id), true) = (at, seen < 32) {
        seen += 1;
        let Ok(node) = pdf.get_dictionary(id) else {
            break;
        };
        match node.get(b"Resources") {
            Ok(Object::Dictionary(own)) => return own.clone(),
            Ok(Object::Reference(reference)) => {
                if let Ok(own) = pdf.get_dictionary(*reference) {
                    return own.clone();
                }
            }
            _ => {}
        }
        at = node.get(b"Parent").and_then(Object::as_reference).ok();
    }
    Dictionary::new()
}

/// Appends `stamp` to `page`'s content, with the document's own drawing wrapped in a
/// saved graphics state first.
///
/// The wrapping is the whole point of doing it here rather than with lopdf's own
/// append: a content stream may end with a clip, a colour or a transform still in
/// force, and a number drawn after it would inherit all three — clipped away, in the
/// wrong colour, in the wrong place. `q` before the document and `Q` after it hand the
/// stamp a clean page.
fn under_a_clean_hand(pdf: &mut Document, page: ObjectId, stamp: ObjectId) -> Result<(), String> {
    let opening = pdf.add_object(Stream::new(Dictionary::new(), b"q\n".to_vec()));
    let closing = pdf.add_object(Stream::new(Dictionary::new(), b"\nQ\n".to_vec()));
    let node = pdf
        .get_dictionary_mut(page)
        .map_err(|trouble| format!("a page of the document is unreadable: {trouble}"))?;
    let document = match node.get(b"Contents") {
        Ok(Object::Array(streams)) => streams.clone(),
        Ok(other) => vec![other.clone()],
        Err(_) => Vec::new(),
    };
    let mut contents = vec![Object::Reference(opening)];
    contents.extend(document);
    contents.extend([Object::Reference(closing), Object::Reference(stamp)]);
    node.set("Contents", Object::Array(contents));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stamp is one drawing of one string and nothing else: a footer that had
    /// picked up an operator from somewhere would be furniture the owner ruled out.
    #[test]
    fn the_stamp_draws_the_number_and_nothing_else() {
        let drawn = footer("7", 100.0, 25.5).expect("a footer");
        let drawn = String::from_utf8_lossy(&drawn);
        assert!(drawn.contains("(7) Tj"), "the number is not drawn: {drawn}");
        assert_eq!(
            drawn.matches("Tj").count(),
            1,
            "more than the number: {drawn}"
        );
        assert!(
            !drawn.contains("Do") && !drawn.contains("re"),
            "the footer draws something besides text: {drawn}",
        );
    }
}
