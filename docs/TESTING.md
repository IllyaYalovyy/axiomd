# Testing Strategy

This project values tests that protect user behavior and system invariants over
raw coverage percentages.

## The testability rule (non-negotiable)

**A feature or bug fix is NOT done without an automated test.** A manual check
never closes a task; at most it supplements automation.

If a behavior cannot be tested automatically, the task **stops and escalates**:
report exactly what is untestable, why, and at least one candidate way to test
it (or a proposal to drop the behavior). The human decides — drop it, or build
the way to test it. Silently shipping untested behavior and silently dropping
scope are both violations.

## Test Layers

Use the lowest layer that catches the risk clearly:

- **Unit tests** - pure functions, event/span invariants, parsing, wikilink
  resolution, anchor-map logic, state transitions
- **Golden tests** - the rendering pipeline: `.md` fixture → pinned `.html`
  (byte-for-byte after normalization); spec conformance suites (CommonMark,
  GFM); MathML output corpus
- **Integration tests** - scheme handler, file monitoring/debounce/cancel,
  sanitization, link policy, zero-network guarantees
- **e2e (headless UI) tests** - the real app under a headless display,
  driven and asserted through the webview DOM bridge (`evaluate_javascript`)
  and GTK actions; user flows from `designs/MVP-USER-TASKS.md`.
  The harness is the `axiomd-e2e` crate: `launch(document)` starts the
  shipped binary on a headless weston of its own, `app.dom(js)` /
  `app.dom_text(selector)` read the rendered document, `app.activate(action)`
  fires the action a menu item fires, `app.screenshot()` captures pixels, and
  `app.close()` returns the processes that outlived the run. Every wait is a
  condition with a deadline — the harness contains no sleeps, and neither may
  a test written on it.
- **Screenshot goldens** - visual fidelity: a rendered fixture is approved by
  a human ONCE, then pinned as an image and diffed thereafter. Subjective
  "does it look right" becomes objective "did it change without approval".
  Re-pinning a golden is a human-approved act, recorded in the commit.
- **Property / fuzz tests** - parser inputs, spans, untrusted documents
- **Perf budget tests** - stated numbers asserted by the perf harness

## What is accepted as not fully automatable

Ratified by the project owner on 2026-08-02 (categories 2 and 3 explicitly:
integration and packaging checks beyond the automated boundary are done
manually by the owner). This list is exhaustive; growing it is a human
decision. Everything on it has a defined partial check:

1. **First-time visual approval** of a new rendered surface — human approves
   once; the screenshot golden pins it forever after.
2. **Desktop-environment integration** (Files double-click, default-handler
   pick-up) — automated to the boundary: desktop-file validation, `xdg-mime`
   query checks, and launching the real binary with a file argument headless.
   The final DE-side dispatch is the platform's contract, not ours.
3. **Flatpak runtime behavior** — asserted by scripted probes that drive the
   *installed* flatpak: the packaged application opens a document and renders
   it, a document full of remote images makes zero requests until one is
   pressed, a picture the author kept beside the document arrives while one a
   folder above it does not, the sandbox writes nothing but the document the
   portal granted, and the sandbox's own permissions are compared with the
   pinned `build-aux/flatpak/permissions.pinned`. They are the `#[ignore]`d
   tests in `crates/axiomd-app/tests/packaging.rs`, built, installed and run by
   `scripts/quality.d/40-flatpak.sh`. The store-side install flow is not ours.

If work surfaces a fourth category, it goes to the human before the task
closes.

## Regression Rule

Every fixed bug leaves behind a test that fails without the fix.

Every new user-facing behavior has a test that fails if the behavior
disappears. Tests are demonstrated red first (see `agent/skills/test-quality`).

## Risk Matrix

| Risk / failure mode | User impact | Test layer | Coverage |
|---|---|---|---|
| Rendering deviates from CommonMark/GFM | Wrong document | Golden (spec suites) | engine spec tests |
| Span drift | Outline/scroll/search/reload all break | Property + unit | engine span tests |
| Malicious document executes/exfiltrates | Security | Integration (sanitize, CSP, egress) | render + app tests |
| Full-page reload regression | Flash, lost position | e2e (navigation count) | app e2e |
| Budget regression | App feels slow again | Perf harness | tests/perf |
| Visual regression | Ugly render | Screenshot golden | e2e goldens |

## Local Quality Gate

```bash
./scripts/quality.sh
```

Add project-specific checks as executable files in `scripts/quality.d/`
(the e2e harness and perf subset wire in there).

## Test Naming

Prefer names that describe the requirement:

```text
preserves_scroll_anchor_across_live_reload
renders_remote_image_as_placeholder_with_zero_egress
outline_click_scrolls_to_heading_anchor
```

Avoid names that only describe the implementation:

```text
test_update
handler_returns_true
component_renders
```
