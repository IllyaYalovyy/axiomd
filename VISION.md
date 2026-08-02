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

axiomd is a native Markdown application for modern GNOME, written in Rust.
It opens `.md` files instantly, renders them with best-in-class fidelity,
and stays responsive no matter how many windows are open or how large the
file is. Reading is the soul of the app — but editing, print, and export
(PDF/HTML) are built in from day one: a Markdown app without them is
pointless for daily work. Rendering capabilities (diagrams, math, and
whatever comes next) are optional plugins on a well-architected extension
layer.

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
   boundary: document in, typed events with source spans out. Multiple
   engines are selectable — the default is chosen by measured conformance
   and performance, not by assumption — and no engine-specific type leaks
   past the boundary. On top of it, a plugin layer adds rendering
   capabilities (diagrams, math, UML, …) as optional, independently
   toggleable modules. Tables and images are core, never plugins.
4. **Native modern GNOME.** GTK4 + libadwaita, HIG-compliant, adaptive,
   dark/light/high-contrast aware. It should look like it shipped with the
   desktop.
5. **Reading first, editing built in.** The reading experience leads UX
   decisions, but the document model is editable from day one — an
   afterthought editor is unfixable later. Print and export are core
   workflows, not extras.
6. **Best effort, never a modal.** Every document renders as well as it
   can, immediately. The app never interrupts with blocking questions
   (Apostrophe's security dialog is the anti-pattern). Degraded content
   gets an inline, one-click affordance instead.
7. **Local-first, zero implicit network.** All assets (fonts, styles,
   highlighters, diagram renderers) are bundled. A document never causes a
   network fetch without an explicit one-click user action.
8. **TDD.** Behavior lands with tests demonstrated red first. The
   rendering pipeline is golden-tested against spec fixtures; regressions
   are caught by the gate, not by users.
9. **Deep modules, simple APIs.** Every module hides significant
   functionality behind a small interface. Shallow modules with wide
   interfaces are design defects. Each change leaves interfaces no wider
   than it found them; code quality and clarity improve continuously.

## What axiomd is NOT

- Not a note manager — no vault, no database, no sync. Files are files.
- Not a universal converter — export means print-quality PDF and HTML,
  not pandoc's 19 formats.
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
- Editing a document keeps typing latency instant while the preview
  updates incrementally; print and PDF/HTML export reproduce the rendered
  document faithfully.
