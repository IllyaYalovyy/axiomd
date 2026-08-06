//! What a printed page measures, read off the PDF the way a ruler reads paper.
//!
//! The print suite's other half. [`super`] reads what a PDF *says*; this reads where
//! on the sheet it says it and how big — the two questions the photographs in issue
//! #43 were the answer to. Margins, type size and the numbered footer are all one
//! measurement: the position and size of every mark on the page, in points, in the
//! page's own coordinates (origin bottom left, 72 to the inch).
//!
//! Positions come from `pdf-extract`'s own text machinery — the crate the suite
//! already reads exported PDFs with — through its [`OutputDev`] hook, which is handed
//! each glyph's text-rendering matrix, advance and size. Nothing here re-implements
//! PDF; it only collects what that hook is given.

#![allow(dead_code)]

use std::path::Path;

use pdf_extract::{ColorSpace, MediaBox, OutputDev, OutputError, Transform};

/// One page of a PDF, measured.
#[derive(Debug, Clone)]
pub struct Page {
    /// The sheet's width in points.
    pub width: f64,
    /// The sheet's height in points.
    pub height: f64,
    /// Every run of type on it, in the order it was drawn.
    pub runs: Vec<Run>,
}

/// A stretch of type drawn along one baseline at one size.
#[derive(Debug, Clone)]
pub struct Run {
    /// What it says.
    pub text: String,
    /// Where its first glyph starts, in points from the left edge.
    pub left: f64,
    /// Where its last glyph ends, in points from the left edge.
    pub right: f64,
    /// Its baseline, in points up from the bottom edge.
    pub baseline: f64,
    /// The type size it is set in, in points.
    pub size: f64,
}

impl Run {
    /// The middle of the run, in points from the left edge.
    pub fn centre(&self) -> f64 {
        (self.left + self.right) / 2.0
    }
}

/// A rectangle in page coordinates: points from the left and bottom edges.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Box2D {
    pub left: f64,
    pub right: f64,
    pub bottom: f64,
    pub top: f64,
}

impl Page {
    /// Everything on the page above `band` points from the bottom edge — the document
    /// itself, as distinct from the furniture printed in the bottom margin.
    pub fn above(&self, band: f64) -> Vec<&Run> {
        self.runs
            .iter()
            .filter(|run| run.baseline >= band)
            .collect()
    }

    /// Everything printed below `band` points from the bottom edge: the footer.
    pub fn below(&self, band: f64) -> Vec<&Run> {
        self.runs.iter().filter(|run| run.baseline < band).collect()
    }

    /// The box the given runs all fall inside, or `None` when there are none.
    ///
    /// A run's box reaches from its baseline down by a quarter of its size and up by
    /// three quarters — the descender and ascender of a Latin face, which is as close
    /// as glyph extents can be read without the font's own outlines. Both are counted
    /// against the page's margins, so the measurement errs towards saying a page is
    /// closer to the edge than it is.
    pub fn ink(runs: &[&Run]) -> Option<Box2D> {
        runs.iter().fold(None, |so_far: Option<Box2D>, run| {
            let here = Box2D {
                left: run.left,
                right: run.right,
                bottom: run.baseline - run.size * 0.25,
                top: run.baseline + run.size * 0.75,
            };
            Some(match so_far {
                None => here,
                Some(box2d) => Box2D {
                    left: box2d.left.min(here.left),
                    right: box2d.right.max(here.right),
                    bottom: box2d.bottom.min(here.bottom),
                    top: box2d.top.max(here.top),
                },
            })
        })
    }

    /// The most common type size on the page, to the nearest tenth of a point: what
    /// the reader would call the body size, since body text is most of any page.
    pub fn commonest_size(&self) -> Option<f64> {
        let mut tally: Vec<(i64, usize)> = Vec::new();
        for run in &self.runs {
            let key = (run.size * 10.0).round() as i64;
            let glyphs = run.text.chars().filter(|c| !c.is_whitespace()).count();
            match tally.iter_mut().find(|(size, _)| *size == key) {
                Some((_, count)) => *count += glyphs,
                None => tally.push((key, glyphs)),
            }
        }
        tally
            .into_iter()
            .max_by_key(|(_, count)| *count)
            .map(|(size, _)| size as f64 / 10.0)
    }
}

