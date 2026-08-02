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
  themed live, flatpak-packaged with zero implicit network (test-enforced;
  the only network use is an explicit one-click image load).
- **G5** — Complete daily-work loop: edit with instant typing latency and
  incremental preview, print, and export to PDF/HTML — built on the same
  pipeline, from day one.
- **G6** — Extensible by design: a plugin layer for optional rendering
  capabilities (diagrams, math, …) and selectable engines behind the
  boundary; deep modules with simple APIs throughout.

## Non-Goals

- **NG1** — Vault/notebook management, indexing, or sync.
- **NG2** — Universal conversion (pandoc's 19 formats). Export means
  print-quality PDF and standalone HTML.
- **NG3** — A native (non-webview) renderer. Out of scope by owner
  decision (2026-08-02); scalability effort goes into the webview path.
- **NG4** — Rich authoring chrome (formatting toolbars, table editors,
  WYSIWYG). MVP editing is a first-class source editor with live preview;
  authoring conveniences come later on the day-one document model.

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
| Operators / packagers | Flatpak on org.gnome.Platform, zero implicit network, no bundled pandoc (~150 MB saved vs Apostrophe). |

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

**Chosen option: Option A — ratified by the owner 2026-08-02 (D1), with
the amendment that a native renderer is OUT of scope entirely (NG3).**
Scalability and responsiveness inside the webview are first-class
requirements: even a large, complex document must never hang the app —
incremental DOM patching, lazy/virtualized rendering of heavy content, and
cancellable background work are the mechanisms, and the perf budgets are
the enforcement.

Rationale: rendering perfection is the product's critical goal and Option A
reaches it fastest at zero packaging cost; the main cost (web-process
memory) does not threaten the responsiveness budgets.

## Design

### Crate layout (cargo workspace)

```
axiomd/
├── crates/
│   ├── axiomd-engine/     # engine boundary + engines (comrak first)
│   ├── axiomd-render/     # events → sanitized HTML with anchors + plugin layer
│   ├── axiomd-doc/        # editable document model (buffer = source of truth)
│   └── axiomd-app/        # gtk4 + libadwaita + webkit6 application
├── data/                  # desktop file, icons, gschema, blueprints, CSS
└── build-aux/flatpak/     # manifest
```

Module design rule (owner mandate, 2026-08-02): **deep modules with simple
APIs**. Each crate hides significant machinery behind a small surface;
shallow pass-through modules and wide interfaces are design defects that
reviews reject. Interfaces only get narrower over time.

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

Selectable engines are a MUST (owner ruling on D2, 2026-08-02): the
boundary is proven by at least two real engines (comrak first,
pulldown-cmark second) plus an engine registry the app selects from at
runtime. The default engine is an open question settled by measurement —
conformance suites and the perf harness run against every registered
engine, and the comparison report goes to the owner for the ruling.

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

### Plugin layer (`axiomd-render`)

Optional rendering capabilities are plugins with a small, versioned API —
owner mandate (2026-08-02). A plugin can register:

- **fence handlers** — claim a code-fence language (```mermaid, ```plantuml)
  and produce HTML or defer to in-view rendering;
- **event transforms** — rewrite typed event streams (callout detection,
  wikilink resolution live here for flavor profiles);
- **post-render hooks** — decorate the HTML/anchor map;
- **assets** — CSS/JS/fonts served via `axiomd://`, injected only when the
  plugin is active AND the document needs it.

Plugins are independently toggleable at runtime, off-cost-free when unused,
and sandboxed by the same CSP/no-network rules as everything else. Core
rendering — CommonMark/GFM including tables and images — is never a
plugin. Math and mermaid ship as the first two built-in plugins, proving
the API from both the pure-Rust side (MathML) and the asset-injection side
(mermaid). Failure of a plugin degrades to the source block with an inline
badge — never an error dialog, never a broken document.

### Document model (`axiomd-doc`)

Editing is built in from day one (owner decision, 2026-08-02): while a
window owns a file, the **buffer is the source of truth** — rendering,
outline, search, and export all consume the buffer, not the file. The
model provides: load/save/save-as with atomic writes, dirty state,
external-change reconciliation (the file monitor feeds the model; a clean
buffer follows the file silently with the reading position preserved, a
modified buffer surfaces an inline banner — never a blocking dialog on the
view path), **optional autosave** (debounced after idle plus on focus
loss; atomic writes; self-triggered monitor events ignored so autosave
never loops the reload path), and change notifications that drive the same
debounced incremental render path as live reload. Undo/redo rides
GtkSourceView's buffer in the app layer. Modal dialogs are reserved for
explicit user-initiated actions (Save As, preferences,
close-with-unsaved) per `ux_decisions.md`.

### Editor and modes (`axiomd-app`)

GtkSourceView 5 editor pane with the Markdown language definition, Adwaita
scheme, and optional edit-mode-only spellcheck (libspelling, preferences
toggle). Two window modes (owner ruling 2026-08-02): **read** (default
when opening a file) and **edit**, toggled with Ctrl+E. No split view in
MVP — but the view container and document model must not assume a single
visible surface, so split + scroll sync can be added later without
rework. Mode switches preserve position in both directions through the
span/anchor map (read→edit: caret at the source line of the topmost
visible anchor; edit→read: scroll to the anchor nearest the caret).
Bare launch and Ctrl+N open a new untitled document in edit mode.
Typing latency is independent of render cost: keystrokes hit the buffer
synchronously; parse+render runs debounced (150 ms) on the worker with
cancellation, keeping the read view current so switching back is instant.

### Preferences (`axiomd-app`)

Owner ruling (2026-08-02): all features are configurable in preferences.
A gschema-backed settings module (deep, typed, no scattered key strings)
plus `AdwPreferencesDialog`: theme override, reading width (on/off +
width), autosave (on/off, default ON + delay), spellcheck toggle, plugin
toggles, default engine (per-document override lives in the view menu).
Every setting applies live; every feature task lands its own preferences
row as an exit criterion.

### Print and export (`axiomd-app` + `axiomd-render`)

- **Print**: WebKit's print operation over the rendered document, with a
  dedicated print stylesheet (page margins, break rules for headings/code
  blocks/tables, no app chrome). GTK print dialog; print preview is the
  document itself.
- **Export to PDF**: the same print machinery driven headlessly to a file
  destination — one code path for print and PDF ensures they never drift.
- **Export to HTML**: the render pipeline emits a standalone document
  (inlined styles, embedded images as data URIs or a sibling assets
  folder — user choice in the save dialog, not a modal).
- No converter subprocesses, per `design_decisions.md`.

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
| User flows (UT-001…011) | e2e harness (headless app, DOM assertions) | `axiomd-e2e` suite |
| Visual regressions | screenshot goldens (human-pinned once, diffed forever) | `tests/goldens/` |
| Typing latency vs preview cost | perf harness (keystroke echo budget while a heavy doc renders) | `tests/perf/` |
| Edit → preview correctness | e2e (buffer change → patched DOM matches full re-render) | `axiomd-e2e` |
| Data loss on save | doc-model tests (atomic write, external-change reconciliation) | `axiomd-doc` tests |
| Print/PDF fidelity | export golden (PDF text+structure extraction vs fixture; page-break rules) | export tests |
| Standalone HTML export | golden (self-contained, renders offline) | export tests |
| Plugin isolation | render tests (plugin off = zero cost/assets; plugin failure = inline badge, doc intact) | plugin tests |
| Engine parity | conformance+perf matrix across registered engines | engine comparison harness |

Testability is a MUST (docs/TESTING.md): no automated test → not done;
untestable behavior escalates to the human. Subjective beauty of the default
stylesheet is handled by the screenshot-golden model — one-time human
side-by-side approval against Obsidian's reading view pins the goldens;
thereafter it is a regression test like any other.

## Goals Alignment

| Goal | How addressed |
|---|---|
| G1 | comrak engine + golden/spec suites; pre-rendered MathML; mermaid in-view |
| G2 | in-process parse, DOM patching, budgets as tests |
| G3 | event+span boundary trait; comrak types sealed inside `axiomd-engine` |
| G4 | gtk4-rs/libadwaita shell, live theming, flatpak with no network |

## MVP Scope

In scope (mirrors `designs/MVP-USER-TASKS.md` plus the 2026-08-02 owner
rulings): open from file manager/CLI/dialog; best-in-class core render
(CommonMark/GFM with first-class tables and images); **editing** (read/
edit modes with position-preserving switch, new-untitled on bare launch
and Ctrl+N, atomic save, configurable autosave default-ON, optional
edit-mode spellcheck, instant typing latency); **interactive task-list
checkboxes** (click in read mode updates the source); **print** and
**export to PDF/HTML**; **preferences dialog** (every feature knob,
applied live); plugin layer with mermaid and math as the first built-in
optional plugins; two engines with app-default + per-document selection
and a measured comparison; live reload with position preservation;
outline sidebar; search in both modes; link handling; remote images as
one-click-load placeholders; live theming; zoom; configurable reading
width; multi-window; perf budgets; a local flatpak build (flathub polish
explicitly deprioritized).

Out of scope for MVP: recents UI, tabs (windows only — owner ruling),
split view with scroll sync (door kept open architecturally), frontmatter
rendering (parsed as metadata, hidden), footnote hover-popovers,
formatting toolbars/WYSIWYG, vim-style keybindings, native-widget
renderer (out of scope entirely, not just for MVP), third-party plugin
loading (the API ships, built-ins prove it; external distribution is
post-MVP).

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
- [ ] **Step 4b** — e2e harness: headless app driving, DOM assertions,
  screenshot goldens with human-only pinning *(Step 4)*
- [ ] **Step 5** — Live reload with anchor preservation *(Steps 4, 4b)*
- [ ] **Step 6** — Links and images: relative nav, anchors, external,
  placeholder policy *(Step 4)*
- [ ] **Step 7** — Outline sidebar + position tracking *(Step 4)*
- [ ] **Step 8** — In-document search *(Step 4)*
- [ ] **Step 9** — Perf harness + budget tests *(Step 5)*
- [ ] **Step 10** — Live theming + zoom *(Step 4)*
- [ ] **Step 11** — Math: LaTeX→MathML + bundled font *(Step 3)*
- [ ] **Step 12** — Obsidian extensions: callouts, wikilinks, footnotes,
  task lists *(Step 3)*
- [ ] **Step 13** — Mermaid as the first asset-injection plugin *(Steps 4,
  16)*
- [ ] **Step 14** — Flatpak packaging, local build only *(Step 4)*
- [ ] **Step 16** — Plugin layer: fence handlers, event transforms,
  post-render hooks, conditional assets, runtime toggles *(Step 3)*
- [ ] **Step 17** — Second engine (pulldown-cmark) + engine registry/
  selection + measured comparison report for the default ruling *(Step 2)*
- [ ] **Step 18** — Document model + editor: buffer as source of truth,
  read/split/source modes, save, bidirectional scroll sync, typing-latency
  budget *(Steps 4b, 5)*
- [ ] **Step 19** — Print + export to PDF/HTML *(Step 4)*
- [ ] **Step 20** — Preferences dialog + settings infrastructure; later
  feature tasks land their own rows *(Step 4)*

---

## Decisions to Ratify

- [x] **D1** — RATIFIED by owner 2026-08-02: renderer is a WebKitGTK 6
  webview with Rust-side pre-rendering. Amendment: native renderer is OUT
  of scope entirely; scalability/responsiveness inside the webview is a
  hard requirement — a large, complex document must never hang the app.
- [x] **D2** — RULED by owner 2026-08-02: no engine is anointed by
  assumption. Selectable engines behind the abstraction are a MUST; at
  least comrak and pulldown-cmark ship behind the boundary, the candidates
  are tested (conformance + perf comparison), and the DEFAULT remains an
  open decision (**D5**) taken on that evidence.
- [x] **D3** — RULED by owner 2026-08-02: Obsidian is a fidelity
  benchmark, not the profile definition. Tables and images are first-class
  core priorities; diagrams matter more than math to the owner; neither is
  a blocker. Capabilities beyond core (math, UML/diagrams, …) are OPTIONAL
  plugins on a well-architected plugin API.
- [x] **D4** — RATIFIED by owner 2026-08-02 with refinement: always render
  best-effort; NO modal questions ever (Apostrophe's dialog is the named
  anti-pattern). Remote images: placeholder by default, and the
  placeholder is a one-click load button (plus an inline per-document
  "load all").
- [ ] **D5** — Default engine, ruled AFTER Step 17's measured comparison
  report. *(candidates: comrak, pulldown-cmark; criteria: conformance
  pass rates, extension coverage, parse throughput, span quality)*

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
