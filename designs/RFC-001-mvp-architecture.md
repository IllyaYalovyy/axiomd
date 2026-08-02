# RFC-001: MVP Architecture — Engine Boundary, Rendering Pipeline, App Shell

| Field | Value |
|---|---|
| Status | Draft |
| Author(s) | Illya Yalovyy (drafted with AI assistance) |
| Supersedes | - |
| Superseded by | - |

## Summary

axiomd is a native Markdown viewer for modern GNOME, written in Rust. This
RFC fixes the MVP architecture: an in-process parser behind a sealed engine
boundary (comrak as the first engine), a rendering pipeline that pre-renders
everything possible in Rust (HTML with source anchors, MathML math, syntect
highlighting, sanitization) and displays it in a WebKitGTK 6 webview, and a
gtk4-rs + libadwaita app shell with one document per window. It also defines
the MVP scope and the ordered development plan the GitHub issues mirror.

## Goals

- **G1** — Obsidian-grade rendering fidelity: 100 % CommonMark + GFM
  conformance, plus math, callouts, wikilinks, footnotes, task lists, and
  mermaid, verified by golden tests.
- **G2** — Instant and incremental: no subprocess, no full-page reload on
  change, cold start < 300 ms, 10 MB documents usable, budgets enforced by
  tests.
- **G3** — Swappable engines: the view layer never sees parser types;
  a second engine can be added without touching rendering or UI.
- **G4** — Native modern GNOME: GTK4 + libadwaita, HIG-compliant, adaptive,
  themed live, flatpak-packaged with no network permission.

## Non-Goals

- **NG1** — Editing. axiomd renders; it does not modify documents (the one
  exception, checkbox toggling, is post-MVP and gated on its own decision).
- **NG2** — Multi-format export (pandoc territory). Print/PDF is post-MVP.
- **NG3** — Vault/notebook management, indexing, or sync.
- **NG4** — A second engine in MVP. The boundary ships; only comrak ships
  behind it.

## Background and Motivation

