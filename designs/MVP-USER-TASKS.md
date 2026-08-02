# MVP User Tasks

This file captures the user workflows the MVP must support. Treat it as a
test planning document, not marketing copy. Interactions counts are budgets:
if an implementation needs more actions than listed, the design regressed.

## UT-001: Open a Markdown file from the file manager

**Precondition:** axiomd is installed and registered as a handler for
`text/markdown`.

**Flow:**

1. User double-clicks `README.md` in Files (or runs `xdg-open README.md`).

**Outcome:** A window appears with the document fully rendered, scrolled to
the top, in the system light/dark theme, in under 300 ms on a typical file.

**Interactions:** 1

**Regression coverage:** e2e smoke: launch with file argument, assert
rendered content; startup-latency budget test.

## UT-002: Open a file from within the app

**Precondition:** An axiomd window is open.

**Flow:**

1. User presses `Ctrl+O` (or clicks Open).
2. User picks a `.md` file in the portal file chooser.

**Outcome:** The document renders in the current window; the recents list
records it.

**Interactions:** 2

**Regression coverage:** integration test on the open action; manual portal
check.

## UT-003: Read a large document smoothly

**Precondition:** A 10 MB Markdown file exists.

**Flow:**

1. User opens the file.
2. User scrolls continuously to the bottom.

**Outcome:** Scrolling stays smooth (no multi-frame stalls); memory stays
bounded; the UI never blocks.

**Interactions:** 2

**Regression coverage:** perf harness test with a generated 10 MB fixture,
asserting frame budget and peak memory.

## UT-004: Live-reload a file being edited elsewhere

**Precondition:** A document is open in axiomd and in a text editor.

**Flow:**

1. User saves the file in the editor.

**Outcome:** axiomd re-renders the changed document within the debounce
window, preserving the scroll position; no flash, no full-page reload
artifact.

**Interactions:** 0 (in axiomd)

**Regression coverage:** integration test: file monitor event → incremental
re-render → scroll anchor preserved.

## UT-005: Navigate via the outline

**Precondition:** A document with headings is open.

**Flow:**

1. User opens the outline sidebar (`F9` or button).
2. User clicks a heading.

**Outcome:** The view scrolls to that heading; the outline tracks the
current position while scrolling.

**Interactions:** 2

**Regression coverage:** integration test mapping outline entry → scroll
target via source spans.

## UT-006: Search inside the document

**Precondition:** A document is open.

**Flow:**

1. User presses `Ctrl+F`.
2. User types a query.
3. User presses Enter / `Ctrl+G` to step through matches.

**Outcome:** Matches are highlighted in the rendered view; navigation cycles
through them with a match counter.

**Interactions:** 3

**Regression coverage:** integration test on match highlighting and
next/previous ordering.

## UT-007: Follow links

**Precondition:** A document with a relative link to another local `.md`
file, an anchor link, and an external URL is open.

**Flow:**

1. User clicks each link.

**Outcome:** Relative `.md` link opens in axiomd (same window, with back
navigation); anchor link scrolls within the document; external URL opens in
the default browser after an explicit activation (never automatically).

**Interactions:** 1 per link

**Regression coverage:** integration tests per link class; security test
that no remote fetch happens on render.

## UT-008: Switch theme

**Precondition:** A document is open; system theme is light.

**Flow:**

1. User switches the system (or in-app) style to dark.

**Outcome:** Rendered document restyles to the dark palette without a reload
flash; code blocks switch highlight theme.

**Interactions:** 1

**Regression coverage:** integration test: style-manager change → CSS swap
without document re-parse.

## UT-009: Read Obsidian-flavored notes

**Precondition:** A note using GFM tables, task lists, footnotes, `$...$`
math, callouts (`> [!note]`), and a mermaid fence is open.

**Flow:**

1. User opens the note.

**Outcome:** Tables, task lists, footnotes render per GFM; math renders
typeset; callouts render as styled admonitions; mermaid renders as a
diagram (or a clearly-labeled fallback if the engine is disabled).

**Interactions:** 1

**Regression coverage:** golden-file rendering tests per extension.

## UT-010: Work with many windows

**Precondition:** Ten documents are open in ten windows.

**Flow:**

1. User switches between windows and scrolls in each.

**Outcome:** Every window stays responsive; closing a window frees its
resources; no shared-state leaks between windows.

**Interactions:** n/a (load scenario)

**Regression coverage:** perf harness: N-window scenario with per-window
memory and responsiveness assertions.

## UT-011: Zoom

**Precondition:** A document is open.

**Flow:**

1. User presses `Ctrl+plus` / `Ctrl+minus` / `Ctrl+0` (or pinches).

**Outcome:** Rendered text scales in steps, persisted per window session;
layout reflows; `Ctrl+0` restores 100 %.

**Interactions:** 1

**Regression coverage:** integration test on zoom action and persistence.

## UT-012: Edit a document with live preview

**Precondition:** A document is open in read mode.

**Flow:**

1. User presses `Ctrl+E` (or the mode switcher) to enter split mode.
2. User types in the editor pane.
3. User presses `Ctrl+S`.

**Outcome:** Keystrokes echo instantly regardless of document size; the
preview patches incrementally within the debounce window; the saved file
matches the buffer exactly (atomic write); scroll stays synced between
panes in both directions.

**Interactions:** 3

**Regression coverage:** e2e: edit → preview DOM equals full re-render;
doc-model atomic-save and external-change-matrix tests; perf harness
keystroke-echo budget.

## UT-013: Print a document

**Precondition:** A rendered document is open.

**Flow:**

1. User presses `Ctrl+P`.
2. User confirms in the print dialog.

**Outcome:** Printed pages reproduce the rendered document with sane page
breaks (no orphan headings), no app chrome, light palette.

**Interactions:** 2

**Regression coverage:** print/PDF single-path test + PDF structure
extraction on the crafted break-rule fixture.

## UT-014: Export to PDF or HTML

**Precondition:** A rendered document (with active plugin content) is open.

**Flow:**

1. User presses `Ctrl+Shift+E`.
2. User picks format and destination in the file dialog.

**Outcome:** PDF matches print output byte-for-byte (same code path); HTML
is fully self-contained and renders offline in a browser, including images
and active-plugin output.

**Interactions:** 2

**Regression coverage:** export goldens; zero-external-refs check on HTML;
pdftotext content-order assertions.
