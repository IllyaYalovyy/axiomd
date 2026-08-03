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
//   section()      the same question asked of headings rather than of blocks: which
//                  section the reader is in, for the outline to highlight (issue #7).
//   headings()     the elements that question is answered from, and the ones the
//                  outline's bridge watches (`track.js`).
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

  // Every heading a reader can be sent to, in document order: the anchored ones,
  // which are exactly the entries the outline lists (`Rendered::outline`).
  headings() {
    return document.querySelectorAll(
      'h1[data-line], h2[data-line], h3[data-line], ' +
        'h4[data-line], h5[data-line], h6[data-line]',
    );
  },

  // How near the top of the page a heading has to come to count as reached: a
  // hundredth of the window's height.
  //
  // The slack is load-bearing, not decoration. Restoring a reader's place after a live
  // reload lands within a pixel or two of where they were, and gliding a heading to
  // the top can leave it a fraction of a device pixel below it. With no slack, a
  // document that changes under somebody reading its fourth section reads as its
  // third — which is the sidebar telling them they are somewhere they are not.
  band() {
    return (document.documentElement.clientHeight || 0) / 100;
  },

  // The same line, written as the root of an `IntersectionObserver`: everything above
  // it, a million pixels up and the whole window less that hundredth down. `track.js`
  // watches with this, so the crossings that wake the bridge are exactly the crossings
  // `section` changes its answer at — the two are here together so they cannot drift.
  watched() {
    return '1000000px 0px -99% 0px';
  },

  // The section the reader is in, as its heading's source line — or 0 while they are
  // still above the first heading, which is a real place to be and not a section.
  //
  // The same rule as `topmost`, asked of headings: the last one whose top edge has
  // reached the top of the page. Answering it costs the document's headings rather
  // than its blocks, which is what makes it affordable to ask on every frame in which
  // the answer could have changed.
  section() {
    const band = this.band();
    let line = 0;
    for (const heading of this.headings()) {
      if (heading.getBoundingClientRect().top > band) {
        break;
      }
      line = Number(heading.dataset.line) || line;
    }
    return line;
  },

  // Puts the block that source `line` belongs to at the top of the page.
  //
  // `smooth` is what tells the reader they moved rather than that the document
  // changed: picking a section in the outline glides there, where restoring a place
  // they never left — a mode switch, a live reload — must simply already be there.
  scrollTo(line, smooth) {
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
    target.scrollIntoView({
      behavior: smooth ? 'smooth' : 'auto',
      block: 'start',
      inline: 'nearest',
    });
    return String(target.dataset.line);
  },
}
