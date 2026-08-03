// How a document already on screen becomes the next version of itself.
//
// Called with the new document's blocks as markup, in a JavaScript world of the app's
// own — never the document's, which cannot run a script at all. It replaces only the
// blocks whose content changed, so everything the edit did not touch keeps the very
// element it had: no flash, no images fetched again, no selection lost, and a scroll
// position that still means something afterwards.
//
// # A block is its content, not its line
//
// Inserting a paragraph at the top of a document moves the source line of every block
// below it, so `data-line` cannot be what says two blocks are the same one — matching
// on it would rebuild the whole document for a one-word edit at the top. Blocks are
// matched on their content with the line left out, and a block that is kept is given
// the line it now sits on, because the anchor map is what outline navigation, search
// and scrolling all read.
//
// # Where the reader ends up
//
// At the block they were looking at, which is found by identity: the element itself,
// still in the document. If the edit deleted it, the nearest block before it that
// survived takes its place — again by identity, since the lines have moved. Only when
// nothing above the reader survived at all is there nothing left to go on but the
// line, and the block nearest to it becomes the answer.
(next, stylesheets) => {
  const article = document.querySelector('article.markdown');
  if (article === null) {
    return 'no document';
  }

  // The styling the document needs beyond the bundled sheet, which lives in the head
  // and so is not part of the article this patch replaces. A capability switched on or
  // off between two renders changes exactly this list: the sheet its blocks need has to
  // arrive with them, and the sheet nothing needs any more has to go.
  const wanted = new Set(stylesheets);
  for (const link of document.querySelectorAll('link[rel="stylesheet"]')) {
    const href = link.getAttribute('href') || '';
    if (href.startsWith('axiomd://assets/plugin/') && !wanted.delete(href)) {
      link.remove();
    }
  }
  for (const href of wanted) {
    const link = document.createElement('link');
    link.rel = 'stylesheet';
    link.href = href;
    document.head.appendChild(link);
  }

  // A block's content, with the source line it happens to be on left out.
  const contentOf = (block) => {
    const line = block.getAttribute('data-line');
    if (line === null) {
      return block.outerHTML;
    }
    block.removeAttribute('data-line');
    const content = block.outerHTML;
    block.setAttribute('data-line', line);
    return content;
  };

  // The document as it stands, held by identity: which of these elements are still in
  // the document afterwards is how the reader is put back where they were.
  const standing = Array.from(article.children);

  // Where the reader is: the topmost block still on screen, and how far below the top
  // of the viewport it starts.
  let place = null;
  for (let index = 0; index < standing.length; index++) {
    const box = standing[index].getBoundingClientRect();
    if (box.bottom > 0) {
      place = {
        index: index,
        line: Number(standing[index].dataset.line),
        top: box.top,
      };
      break;
    }
  }

  const staging = document.createElement('div');
  staging.innerHTML = next;

  const spare = new Map();
  for (const block of standing) {
    const content = contentOf(block);
    const same = spare.get(content);
    if (same === undefined) {
      spare.set(content, [block]);
    } else {
      same.push(block);
    }
  }

  // Walk the wanted blocks in order, keeping the old element wherever its content is
  // unchanged and building a new one only where it is not. `cursor` is the first old
  // block not yet accounted for; whatever is still at or after it when the walk ends
  // is no longer in the document.
  let cursor = article.firstElementChild;
  for (const wanted of Array.from(staging.children)) {
    const same = spare.get(contentOf(wanted));
    const kept = same !== undefined && same.length > 0 ? same.shift() : null;
    if (kept !== null) {
      const line = wanted.getAttribute('data-line');
      if (line === null) {
        kept.removeAttribute('data-line');
      } else {
        kept.setAttribute('data-line', line);
      }
      if (kept === cursor) {
        cursor = cursor.nextElementSibling;
        continue;
      }
    }
    article.insertBefore(kept === null ? wanted : kept, cursor);
  }
  while (cursor !== null) {
    const gone = cursor;
    cursor = cursor.nextElementSibling;
    gone.remove();
  }

  // Put the reader back where they were, measured from the content they were reading
  // rather than from the height of the document.
  if (place !== null) {
    let target = null;
    for (let index = place.index; target === null && index >= 0; index--) {
      if (standing[index].isConnected) {
        target = standing[index];
      }
    }
    if (target === null && Number.isFinite(place.line)) {
      for (const block of article.children) {
        const line = Number(block.dataset.line);
        if (!Number.isFinite(line)) {
          continue;
        }
        if (line > place.line) {
          break;
        }
        target = block;
      }
    }
    if (target !== null) {
      const scroller = document.scrollingElement || document.documentElement;
      scroller.scrollTop += target.getBoundingClientRect().top - place.top;
    }
  }

  return 'patched';
}
