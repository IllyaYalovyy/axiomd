// The page saying that it has been drawn, so the reader can be shown it (issue #41).
//
// Called in a JavaScript world of the app's own — never the document's, which cannot
// run a script at all — with the name of the message handler the app registered in that
// same world. Until this arrives the reader is looking at a surface painted the page's
// own colour, and the webview behind it is not presented at all: on the accelerated
// compositing path WebKit shows black until its web process delivers a first frame, and
// no background colour on the view changes that.
//
// # Why two frames and not one
//
// A `requestAnimationFrame` callback runs at the *start* of a rendering update, before
// the frame it belongs to has been drawn — reporting from it would say "about to be
// drawn". The callback it registers in turn runs at the start of the next update, which
// only happens once the frame before it was produced. So the second callback is the
// first moment the page can honestly say a frame of this document exists.
//
// # Why `requestAnimationFrame` and nothing else
//
// A document is displayed with `enable-javascript = false`, and under that setting
// WebKitGTK 2.52.5 runs no event listener and no timer, in any world (`track.js`). The
// rendering steps still run, and this is one of them — probed on WebKitGTK 2.52.5 on
// 2026-08-05: armed from the app's world, both callbacks ran within 500 ms while the
// webview was the surface on screen, and the same arming ran neither of them for two
// seconds while the reader was in the editor with the webview off screen, then ran both
// the moment it came back. That is also why the surface in front of the webview is an
// overlay and not a page of the window's stack: a webview nothing is drawing has no
// rendering updates to report from, and would never be shown at all.
(handler) => {
  const handlers = window.webkit && window.webkit.messageHandlers;
  const bridge = handlers && handlers[handler];
  if (!bridge) {
    return 'no bridge';
  }
  requestAnimationFrame(() => {
    requestAnimationFrame(() => bridge.postMessage(1));
  });
  return 'waiting';
}
