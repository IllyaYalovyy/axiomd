// The remote images of the document on screen, as the reader has left them.
//
// Called in a JavaScript world of the app's own — never the document's, which cannot
// run a script at all — with everything the reader has asked for so far:
//
//   { loaded: { <source>: <axiomd uri> }, failed: { <source>: <what to tell them> } }
//
// It is applied after every render, not only after a load, because that is what makes
// a loaded image survive the file changing underneath it: the patch rebuilds the block
// out of the freshly rendered placeholder, and this puts the reader's image back.
// Applying it twice must therefore be the same as applying it once, and it is —
// everything here is keyed on the placeholder's own `data-remote-src`, and a
// placeholder that has become an image is no longer a placeholder.
(state) => {
  const article = document.querySelector('article.markdown');
  if (article === null) {
    return 'no document';
  }

  const placeholders = Array.from(
    article.querySelectorAll('a.remote-image[data-remote-src]'),
  );
  for (const placeholder of placeholders) {
    const source = placeholder.getAttribute('data-remote-src');
    const loaded = state.loaded[source];
    if (loaded !== undefined) {
      const image = document.createElement('img');
      image.setAttribute('src', loaded);
      const label = placeholder.querySelector('.remote-image-label');
      image.setAttribute('alt', label === null ? '' : label.textContent);
      const title = placeholder.getAttribute('title');
      if (title !== null) {
        image.setAttribute('title', title);
      }
      placeholder.replaceWith(image);
      continue;
    }
    // A load that did not work says so where the reader clicked, and the card stays
    // a button: trying again is the same one click (`ux_decisions.md` — inline, with
    // an affordance, never a dialog).
    const failure = state.failed[source];
    const action = placeholder.querySelector('.remote-image-action');
    if (failure !== undefined && action !== null) {
      placeholder.classList.add('remote-image-failed');
      action.textContent = failure + ' Try again';
    }
  }

  // The load-all affordance is about placeholders, so it lasts exactly as long as
  // one of them does.
  const banner = article.querySelector('.remote-banner');
  if (banner !== null) {
    banner.hidden =
      article.querySelector('a.remote-image[data-remote-src]') === null;
  }

  return String(article.querySelectorAll('a.remote-image[data-remote-src]').length);
}
