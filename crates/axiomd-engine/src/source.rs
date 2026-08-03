//! The document a parse is reading, and the span arithmetic every engine needs.
//!
//! Spans are load-bearing (invariant 3), and every engine has to get the same four
//! things right about them: which line a byte is on, that a range never splits a
//! character, that a child never escapes its parent, and that an indented code block
//! carries the indentation that makes it code. Getting them right once here is what
//! keeps two engines from drifting apart on the properties four features depend on.

use std::ops::Range;

use crate::boundary::Span;

/// The source text of one parse, indexed by line.
pub(crate) struct Source<'a> {
    text: &'a str,
    /// Byte offset of the first character of each line, indexed by line - 1.
    line_starts: Vec<usize>,
}

impl<'a> Source<'a> {
    pub(crate) fn new(text: &'a str) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(text.match_indices('\n').map(|(at, _)| at + 1));
        Self { text, line_starts }
    }

    pub(crate) fn text(&self) -> &'a str {
        self.text
    }

    /// 1-based line number containing a byte offset.
    pub(crate) fn line_of(&self, offset: usize) -> u32 {
        self.line_starts.partition_point(|&start| start <= offset) as u32
    }

    /// Byte offset of a 1-based line/column pair, clamped into the source.
    ///
    /// For engines that report positions the way a text editor does rather than as
    /// byte ranges.
    pub(crate) fn offset(&self, line: usize, column: usize) -> usize {
        let base = self
            .line_starts
            .get(line.saturating_sub(1))
            .copied()
            .unwrap_or(self.text.len());
        base.saturating_add(column.saturating_sub(1))
            .min(self.text.len())
    }

    /// A byte range as a span: inside the source, never splitting a character, never
    /// running backwards.
    ///
    /// A span that splits a character cannot slice the source, and slicing the source
    /// is the whole of what a span is for.
    pub(crate) fn span(&self, range: Range<usize>) -> Span {
        let mut start = range.start.min(self.text.len());
        while start > 0 && !self.text.is_char_boundary(start) {
            start -= 1;
        }
        let mut end = range.end.clamp(start, self.text.len());
        while end < self.text.len() && !self.text.is_char_boundary(end) {
            end += 1;
        }
        Span {
            range: start..end,
            line: self.line_of(start),
        }
    }

    /// Confines a span to the block containing it.
    ///
    /// Parsers occasionally report a child that overshoots its parent — a list item
    /// that swallows the blank line after the list ends, the phantom cell GFM adds to a
    /// short table row. Outline, scroll sync and search all assume a child's source
    /// lies inside its parent's, so the boundary makes that true rather than passing
    /// the inconsistency on.
    pub(crate) fn clamped(&self, span: Span, parent: Option<&Range<usize>>) -> Span {
        let Some(parent) = parent else {
            return span;
        };
        let start = span.range.start.clamp(parent.start, parent.end);
        let end = span.range.end.clamp(start, parent.end);
        if start == span.range.start && end == span.range.end {
            return span;
        }
        Span {
            range: start..end,
            line: self.line_of(start),
        }
    }

    /// Widens a span leftwards over the whitespace that precedes it on its line.
    ///
    /// An indented code block *is* its indentation: a parser points at the first
    /// content character, so slicing by that would yield text that no longer parses as
    /// code. Stopping at the first non-whitespace byte keeps the span inside any
    /// container marker (`>`, `-`) on the same line.
    pub(crate) fn over_indent(&self, span: Span) -> Span {
        let bytes = self.text.as_bytes();
        let mut start = span.range.start;
        while start > 0 && matches!(bytes[start - 1], b' ' | b'\t') {
            start -= 1;
        }
        Span {
            range: start..span.range.end.max(start),
            line: self.line_of(start),
        }
    }
}