The Apostrophe review (see `../apostrophe` clone) found the slowness is
structural, not incidental: a 10 ms "debounce" triggers a full `pandoc`
subprocess fork and a full `WebKit.load_html()` page reload per change;
three forked Python processes per window re-regex the whole document per
keystroke; math is fetched from a CDN (hence the flatpak's network hole);
scroll sync is proportional and desynchronizes on tall blocks; several
class-attribute leaks make state shared across windows. None of this is
fixable by patching — the rewrite inverts each decision by design (see
`design_decisions.md`).

Research findings (August 2026) that shape the choices below:

- **comrak 0.54** is the only maintained Rust parser covering the full
  Obsidian surface natively — wikilinks, callouts/alerts, `$`/`$$` math,
  front-matter, emoji shortcodes, footnotes — at 652/652 CommonMark and
  670/670 GFM, with per-node source positions (`data-sourcepos`) and
  per-language code-fence renderer plugins.
- **pulldown-cmark 0.13** is faster but covers less of the extension
  surface; **markdown-it-rs is dead**; **rushdown** (goldmark author) is
  promising but young. A multi-engine abstraction over an event stream with
  byte spans is the common denominator all of these can satisfy.
- **WebKitGTK 6** ships in org.gnome.Platform (zero flatpak size cost),
  renders **MathML Core natively** — so math can be pre-rendered
  LaTeX→MathML in Rust (`pulldown-latex`) with zero JS — and gives
  print-to-PDF and best-in-class text layout for free. Cost: ~200–300 MB
  RSS floor per web process.
- The native-widget alternative (Fractal/Manuscript/html2gtk pattern) is
  real and where the ecosystem is heading (Newsflash deleted WebKit in
  2026), but native mermaid and math renderers are not yet at Obsidian
  fidelity.

## User Impact

| Audience | Impact |
|---|---|
| End users | `xdg-open README.md` gives an instant, beautiful, Obsidian-faithful render; no hangs with many windows. |
| Contributors | Clear crate boundaries; golden-test-driven rendering work; standard GNOME stack. |
| Operators / packagers | Flatpak on org.gnome.Platform, no network permission, no bundled pandoc (~150 MB saved vs Apostrophe). |

## Considered Options

### Option A — WebKitGTK 6 webview, maximal Rust-side pre-rendering (v1)

Parse and render to HTML in Rust: comrak AST → HTML with `data-sourcepos`
anchors, math pre-rendered to MathML, code highlighted via syntect CSS
classes, sanitized with ammonia. The webview displays static local HTML;
JS is limited to a mermaid renderer and a thin bridge (scroll sync,
link/checkbox interception). Updates patch the DOM; no `load_html` reloads
after first paint.

**Pros**: Obsidian-grade tables/math/typography in weeks; CSS theming;
print-to-PDF later for free; WebKitGTK ships in the GNOME runtime; sandbox
on by default.

**Cons**: ~200–300 MB RSS floor per window's web process; selection/find UI
must be styled to look Adwaita; inherits WebKit behavior.

### Option B — Native widget tree (Fractal/Manuscript/html2gtk pattern)

One GTK widget per block: Pango labels, grids for tables, sourceview5 code
blocks, resvg-rendered SVG for diagrams, ReX/katex-rs for math.

**Pros**: tiny memory footprint, perfect Adwaita integration, no web stack.

**Cons**: months to reach table/math/mermaid fidelity; cross-block text
selection is an open problem; native math/mermaid crates not yet at
Obsidian fidelity.

### Option C — Keep an external converter (pandoc et al.)

Rejected outright: it is the architecture that made Apostrophe unusable,
and `design_decisions.md` forbids it.

## Decision

**Chosen option: Option A for v1, with Option B as the stated long-term
direction** — pending human ratification of **D1** below. The engine
boundary and the Rust-side pre-rendering pipeline are renderer-agnostic by
construction: everything up to and including sanitized HTML with source
anchors survives a later migration to native widgets block-by-block.

Rationale: rendering perfection is the product's critical goal and Option A
reaches it fastest at zero packaging cost; the main cost (web-process
memory) does not threaten the responsiveness budgets, and the pipeline is
built so the expensive part is reusable when native rendering matures.

## Design

### Crate layout (cargo workspace)

```
axiomd/
├── crates/
│   ├── axiomd-engine/     # engine boundary + comrak engine
│   ├── axiomd-render/     # AST/events → sanitized HTML with anchors
│   └── axiomd-app/        # gtk4 + libadwaita + webkit6 application
├── data/                  # desktop file, icons, gschema, blueprints, CSS
└── build-aux/flatpak/     # manifest
```

### Engine boundary (`axiomd-engine`)

The boundary contract is an **event stream with byte spans** — the common
denominator of comrak (AST walk + sourcepos), pulldown-cmark
(`into_offset_iter`), and jotdown (`OffsetIter`):

```rust
pub trait MarkdownEngine: Send + Sync {
    fn id(&self) -> EngineId;
    fn capabilities(&self) -> Capabilities;   // flavor + extension set
    fn parse<'a>(&self, source: &'a str, opts: &ParseOptions)
        -> Box<dyn DocumentEvents<'a> + 'a>;  // events, each with Span
}

pub struct Span { pub range: core::ops::Range<usize>, pub line: u32 }
```

Events are axiomd's own types (block open/close, inline, code fence with
language, math, callout kind, wikilink target, footnote ref/def, table
cells with alignment). No comrak type appears in any public signature —
enforced by the crate's public API surface and reviewed at the boundary.

### Rendering pipeline (`axiomd-render`)

`events → HTML string + anchor map`, all in Rust, all off the main thread:

1. Structural HTML with `data-line` anchors on every block (from spans).
2. Math events → MathML Core via `pulldown-latex`; bundled math font.
3. Code fences → syntect with class-based output; CSS supplies both the
   light and dark palettes (two-face syntax set).