/// Millimetres as points, the unit a PDF is written in.
pub fn mm(millimetres: f64) -> f64 {
    millimetres * 72.0 / 25.4
}

/// Measures every page of the PDF at `file`.
///
/// Panics with the file's name when it cannot be read as a PDF, so a test that
/// produced nothing says so rather than measuring an empty vector.
pub fn measure(file: &Path) -> Vec<Page> {
    let document = pdf_extract::Document::load(file)
        .unwrap_or_else(|error| panic!("{} is not a readable PDF: {error}", file.display()));
    let mut ruler = Ruler::default();
    pdf_extract::output_doc(&document, &mut ruler)
        .unwrap_or_else(|error| panic!("{} could not be laid out: {error}", file.display()));
    ruler.finish()
}

/// Collects each glyph's position as `pdf-extract` walks a page's content.
#[derive(Default)]
struct Ruler {
    pages: Vec<Page>,
    page: Option<Page>,
    run: Option<Run>,
}

impl Ruler {
    fn finish(mut self) -> Vec<Page> {
        self.close_page();
        self.pages
    }

    fn close_run(&mut self) {
        if let (Some(page), Some(run)) = (self.page.as_mut(), self.run.take())
            && !run.text.trim().is_empty()
        {
            page.runs.push(Run {
                text: run.text.trim().to_owned(),
                ..run
            });
        }
    }

    fn close_page(&mut self) {
        self.close_run();
        if let Some(page) = self.page.take() {
            self.pages.push(page);
        }
    }
}

impl OutputDev for Ruler {
    fn begin_page(
        &mut self,
        _page: u32,
        media: &MediaBox,
        _art: Option<(f64, f64, f64, f64)>,
    ) -> Result<(), OutputError> {
        self.close_page();
        self.page = Some(Page {
            width: (media.urx - media.llx).abs(),
            height: (media.ury - media.lly).abs(),
            runs: Vec::new(),
        });
        Ok(())
    }

    fn end_page(&mut self) -> Result<(), OutputError> {
        self.close_page();
        Ok(())
    }

    fn output_character(
        &mut self,
        trm: &Transform,
        advance: f64,
        spacing: f64,
        size: f64,
        character: &str,
    ) -> Result<(), OutputError> {
        // The glyph's size on the page: the type size through the text matrix, as the
        // crate's own outputs compute it — the side of the square with the area of the
        // transformed em box, so a matrix that scales x and y differently still gives
        // one number.
        let (across, down) = (
            trm.m11 * size + trm.m21 * size,
            trm.m12 * size + trm.m22 * size,
        );
        let on_the_page = (across * down).abs().sqrt();
        let (left, baseline) = (trm.m31, trm.m32);
        let right = left + advance * on_the_page + spacing;

        let carries_on = self.run.as_ref().is_some_and(|run| {
            (run.baseline - baseline).abs() < 0.5
                && (run.size - on_the_page).abs() < 0.1
                && left >= run.right - on_the_page
                && left <= run.right + on_the_page * 2.0
        });
        match self.run.as_mut().filter(|_| carries_on) {
            Some(run) => {
                // A gap wide enough to be a space is one: the words a run says are what
                // a reader would read off the page, not the glyphs a PDF happens to
                // group.
                if left > run.right + on_the_page * 0.1 {
                    run.text.push(' ');
                }
                run.text.push_str(character);
                run.right = right;
            }
            None => {
                self.close_run();
                self.run = Some(Run {
                    text: character.to_owned(),
                    left,
                    right,
                    baseline,
                    size: on_the_page,
                });
            }
        }
        Ok(())
    }

    fn begin_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn end_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn end_line(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn stroke(
        &mut self,
        _ctm: &Transform,
        _colorspace: &ColorSpace,
        _color: &[f64],
        _path: &pdf_extract::Path,
    ) -> Result<(), OutputError> {
        Ok(())
    }

    fn fill(
        &mut self,
        _ctm: &Transform,
        _colorspace: &ColorSpace,
        _color: &[f64],
        _path: &pdf_extract::Path,
    ) -> Result<(), OutputError> {
        Ok(())
    }
}
