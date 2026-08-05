// How a document already on screen becomes the next version of itself.
//
// Called with the new document's blocks as markup, in a JavaScript world of the app's
// own — never the document's, which cannot run a script at all. It replaces only the
// nodes whose content changed, so everything the edit did not touch keeps the very
// element it had: no flash, no images fetched again, no selection lost, and a scroll
// position that still means something afterwards.
//
// # A node is its content, not its line
//
// Inserting a paragraph at the top of a document moves the source line of every block
// below it, so `data-line` cannot be what says two blocks are the same one — matching
// on it would rebuild the whole document for a one-word edit at the top. Nodes are
// matched on their content with the line left out, and a block that is kept is given
// the line it now sits on, because the anchor map is what outline navigation, search
// and scrolling all read.
//
// # Why the match goes deeper than a block
//
// A block whose content changed is not necessarily a block the reader wants rebuilt.
// Ticking one item off a fifty-item task list changes the markup of the one `<ul>` the
// whole list is, and replacing that `<ul>` throws away the element the reader's scroll
// position was measured against — which is exactly the jump issue #38 is about. So a
// changed node is not replaced while its old counterpart can be brought up to date
// instead: same kind of node, in the same place, and not itself wanted verbatim
// somewhere further on. Then the same rule is applied to its children, and to theirs,
// until what is left to change is one attribute on one `<input>`.
//
// The one node never brought up to date in place is one a capability has drawn into. A
// drawn block keeps its source as markup and its picture in a shadow root, and the
// picture is drawn from the markup that was there when it was made: changing that
// markup underneath it would leave the reader looking at a drawing of a diagram they
// no longer have. Such a block is replaced, which is what makes the capability draw
// the new one (`mermaid-view.js`).
//
// # Where the reader ends up
//
// At the block they were looking at, which is found by identity: the element itself,
// still in the document. If the edit deleted it, the nearest block before it that
// survived takes its place — again by identity, since the lines have moved. Only when
// nothing above the reader survived at all is there nothing left to go on but the
// line, and the block nearest to it becomes the answer. A page whose blocks all stayed
// where they were is not scrolled at all, not even by nought pixels: the reader who
// pressed a checkbox must not be moved, and must not be moved and put back either.
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

  // What each node has already been found to say. Asking is what the whole patch is
  // made of and the answer is the node's markup, so asking twice about a ten megabyte
  // document is a second pass over ten megabytes. Every node is asked before anything
  // is done to it and never asked again afterwards, which is what makes one answer
  // good for the whole patch.
  const known = new WeakMap();

  // A node's content, with the source line it happens to be on left out. Words and
  // markup answer differently so that a paragraph can never be mistaken for the text
  // that reads the same.
  const contentOf = (node) => {
    const already = known.get(node);
    if (already !== undefined) {
      return already;
    }
    let content;
    if (node.nodeType !== Node.ELEMENT_NODE) {
      content = node.nodeType + ':' + node.nodeValue;
    } else {
      const line = node.getAttribute('data-line');
      if (line === null) {
        content = 'e:' + node.outerHTML;
      } else {
        node.removeAttribute('data-line');
        content = 'e:' + node.outerHTML;
        node.setAttribute('data-line', line);
      }
    }
    known.set(node, content);
    return content;
  };

  // The line a kept node now sits on. Its content matched with the line left out, so
  // this is the whole of what a block that only moved has to be told — and a block
  // that did not even move is not written to at all, because everything watching the
  // document for changes is woken by a write that changes nothing just the same.
  const reline = (kept, wanted) => {
    if (kept.nodeType !== Node.ELEMENT_NODE) {
      return;
    }
    const line = wanted.getAttribute('data-line');
    if (line === kept.getAttribute('data-line')) {
      return;
    }
    if (line === null) {
      kept.removeAttribute('data-line');
    } else {
      kept.setAttribute('data-line', line);
    }
  };

  // Whether these two are one node that changed rather than two different nodes: the
  // same kind of thing, in the same place — and, for an element, not one a capability
  // has drawn into, whose drawing is made from the markup this would change.
  const alike = (kept, wanted) =>
    kept.nodeType === wanted.nodeType &&
    (kept.nodeType !== Node.ELEMENT_NODE ||
      (kept.tagName === wanted.tagName && kept.shadowRoot === null));

  // What a walk is over. The article's own children are the document's blocks *and*
  // the newlines the page was written with between them; the markup a patch carries is
  // the blocks alone, so at the top level the blocks are matched by themselves and the
  // page's own whitespace is left exactly where it is. Below that the two sides are
  // both the renderer's own output and are matched node for node — inside a block, the
  // space between two words is content like any other.
  const BLOCKS = {
    first: (node) => node.firstElementChild,
    after: (node) => node.nextElementSibling,
    of: (node) => node.children,
  };
  const NODES = {
    first: (node) => node.firstChild,
    after: (node) => node.nextSibling,
    of: (node) => node.childNodes,
  };

  // Brings `kept` up to date with `wanted` without replacing it: what it says, what it
  // is, and then the same rule again for everything inside it.
  const become = (kept, wanted) => {
    if (kept.nodeType !== Node.ELEMENT_NODE) {
      if (kept.nodeValue !== wanted.nodeValue) {
        kept.nodeValue = wanted.nodeValue;
      }
      return;
    }
    for (const attribute of Array.from(kept.attributes)) {
      if (!wanted.hasAttribute(attribute.name)) {
        kept.removeAttribute(attribute.name);
      }
    }
    for (const attribute of Array.from(wanted.attributes)) {
      if (kept.getAttribute(attribute.name) !== attribute.value) {
        kept.setAttribute(attribute.name, attribute.value);
      }
    }
    // A box that has been pressed answers for its own state from then on and no longer
    // for the attribute it was written with — HTML's dirty checkedness flag. The render
    // is the truth about what the source says, so it is written to the state as well.
    if (kept.tagName === 'INPUT') {
      kept.checked = wanted.checked;
    }
    patchInto(kept, wanted, NODES);
  };

  // Walks the wanted children of `into` in order, keeping the old node wherever its
  // content is unchanged, bringing the old node up to date where it can be, and
  // building a new one only where neither is possible. `cursor` is the first old node
  // not yet accounted for; whatever is still at or after it when the walk ends is no
  // longer in the document.
  const patchInto = (into, from, walk) => {
    const spare = new Map();
    for (const node of walk.of(into)) {
      const content = contentOf(node);
      const same = spare.get(content);
      if (same === undefined) {
        spare.set(content, [node]);
      } else {
        same.push(node);
      }
    }
    // How many of each content the new version still wants, counted down as they are
    // accounted for. A node whose content is wanted further on is never brought up to
    // date in place: it is the very node that match is going to be made of.
    const needed = new Map();
    for (const node of walk.of(from)) {
      const content = contentOf(node);
      needed.set(content, (needed.get(content) || 0) + 1);
    }

    let cursor = walk.first(into);
    for (const node of Array.from(walk.of(from))) {
      const content = contentOf(node);
      needed.set(content, needed.get(content) - 1);
      const same = spare.get(content);
      const kept = same !== undefined && same.length > 0 ? same.shift() : null;
      if (kept !== null) {
        reline(kept, node);
        if (kept === cursor) {
          cursor = walk.after(cursor);
          continue;
        }
        into.insertBefore(kept, cursor);
        continue;
      }
      if (cursor !== null && alike(cursor, node)) {
        const held = contentOf(cursor);
        const wantedLater = needed.get(held) > 0;
        if (!wantedLater) {
          const changed = cursor;
          cursor = walk.after(cursor);
          const twins = spare.get(held);
          if (twins !== undefined) {
            const at = twins.indexOf(changed);
            if (at !== -1) {
              twins.splice(at, 1);
            }
          }
          become(changed, node);
          continue;
        }
      }
      into.insertBefore(node, cursor);
    }
    while (cursor !== null) {
      const gone = cursor;
      cursor = walk.after(cursor);
      gone.remove();
    }
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
  patchInto(article, staging, BLOCKS);

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
      const moved = target.getBoundingClientRect().top - place.top;
      if (moved !== 0) {
        const scroller = document.scrollingElement || document.documentElement;
        scroller.scrollTop += moved;
      }
    }
  }

  return 'patched';
}