4. Callouts → semantic `<div class="callout callout-note">` with bundled
   Lucide icons; the full Obsidian set (fold markers, custom titles) via
   blockquote post-processing.
5. Wikilinks resolved relative to the document (target existence checked);
   task-list checkboxes rendered disabled in MVP.
6. `ammonia` sanitization as defense-in-depth (JS already disabled for
   document content; strict CSP meta tag; `img-src` limited to the custom
   local scheme).

The pipeline is pure (no I/O, no GTK): golden-testable byte-for-byte.

### App shell (`axiomd-app`)

- Plain gtk4-rs 0.11 + libadwaita 0.9 with composite templates
  (Blueprint). Feature-pinned to the local platform: `v4_20` / adw `v1_8`
  (GNOME 49 baseline; bump deliberately).
- `Adw.ApplicationWindow` per document; `Adw.ToolbarView` skeleton;
  `Gtk.FileDialog` for opening; `HANDLES_OPEN` for file-manager launches;
  desktop file registers `text/markdown`.
- One `webkit6::WebView` per window, JS disabled for document content,
  custom `axiomd://` URI scheme serving the rendered HTML and resolving
  relative images from the document's directory — never a `file://` grant.
- Bridge: `UserContentManager` script message handlers for scroll position
  and link activation; scroll sync and outline tracking map through the
  anchor map (`data-line`), never proportional height.
- Live reload: `Gio.FileMonitor` → 150 ms debounce → cancel in-flight
  parse → re-render → DOM patch preserving the nearest anchor.
- Background work: a per-window render task on a worker thread
  (cancellation token per generation); results marshalled to the main
  thread via `glib::spawn_future_local` channels.

### Incremental strategy (post-first-paint)

CommonMark block structure is line-based: split top-level blocks, hash
source per block, re-parse only dirty blocks, cache rendered HTML keyed by
span; link-reference/footnote definitions force global invalidation. MVP
ships whole-document background re-parse with DOM patching (adequate for a
viewer whose input changes on file save); block-level caching lands behind
the same interface when the 10 MB budget test demands it.

## Testing Strategy

| Risk / invariant | Test layer | Test name / location |
|---|---|---|
| CommonMark/GFM conformance | engine golden tests | `axiomd-engine/tests/spec_commonmark.rs`, `spec_gfm.rs` (official spec.txt cases) |
| Extension fidelity (math, callouts, wikilinks, footnotes) | render golden tests | `axiomd-render/tests/golden/*.md` → `.html` fixtures |
| Sanitization (raw HTML, script injection) | render tests | `axiomd-render/tests/sanitize.rs` |
| Span correctness (outline/scroll/search all depend on it) | engine property tests | every block event's span round-trips to its source slice |
| No implicit network | app integration | CSP + URI-scheme test: remote ref renders placeholder, zero requests |
| Live reload preserves position | app integration | monitor event → anchor unchanged |
| Budgets (startup, 10 MB scroll, N windows) | perf harness | `tests/perf/` with generated fixtures, asserted numbers |

Not automatable: subjective beauty of the default stylesheet; verified by
side-by-side review against Obsidian's reading view on a fixture corpus.

## Goals Alignment

| Goal | How addressed |
|---|---|
| G1 | comrak engine + golden/spec suites; pre-rendered MathML; mermaid in-view |
| G2 | in-process parse, DOM patching, budgets as tests |
| G3 | event+span boundary trait; comrak types sealed inside `axiomd-engine` |
| G4 | gtk4-rs/libadwaita shell, live theming, flatpak with no network |

## MVP Scope

In scope (mirrors `designs/MVP-USER-TASKS.md` UT-001…UT-011): open from
file manager/CLI/dialog, Obsidian-grade render (GFM + math + callouts +
wikilinks + footnotes + mermaid), live reload with position preservation,
outline sidebar, in-document search, link handling (relative .md in-app,
anchors, external to browser on click), local images, live theming,
zoom, multi-window, perf budgets, flatpak.

