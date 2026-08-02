# Design decisions

Architecture decision record in FAQ form. Every entry here is an
**intentional decision — not a limitation, a gap, or a bug to be fixed.**
Do not "improve" or "fix" any of these without an explicit human decision
to change direction. Agents: read this before proposing architecture.

## Why is there no in-process pandoc / external converter?

Apostrophe's defining performance failure was forking a `pandoc` subprocess
per change and reloading a full WebKit page with the result. axiomd parses
in-process with a Rust parser behind the engine boundary. **No rendering
path may ever shell out to an external converter.** This is set in stone.
If a format seems to need one, that format is out of scope until decided
otherwise by a human.

## Is axiomd an editor?

No. Viewer first (VISION principle 5). No editing surface ships in v1.
Feature requests that presuppose editing are recorded, not implemented.

## Why an engine boundary instead of committing to one parser?

Rendering compatibility differs by tool ecosystem (CommonMark vs GFM vs
Obsidian flavor). The view layer consumes a typed AST with source spans;
which engine produced it is an implementation detail behind a trait. This
keeps "perfect rendering" achievable per-flavor without rewrites and allows
multiple engines to coexist. No engine type leaks past the boundary.

## Why does the flatpak have no network permission?

Bundled assets only (VISION principle 6). Apostrophe needed
`--share=network` because pandoc emitted a MathJax CDN script tag. axiomd
bundles its math and highlighting assets. A document must never trigger a
network fetch on render; external links open in the browser only on
explicit user activation. Loosening the sandbox is a human-only decision.

## How is scroll sync / outline tracking mapped?

By source spans (line ↔ rendered block anchors), never by proportional
scroll height. Proportional sync is Apostrophe's approach and it
desynchronizes on any tall block (code, image, table). Every engine must
provide source positions; a renderer must preserve them as anchors.

## What are the performance budgets?

Stated numbers, enforced by tests in the perf harness — not aspirations:
cold start to rendered typical file < 300 ms; re-render after file change
incremental (never a full-document reload); a 10 MB document scrolls
without multi-frame stalls; per-window memory bounded and freed on close.
A change that violates a budget test is not done, whatever else it fixes.
