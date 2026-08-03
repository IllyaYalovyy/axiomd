// The reader's search, over the words of the document on screen (issue #8).
//
// Called in a JavaScript world of the app's own — never the document's, which cannot
// run a script at all — exactly as `patch.js` and `place.js` are. Three questions, and
// between them the whole of what a search does to a page:
//
//   apply(text, cased, nth, bring)  every occurrence marked, the nth one made current
//   select(nth, bring)              a different one made current, nothing re-scanned
//   clear()                         every mark taken back out, leaving the page as it
//                                   was down to the text node
//
// # Why the app marks the page rather than asking WebKit to find for it
//
// `WebKitFindController` is the obvious alternative and it cannot do this job. Its
// whole answer is a count — `found-text` and `counted-matches` carry a match *total*
// and nothing else — so there is no way to say *which* match the reader is on, and the
// counter is the requirement the issue says wins. It offers no way to style what it
// finds either (`WebKitFindOptions` is case, word starts, direction and wrap; probed
// against the 6.0 API), so "the current match looks different from the others" and
// "the highlight is legible in light and in dark" are both unreachable. And it marks
// nothing a test can read: what it highlights is not in the DOM, where every other
// assertion this project makes about a document lives.
//
// # A match is text, not markup
//
// The document is walked as one string of its text nodes, so what is searched is what
// the reader can read: `[needle](https://example.com/needle)` is the word `needle`
// once here and twice in the source, and a search for `example.com` finds nothing at
// all in the rendered page. Text that is in the document but not on the page — the
// source under a diagram a capability has drawn — is not read either, for the same
// reason and by the same rule. A match that runs across an element boundary — the `bo` of
// a bolded `**bo**ld` — is marked in as many pieces as it crosses, all carrying the
// same match number, so it counts once and highlights whole.
//
// # Case
//
// Character by character rather than by lowercasing the whole document: the lowercase
// of a string is not always the same length as the string (U+0130 is the standard
// example), and an offset that drifts by one puts the mark on the wrong letters. The
// same rule, character by character, is what the source buffer is searched with in
// `find.rs`, so the two surfaces cannot disagree about what matches.
{
  // Whether `needle` stands at `at` in `haystack`.
  matchesAt(haystack, at, needle, cased) {
    for (let step = 0; step < needle.length; step++) {
      const here = haystack[at + step];
      const wanted = needle[step];
      if (here === wanted) {
        continue;
      }
      if (cased || here.toLowerCase() !== wanted.toLowerCase()) {
        return false;
      }
    }
    return true;
  },

  // Marks every occurrence of `text` and answers how many there are.
  apply(text, cased, nth, bring) {
    this.clear();
    const article = document.querySelector('article.markdown');
    if (article === null || text === '') {
      return '0';
    }

    // The document as one string, and where each text node starts in it.
    //
    // Text the reader cannot see is not part of it. A block a capability has drawn
    // keeps the source it was drawn from — that is what makes a diagram survive a
    // re-render, and what comes back if the capability is switched off — but the
    // reader is looking at a picture, and a search that counted the words underneath
    // it would report matches nothing could be scrolled to. Asked of the parent
    // element rather than of the text node, and remembered while the parent does not
    // change, because text nodes come in runs under one element.
    const walker = document.createTreeWalker(article, NodeFilter.SHOW_TEXT);
    const nodes = [];
    const starts = [];
    let whole = '';
    let parent = null;
    let readable = false;
    for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
      if (node.parentElement !== parent) {
        parent = node.parentElement;
        readable = parent !== null && parent.checkVisibility();
      }
      if (!readable) {
        continue;
      }
      nodes.push(node);
      starts.push(whole.length);
      whole += node.data;
    }

    // Where the matches are in it. Occurrences do not overlap — `aa` is in `aaa`
    // once — which is what every find bar the reader has used does.
    const found = [];
    for (let at = 0; at + text.length <= whole.length; ) {
      if (this.matchesAt(whole, at, text, cased)) {
        found.push(at);
        at += text.length;
      } else {
        at += 1;
      }
    }

    // Each match cut up into the pieces of it that live in one text node.
    const pieces = new Map();
    let cursor = 0;
    found.forEach((start, match) => {
      const end = start + text.length;
      while (cursor < nodes.length && starts[cursor] + nodes[cursor].data.length <= start) {
        cursor += 1;
      }
      for (let at = cursor; at < nodes.length && starts[at] < end; at++) {
        const from = Math.max(start, starts[at]) - starts[at];
        const to = Math.min(end, starts[at] + nodes[at].data.length) - starts[at];
        if (to > from) {
          const list = pieces.get(at);
          if (list === undefined) {
            pieces.set(at, [{ from: from, to: to, match: match }]);
          } else {
            list.push({ from: from, to: to, match: match });
          }
        }
      }
    });

    // Back to front within each node: splitting a text node leaves everything before
    // the split in the node itself, so offsets smaller than the one just used are
    // still the offsets they were.
    for (const [at, list] of pieces) {
      const node = nodes[at];
      list.sort((one, other) => other.from - one.from);
      for (const piece of list) {
        const tail = node.splitText(piece.from);
        tail.splitText(piece.to - piece.from);
        const mark = document.createElement('mark');
        mark.className = 'axiomd-find';
        mark.setAttribute('data-find', String(piece.match));
        tail.parentNode.replaceChild(mark, tail);
        mark.appendChild(tail);
      }
    }

    this.select(nth, bring);
    return String(found.length);
  },

  // Makes match `nth` the current one — the only thing pressing Next has to do, since
  // the marks are already where they belong.
  select(nth, bring) {
    const marks = document.querySelectorAll('mark.axiomd-find');
    let total = 0;
    for (const mark of marks) {
      total = Math.max(total, Number(mark.getAttribute('data-find')) + 1);
      mark.classList.remove('current');
    }
    if (total === 0) {
      return '0';
    }
    const wanted = ((nth % total) + total) % total;
    const current = document.querySelectorAll('mark.axiomd-find[data-find="' + wanted + '"]');
    for (const mark of current) {
      mark.classList.add('current');
    }
    // Instantly, and only when the reader asked to be moved. Gliding would make every
    // press of Next wait for an animation, and a document that changed under a reader
    // with the bar open must not scroll at all (invariant 5).
    if (bring && current.length > 0) {
      current[0].scrollIntoView({ block: 'center', inline: 'nearest' });
    }
    return String(total);
  },

  // Takes the search back out of the page, down to the text nodes it split.
  //
  // Normalising the parent is what makes this exact rather than merely close: the page
  // is compared block by block against the next render (`patch.js`), and a paragraph
  // left holding three text nodes where it had one would be rebuilt as changed.
  clear() {
    const marks = document.querySelectorAll('mark.axiomd-find');
    for (const mark of marks) {
      const parent = mark.parentNode;
      while (mark.firstChild !== null) {
        parent.insertBefore(mark.firstChild, mark);
      }
      parent.removeChild(mark);
      parent.normalize();
    }
    return String(marks.length);
  },
}
