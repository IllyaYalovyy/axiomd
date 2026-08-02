# axiomd

A fast, beautiful Markdown viewer for modern GNOME, written in Rust.

axiomd opens `.md` files instantly, renders them with best-in-class
fidelity (CommonMark + GFM core with first-class tables and images;
diagrams, math, callouts, wikilinks via optional plugins), and stays
responsive with many windows and huge documents. Reading is the soul of
the app; editing, print, and PDF/HTML export are built in from day one.
See `VISION.md` for the full product vision and
`designs/RFC-001-mvp-architecture.md` for the architecture.

Status: pre-MVP. The development plan lives in RFC-001; work items are
GitHub issues mirrored into the local ktask queue.

## Stack

- Rust, cargo workspace: `axiomd-engine` (selectable parsers behind a
  sealed engine boundary; comrak and pulldown-cmark), `axiomd-render`
  (events → sanitized HTML with source anchors, syntect highlighting, and
  the plugin layer), `axiomd-doc` (editable document model),
  `axiomd-app` (gtk4-rs + libadwaita + WebKitGTK 6 + GtkSourceView).
- No external converters; zero implicit network (all assets bundled; the
  only network use is an explicit one-click remote-image load).

Build prerequisites (Fedora): `gtk4-devel`, `libadwaita-devel`,
`webkitgtk6.0-devel`.

## Project Workflow

This repository was created from `ai-proj-template` and follows the
practices evolved in `axiotask`:

1. Write or update the user task / problem statement (GitHub issue with
   Scope, Tests (red first), and Exit criteria).
2. Create an RFC for broad, irreversible, cross-cutting, or
   dependency-adding changes.
3. Implement in small reviewable steps, tests red first.
4. Run `./scripts/quality.sh`.
5. Review for behavior, regressions, secrets, and maintainability before
   merge; push to `origin/main` only with a green gate.

Decision records: `design_decisions.md` and `ux_decisions.md` are FAQ-format
ADRs. Entries there are intentional decisions, not gaps — do not "fix" them
without an explicit decision to change direction.

## Repository Layout

```text
.
├── VISION.md                    # Product vision and principles
├── design_decisions.md          # Architecture ADR (FAQ form)
├── ux_decisions.md              # UX ADR (FAQ form)
├── AGENTS.md                    # Instructions for AI coding agents
├── CONTRIBUTING.md              # Contributor rules and quality bar
├── agent/
│   ├── skills/                  # Tracked source of agent skills
│   └── ktask/                   # Tracked source of ktask prompt/context
├── designs/
│   ├── RFC-001-mvp-architecture.md
│   ├── MVP-USER-TASKS.md        # User workflows as test plans
│   └── RFC-000-template.md
├── docs/                        # Process, testing, review, commit rules
├── scripts/
│   ├── quality.sh               # The quality gate (one command)
│   ├── install-agent-files.sh   # agent/ → gitignored .claude/ + .ktask/
│   └── install-git-hooks.sh     # Pre-commit AI-file guard
└── .github/                     # Issue templates
```

## Quality Gate

Local tests are the gate — there is no CI/CD (deliberate decision, 2026-08-02):

```bash
./scripts/quality.sh
```

Rust: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`.
Project-specific checks live under `scripts/quality.d/`.
