# axiomd — Vision

## Problem

Reading Markdown on Linux is either ugly, slow, or both. Apostrophe — the
nicest-looking GNOME option — forks a `pandoc` subprocess and reloads a full
WebKit page on effectively every keystroke, runs whole-document regex passes
in forked Python processes per window, and hangs under real daily load with
several windows open. Editors like Obsidian render Markdown beautifully but
are Electron apps with a vault model — not a lightweight system viewer you
can point at any file. GNOME deserves a Markdown viewer that opens instantly,
renders perfectly, and stays fast with many windows and huge documents.

## Solution

axiomd is a native Markdown **viewer** for modern GNOME, written in Rust.
It opens `.md` files instantly, renders them at Obsidian-grade fidelity, and
stays responsive no matter how many windows are open or how large the file
is. It is a reader first: the fastest, most faithful way to *look at*
Markdown on a GNOME desktop.

## Core principles

1. **Rendering correctness is the product.** Output is pinned to the
   CommonMark and GFM specs by conformance tests, not by eyeballing.
   Extensions (math, diagrams, callouts) follow the de-facto standards set
   by the tools people actually use (Obsidian, GitHub). A rendering
   difference from the spec is a bug, never a quirk.
2. **Never block, never fork.** Parsing is in-process and incremental;
   long work happens off the main thread and is cancellable. There is no
   per-keystroke or per-file subprocess, ever. Performance budgets (startup
   time, reload latency, memory per window) are stated numbers enforced by
   tests, not aspirations.
3. **A pluggable rendering pipeline.** The parser sits behind an engine
   boundary: document in, typed AST with source spans out. Engines and
   flavors can be added or swapped without touching the view layer. No
   engine-specific type leaks past the boundary.
4. **Native modern GNOME.** GTK4 + libadwaita, HIG-compliant, adaptive,
   dark/light/high-contrast aware. It should look like it shipped with the
   desktop.
5. **Viewer first.** axiomd is not an editor. Reading-centric affordances
   (navigation, outline, search, zoom, printing) always win over authoring
   features.
6. **Local-first, zero network.** All assets (math fonts, styles,
   highlighters) are bundled. The flatpak requests no network access. A
   document never causes a network fetch without an explicit user action.
7. **TDD.** Behavior lands with tests demonstrated red first. The
   rendering pipeline is golden-tested against spec fixtures; regressions
   are caught by the gate, not by users.

## What axiomd is NOT

- Not an editor (no editing surface in v1; "open in editor" delegates out).
- Not a note manager — no vault, no database, no sync. Files are files.
- Not an export studio — pandoc-style multi-format export is out of scope
  for MVP.
- Not a browser — remote content is never loaded implicitly.

## Target users

- Developers and writers who read READMEs, docs, and notes locally.
- GNOME users who want `xdg-open README.md` to produce something beautiful
  and instant.
- Obsidian users who want their notes to render identically outside the app.

## Success criteria

- Cold start to rendered document: fast enough to feel instant (<300 ms on
  a typical file).
- A 10 MB Markdown file scrolls smoothly and re-renders incrementally.
- Ten windows open: no cross-window slowdown, bounded memory per window.
- CommonMark spec conformance suite passes; GFM extension suites pass.
- An Obsidian user opens their notes and sees the same document.
