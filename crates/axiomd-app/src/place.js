// The span map as the rendered page sees it, and the whole of how switching modes
// keeps the reader's place (issue #18).
//
// Every top-level block carries the source line it was rendered from (`data-line`), so
// a place in the page and a place in the source are the same thing said two ways.
// These two functions are the two directions of that:
//
//   topmost()      the source line of the highest block still on screen — what read
//                  mode hands to the caret when the reader presses Ctrl+E.
//   scrollTo(line) the block the caret is in, brought back to the top of the page —
//                  what edit mode hands back when they press it again.
//
// # The heuristic, stated once
//
// A source line usually falls *inside* a block rather than at its first line: the
// caret on line 84 of a paragraph that starts at line 81 means that paragraph. So the
// block a line belongs to is the last one that begins at or before it, and only a line
// before the first block falls back to the first block. This is the same rule the
// live-reload patch already uses to decide which surviving block a reader was on, so
// the two never disagree about where "here" is.
//
// Height is never consulted, in either direction — proportional scroll sync is
// Apostrophe's approach and it desynchronises on any tall block (design_decisions.md).
{
  // The source line of the first block whose bottom edge has not gone past the top of
  // the viewport. That is the block the reader is reading: one scrolled halfway off
  // the top is still the one they are in the middle of.
  topmost() {
    const blocks = document.querySelectorAll('[data-line]');
    for (const block of blocks) {
      if (block.getBoundingClientRect().bottom > 0) {
        return Number(block.dataset.line) || 1;
      }
    }
    // Everything is above the viewport — the reader is at the very end of the
    // document, so the last block is where they are.
    const last = blocks[blocks.length - 1];
    return last === undefined ? 1 : Number(last.dataset.line) || 1;
  },

  // Puts the block that source `line` belongs to at the top of the page.
  scrollTo(line) {
    const blocks = document.querySelectorAll('[data-line]');
    let target = blocks[0];
    for (const block of blocks) {
      if (Number(block.dataset.line) > line) {
        break;
      }
      target = block;
    }
    if (target === undefined) {
      return 'no blocks';
    }
    target.scrollIntoView(true);
    return String(target.dataset.line);
  },
}
