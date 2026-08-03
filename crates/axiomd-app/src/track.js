// The page telling the app which section the reader is in (issue #7).
//
// Called in a JavaScript world of the app's own — never the document's, which cannot
// run a script at all — with `place.js` and the name of the message handler the app
// registered in that same world. The app re-runs it after every render, and running it
// again is the whole of what a re-render costs the bridge: the old watch is dropped,
// the new blocks are watched, and the reader's section is reported once.
//
// # Why an observer and not a scroll listener
//
// A document is displayed with `enable-javascript = false`, and under that setting
// WebKitGTK 2.52.5 runs *no* event listener and *no* timer, in any world — probed on
// this machine: a listener added from an isolated world never fires, for a real scroll
// or for a dispatched one, and `setTimeout` never runs. What it does still run is the
// rendering steps: `requestAnimationFrame` fires, and so do observer callbacks. So the
// bridge is an `IntersectionObserver`, which is the throttle a scroll listener would
// have had to build by hand, and a cheaper one than it:
//
//   * its callback is delivered once per rendering update, with every heading that
//     crossed in that frame batched into it — a jump from the top of a document to
//     its end crosses every section and reports once, not once per section;
//   * it says nothing at all while the reader scrolls *inside* a section, because
//     nothing they can see has changed section;
//   * it watches the document's headings rather than its blocks, so what it costs is
//     the size of the outline and not the size of the document.
//
// The alternative — the app asking the page where the reader is on a timer — would be
// work every frame forever instead of work when the answer changes.
//
// # What it watches for
//
// `place.watched()` makes the observer's root everything above the top of the page: a
// heading "intersects" it exactly when it has reached the line `place.section()` reads
// the reader's section from. Trigger and rule are the same line, written together in
// `place.js`, so every crossing wakes the bridge and nothing else does. Probed on
// WebKitGTK 2.52.5 across all four movements that matter: gliding a heading to the top
// (one report, and the right one), jumping to the end of the document (one report for
// every section it passed), jumping back to the top (one report), and reading on
// inside a section (none).
(place, handler) => {
  const handlers = window.webkit && window.webkit.messageHandlers;
  const bridge = handlers && handlers[handler];
  if (!bridge) {
    return 'no bridge';
  }
  const report = () => bridge.postMessage(place.section());

  // The headings this was watching may have been replaced by the render that is
  // calling it again, and an observer watching elements that have left the document
  // watches nothing. There is exactly one of these per page, held in this world's own
  // global — which is the app's, and which no document can reach.
  if (window.axiomdWatch !== undefined) {
    window.axiomdWatch.disconnect();
  }
  const watch = new IntersectionObserver(report, { rootMargin: place.watched() });
  window.axiomdWatch = watch;
  const headings = place.headings();
  for (const heading of headings) {
    watch.observe(heading);
  }

  // Watching anything at all reports by itself: an observer delivers where everything
  // it was given stands as soon as it is given it. A document with no headings watches
  // nothing, and still has to say so.
  if (headings.length === 0) {
    report();
  }
  return 'tracking';
}
