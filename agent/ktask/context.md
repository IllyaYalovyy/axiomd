# Shared context injected into every task prompt

## Cross-cutting invariants — check EVERY one your change could touch

These are the interactions most likely to produce review-caught defects.
A scoped patch is right; scoped VERIFICATION is not. Before finishing, walk
this list and verify each row your change intersects. When a shipped bug
creates a new invariant, add it here with the issue number.

1. **No external converter, ever.** No rendering path may shell out to a
   subprocess (pandoc or anything else). In-process, incremental, and
   cancellable is the only accepted shape (design_decisions.md).
2. **The engine boundary is sealed.** The view layer consumes the typed
   AST + source spans only. No engine-specific type, feature flag, or HTML
   string format may leak past the boundary trait.
3. **Source spans are load-bearing.** Outline navigation, scroll sync,
   live-reload anchor preservation, and search highlighting all map through
   source positions. Any parser or renderer change must keep spans correct —
   a span regression breaks four features at once.
4. **The GTK main thread never blocks.** Parsing and rendering of
   non-trivial documents happen off the main loop and are cancellable; UI
   state is touched only from the main thread. No synchronous I/O in
   signal handlers.
5. **Live-reload preserves the reading position.** Any change to parsing,
   rendering, or file monitoring must keep UT-004 true: re-render without
   flash, scroll anchored to the same content.
6. **Zero implicit network.** Nothing fetches remote content on render —
   bundled assets only, test-enforced. The only permitted network use is
   an explicit one-click user action (e.g. a remote-image placeholder's
   load button). If a feature seems to need any other fetch, stop and flag.
7. **Per-window isolation.** No shared mutable state between windows
   (Apostrophe leaked class-attribute state across windows). Closing a
   window frees its resources.
8. **Performance budgets are tests.** If your change can affect startup,
   re-render latency, scroll smoothness, or memory, run the perf harness
   and report the numbers. A budget regression means the task is not done.
9. **Theme changes restyle, never re-parse.** Light/dark/high-contrast
   switching swaps styling live; it must not re-run the parser or reload
   the document.
10. **Deep modules, simple APIs — no drift.** Shallow/pass-through modules
   and wide or leaky interfaces are defects (see the `deep-modules`
   skill). Every report states whether touched crates' public API grew,
   held, or shrank; growth needs justification. Each commit tightens.
11. **The buffer is the source of truth while a window owns a file.**
   Rendering, outline, search, and export consume the document model, not
   the file. Any feature reading the file directly while a buffer exists
   is a bug.
12. **Never a modal.** Degraded or blocked content (remote image, failed
   plugin, unrenderable block, external file change on a dirty buffer)
   surfaces inline with a one-click affordance. A blocking dialog is a
   design defect (ux_decisions.md).
13. **Plugins are optional and isolated.** A disabled plugin costs
   nothing (no assets, no scans); a failing plugin degrades to the source
   block with an inline badge and never breaks the document, other
   plugins, or the app.

## Non-negotiable expectations (learned from prior rejected work)

- **Testability is a MUST.** No automated test → not done. Untestable
  behavior is escalated to the human (with a candidate testing approach or
  a drop proposal), never shipped on a manual check and never silently
  dropped. The only pre-accepted exceptions are listed in `docs/TESTING.md`.
- **Tests must assert what the USER SEES** — rendered output, visible
  state — not that an internal call happened.
- **Meet the issue's acceptance criteria literally.** Disabling a behavior
  to resolve its symptom is not a fix.
- **Rendering correctness disputes are settled by spec text** (CommonMark
  spec.txt, GFM spec) or a cited reference implementation, never by what
  looks plausible.
