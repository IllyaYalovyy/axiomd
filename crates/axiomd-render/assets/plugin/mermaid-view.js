// Mermaid diagrams, drawn in the page the reader is looking at (issue #13).
//
// Run by the application, once per page, in a JavaScript world of its own — never the
// document's, which cannot run a script at all — and only for a document that has a
// diagram in it. It runs *ahead* of the library it drives, which is why the first
// thing in it is a clock.
//
// # Why there is a clock in here
//
// A document is displayed with `enable-javascript = false`, and under that setting
// WebKitGTK 2.52.5 runs no timer and no event listener, in any world (probed here and
// already relied on by `track.js`). What it does still run is the rendering steps:
// `requestAnimationFrame` fires, and so do observer callbacks. Mermaid is written for
// a browser and reads the timer functions off the global object as it loads, so this
// file puts them back, built out of animation frames — and does it before the library
// is loaded, which is the whole of why the manifest lists this file first.
//
// Everything else here follows from the same three tools: frames, an
// `IntersectionObserver` for what is near the reader, a `MutationObserver` for blocks
// the app has just patched in, and a `ResizeObserver` for the one question that has no
// observer of its own — whether the desktop went dark.
//
// # Why a diagram lives in a shadow root
//
// A drawn diagram is not in the document. It is in a shadow root attached to the block,
// which leaves the block's own markup exactly as the pipeline wrote it — the diagram's
// source, as a code block. Three things fall out of that, and each of them would
// otherwise have had to be built:
//
//   * `patch.js` matches blocks by their markup, so a document that is re-rendered
//     under a reader — a live reload, a keystroke in the editor — keeps every diagram
//     it had already drawn instead of flashing back to source and drawing again;
//   * a diagram's own stylesheet is scoped to its shadow root, so no `#mermaid-3 .node`
//     rule can reach the document or another diagram;
//   * switching the capability off gives the reader back the source, because the source
//     never went anywhere.
//
// The page refuses styling that arrives as markup (`style-src axiomd:`, and rightly:
// a document may not carry styling of its own), so everything a diagram is styled with
// goes in through the CSSOM instead — a constructed stylesheet for what the library
// wrote as `<style>`, and `element.style` for what it wrote as an attribute. The
// document's policy is not loosened by a word for any of this.
//
// # Nothing is fetched
//
// Mermaid is initialised with `securityLevel: 'strict'`, so a diagram cannot carry a
// click handler or unsanitised markup; the page's own policy admits no request in any
// case. Both are asserted against a hostile diagram in `axiomd-app/tests/mermaid.rs`.
(() => {
  const article = document.querySelector('article.markdown');
  if (article === null) {
    return 'no document';
  }
  // One driver to a page. The app runs a document's scripts once per page load, so
  // this is a guard rather than a mechanism — and it is also the mark a test reads to
  // say the library reached a document that has a diagram, and only such a document.
  if (document.documentElement.dataset.axiomdMermaid !== undefined) {
    return 'already drawing';
  }
  document.documentElement.dataset.axiomdMermaid = 'drawing';

  // A timer, out of animation frames. The delay is dropped: what the library wants is
  // "later, off this stack", and the next frame is the soonest later there is here.
  const due = new Map();
  let ticket = 0;
  globalThis.setTimeout = (run, delay, ...rest) => {
    void delay;
    ticket += 1;
    const id = ticket;
    due.set(id, () => run(...rest));
    requestAnimationFrame(() => {
      const call = due.get(id);
      due.delete(id);
      if (call !== undefined) {
        call();
      }
    });
    return id;
  };
  globalThis.clearTimeout = (id) => {
    due.delete(id);
  };
  // A repeating timer would be a loop nothing in a rendered document has any use for.
  globalThis.setInterval = () => 0;
  globalThis.clearInterval = () => {};

  // The blocks this draws into, and how far ahead of the reader it draws them: one
  // viewport in each direction, so a diagram is ready by the time it arrives rather
  // than drawn while they watch.
  const BLOCK = 'div.plugin-mermaid';
  const NEAR = '100% 0px';

  const sheetOf = (css) => {
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(css);
    return sheet;
  };

  // What a diagram's own shadow root is styled with, beside whatever the library
  // wrote for that particular drawing.
  const SHADOW = sheetOf(
    ':host { display: block; }' +
      '.diagram { max-width: 100%; overflow-x: auto; }' +
      '.plugin-badge { margin: 0.4rem 0 0; padding-left: 0.6rem;' +
      ' border-left: 3px solid var(--axiomd-warning); color: var(--axiomd-fg-dim);' +
      ' font-size: 0.85em; text-align: left; }',
  );

  // The one element on the page that changes size when the desktop changes colour.
  // A media query cannot be listened to — there are no event listeners here — but a
  // box it resizes can be observed, and that is the same fact arriving by the one
  // road that is open.
  document.adoptedStyleSheets = [
    ...document.adoptedStyleSheets,
    sheetOf(
      '.axiomd-scheme { position: fixed; top: 0; left: 0; width: 1px; height: 1px;' +
        ' opacity: 0; pointer-events: none; }' +
        '@media (prefers-color-scheme: dark) { .axiomd-scheme { width: 2px; } }',
    ),
  ];

  const dark = () => matchMedia('(prefers-color-scheme: dark)').matches;

  const rootOf = (block) =>
    block.shadowRoot === null ? block.attachShadow({ mode: 'open' }) : block.shadowRoot;

  // What the reader is told when a diagram cannot be drawn: the source they wrote,
  // still there, and one line saying why — the same shape, and the same words, as a
  // block the pipeline's own plugin layer could not draw. Never a dialog, never a
  // blank box (`ux_decisions.md`).
  const complain = (block, failure) => {
    const said = String((failure && failure.message) || failure || 'it could not be drawn');
    const root = rootOf(block);
    root.innerHTML = '<slot></slot><p class="plugin-badge"></p>';
    root.adoptedStyleSheets = [SHADOW];
    root.querySelector('.plugin-badge').textContent =
      // The plugin's own name, as preferences shows it (issue #31): the same
      // capability must not be two different words to one reader.
      'Mermaid Diagrams could not draw this diagram: ' + said.split('\n')[0].trim();
  };

  const show = (block, svg) => {
    const root = rootOf(block);
    root.innerHTML = '<div class="diagram"></div>';
    const holder = root.firstElementChild;
    holder.innerHTML = svg;
    // The library's styling, back in by a road the page's policy leaves open. Scoped
    // to this shadow root, so a diagram's rules reach this diagram and nothing else.
    const sheets = [SHADOW];
    for (const embedded of holder.querySelectorAll('style')) {
      sheets.push(sheetOf(embedded.textContent));
      embedded.remove();
    }
    root.adoptedStyleSheets = sheets;
    for (const styled of holder.querySelectorAll('[style]')) {
      styled.style.cssText = styled.getAttribute('style');
    }
  };

  let drawings = 0;
  const draw = (block) => {
    const code = block.querySelector('code');
    drawings += 1;
    mermaid.initialize({
      startOnLoad: false,
      securityLevel: 'strict',
      theme: dark() ? 'dark' : 'default',
    });
    mermaid.render('axiomd-diagram-' + drawings, code === null ? '' : code.textContent).then(
      (drawn) => show(block, drawn.svg),
      (failure) => complain(block, failure),
    );
  };

  // Which blocks have been handed to the library, so that a block on its way to being
  // drawn is not started twice. Held here rather than as an attribute on the block:
  // an attribute would change the block's markup, and the patch reads that markup to
  // decide whether the reader's diagram survives their next keystroke.
  const taken = new WeakSet();

  const near = new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        if (!entry.isIntersecting || taken.has(entry.target)) {
          continue;
        }
        taken.add(entry.target);
        near.unobserve(entry.target);
        draw(entry.target);
      }
    },
    { rootMargin: NEAR },
  );

  const watch = () => {
    for (const block of article.querySelectorAll(BLOCK)) {
      if (!taken.has(block)) {
        near.observe(block);
      }
    }
  };

  // The document changing under the reader: blocks the app has just patched in are
  // watched, and blocks it took out are let go of.
  new MutationObserver((records) => {
    for (const record of records) {
      for (const gone of record.removedNodes) {
        if (gone.nodeType === Node.ELEMENT_NODE && gone.matches(BLOCK)) {
          near.unobserve(gone);
        }
      }
    }
    watch();
  }).observe(article, { childList: true, subtree: true });

  // The desktop changing colour: every diagram already drawn is drawn again in the
  // other palette. Only the drawn ones — a diagram that failed to parse fails in
  // either colour, and re-running it would only take its message away and put it back.
  const probe = document.createElement('div');
  probe.className = 'axiomd-scheme';
  document.body.appendChild(probe);
  let scheme = null;
  new ResizeObserver(() => {
    const width = Math.round(probe.getBoundingClientRect().width);
    if (scheme !== null && scheme !== width) {
      for (const block of article.querySelectorAll(BLOCK)) {
        if (block.shadowRoot !== null && block.shadowRoot.querySelector('svg') !== null) {
          draw(block);
        }
      }
    }
    scheme = width;
  }).observe(probe);

  watch();
  return 'drawing';
})();