Out of scope for MVP: editing of any kind, export/print, recents UI,
tabs, checkbox toggling, footnote hover-popovers, a second engine,
native-widget renderer, settings dialog beyond theme override.

## Development Plan

Mirrors the GitHub issues; order is the ktask queue order.

- [ ] **Step 1** — Workspace bootstrap: crates, quality gate, hello
  Adw window *(prerequisite: -)*
- [ ] **Step 2** — Engine boundary + comrak engine + spec conformance
  *(Step 1)*
- [ ] **Step 3** — Render pipeline: HTML + anchors + syntect + ammonia +
  default stylesheet *(Step 2)*
- [ ] **Step 4** — App shell: window, open paths, webview, custom scheme,
  MIME registration *(Step 3)*
- [ ] **Step 5** — Live reload with anchor preservation *(Step 4)*
- [ ] **Step 6** — Links and images: relative nav, anchors, external,
  placeholder policy *(Step 4)*
- [ ] **Step 7** — Outline sidebar + position tracking *(Step 4)*
- [ ] **Step 8** — In-document search *(Step 4)*
- [ ] **Step 9** — Perf harness + budget tests *(Step 5)*
- [ ] **Step 10** — Live theming + zoom *(Step 4)*
- [ ] **Step 11** — Math: LaTeX→MathML + bundled font *(Step 3)*
- [ ] **Step 12** — Obsidian extensions: callouts, wikilinks, footnotes,
  task lists *(Step 3)*
- [ ] **Step 13** — Mermaid (bundled, lazy, offline) *(Step 4)*
- [ ] **Step 14** — Flatpak packaging *(Step 4)*

---

## Decisions to Ratify

- [ ] **D1** — v1 renderer is a WebKitGTK 6 webview with Rust-side
  pre-rendering; native widget tree is the long-term direction.
  *(proposed: Option A now, migrate block-by-block later; alternative:
  native-first à la Manuscript, accepting slower path to math/mermaid
  fidelity and ~months more work, in exchange for a small memory
  footprint)*
- [ ] **D2** — Primary engine is comrak. *(proposed: comrak for its full
  Obsidian extension surface + sourcepos; alternative: pulldown-cmark for
  throughput, hand-rolling callouts/emoji/header-IDs)*
- [ ] **D3** — Obsidian flavor is the default rendering profile (math,
  callouts, wikilinks on by default). *(proposed: yes — matches the
  product promise; alternative: strict GFM default with a per-file or
  per-app flavor switch)*
- [ ] **D4** — Remote images render as placeholders with per-document
  opt-in. *(proposed: placeholder + explicit action; alternative: global
  setting, or always-block)*

## Open Questions

- [ ] **Q1** — Meson+cargo hybrid (GNOME Builder convention) vs pure cargo
  with a small install script; decide when flatpak packaging lands
  (Step 14).
- [ ] **Q2** — When the block-cache lands (Step 9's budgets decide), does
  the DOM patch move to morphdom-style diffing or anchor-keyed block
  replacement?

## References

- Apostrophe review: this conversation's findings; clone at `../apostrophe`
- comrak — https://github.com/kivikakk/comrak
- pulldown-latex — https://github.com/carloskiki/pulldown-latex
- webkit6 bindings — https://gitlab.gnome.org/World/Rust/webkit6-rs
- Native prior art: Manuscript — https://gitlab.com/ilshat-apps/manuscript,
  html2gtk — https://gitlab.com/news-flash/html2gtk,
  Fractal — https://gitlab.gnome.org/World/fractal
- Lezer incremental markdown (architecture blueprint) —
  https://github.com/lezer-parser/markdown
- CommonMark spec — https://spec.commonmark.org/ · GFM spec —
  https://github.github.com/gfm/
