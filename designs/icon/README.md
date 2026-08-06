# App icon reference

Five generated candidates for the axiomd application icon. The owner chose
**`axiomd-alt-3-reader-layout.svg`** (2026-08-05) — the app's own reading
layout: outline sidebar with position dots, heading bar, body lines, a
checked task, and the Markdown badge.

These files are design references, not shipped assets. The shipped icon
lives under `data/icons/hicolor/`. The other four candidates are kept for
context and are not to be shipped.

## The polished icon (issue #46)

The chosen concept was polished — not redesigned — and shipped as
`data/icons/hicolor/scalable/apps/io.github.etf.axiomd.svg`, with a
matching monochrome form as
`data/icons/hicolor/symbolic/apps/io.github.etf.axiomd-symbolic.svg`. Every
element of the concept is still there; what changed is the execution:

- the page sits on the Adwaita icon template's portrait footprint (88 wide,
  inset 8 from the top, centred), measured off the app icons Nautilus and
  Evince draw on that template rather than off a recollection of it;
- every edge is on the 4-unit grid, which is one whole pixel when the dock
  draws the icon at 32;
- text lines, sidebar dots, the checkbox and its check are thick enough to
  survive that size, and the body text carries real contrast rather than a
  38% wash;
- the sidebar, the heading bar and the checkbox share one gradient, in user
  space, so they are lit by the same light;
- the badge is larger and sits on a rim that keeps its silhouette when the
  glyph is gone;
- the drop shadow is baked as two offset rectangles: no SVG filter, so no
  renderer has an opinion about it.

`axiomd-reader-layout-no-badge.svg` is the one variant issue #46 sanctions —
the same drawing without the badge — kept here so the small sizes can be
compared with and without it. It is a reference, not a second shipped asset.

## The contact sheet is what the owner pins

`contact-sheet.png` is the icon as librsvg actually draws it — every tile
rasterised at its true size and never scaled, on a bright background and a
dark one, with the small sizes also magnified nearest-neighbour so the pixels
the dock is given can be seen. Regenerate it with:

```bash
python3 scripts/icon-contact-sheet.py
```

Approving that sheet is the merge gate for the asset itself: a subjective
surface, human-approved once (`docs/TESTING.md`, category 1). An agent may
propose a sheet; only the owner accepts one. What is *not* subjective is
asserted by `crates/axiomd-app/tests/icon.rs`, which reads the pixels
gdk-pixbuf produces and fails if the icon stops reading as a page with a blue
sidebar, a heading, text and a checked task at 128, 64, 48 or 32 — on either
background.
