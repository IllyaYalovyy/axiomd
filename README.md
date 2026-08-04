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
`webkitgtk6.0-devel`, `glib2-devel` (for `glib-compile-schemas`).

Preferences are GSettings-backed: `data/io.github.etf.axiomd.gschema.xml`
must be installed into a schema directory the desktop reads — the
system's, or `~/.local/share/glib-2.0/schemas` for a per-user install —
and `glib-compile-schemas` run over it. `scripts/install.sh` does both. A
copy built but not installed — `cargo run`, the test suite — uses the
schema its own build compiled, so nothing has to be installed to develop
or test.

## Installing

Native is the recommended way to run axiomd (`design_decisions.md`, owner
ruling 2026-08-03): no sandbox, no portal, the reader's own files opened
directly. axiomd is built by cargo and installed by one script — there is
no meson layer (RFC-001 Q1). `cargo install` alone is not enough: it
places the binary and none of the files the desktop reads, including the
compiled settings schema, without which the first preference read aborts.

### For yourself, without root (recommended)

```bash
cargo build --release
scripts/install.sh --user               # into ~/.local
```

That is the whole of it: the binary lands in `~/.local/bin`, the desktop
entry, icons, AppStream data and compiled settings schema in
`~/.local/share`, and the desktop reads all of them from there — axiomd
appears in the app grid and opens `.md` files from Files. The entry starts
the binary by its full path, so it works whether or not `~/.local/bin` is
on your `PATH`; the installer says how to add it if you also want to type
`axiomd` in a terminal.

To take it back out again — exactly the files it wrote, and the desktop's
caches rebuilt around whatever else lives in those directories:

```bash
scripts/install.sh --uninstall --user
```

### System-wide

```bash
cargo build --release
sudo scripts/install.sh                 # --prefix /usr/local by default
sudo scripts/install.sh --uninstall     # and back out again
```

`scripts/install.sh --help` lists the options (`--user`, `--prefix`,
`--destdir`, `--binary`, `--uninstall`) a packager needs.

### Flatpak (secondary)

Supported and maintained, but second in recommendation order: the sandbox
costs integration axiomd would rather have — the open defects are #22–#24
— and an overhead against native that is being measured and worked down
(#36). Prefer the native install above unless you specifically want the
packaged form.

It needs `flatpak-builder` and, once,

```bash
flatpak install --user flathub org.gnome.Platform//49 org.gnome.Sdk//49 \
    org.freedesktop.Sdk.Extension.rust-stable
```

then:

```bash
cd build-aux/flatpak
flatpak-builder --user --install --force-clean build io.github.etf.axiomd.json
flatpak run io.github.etf.axiomd
```

The build has no network. Every crate comes from
`build-aux/flatpak/cargo-sources.json`, generated from `Cargo.lock` and
committed beside the manifest; regenerate it whenever a dependency moves
(the gate fails if it is stale):

```bash
build-aux/flatpak/cargo-sources.py Cargo.lock -o build-aux/flatpak/cargo-sources.json
```

What the sandbox may reach is pinned in
`build-aux/flatpak/permissions.pinned` — no host filesystem at all, and
network solely so that pressing a remote image's placeholder can load it.
Both the manifest and the installed application are held to that file by
`crates/axiomd-app/tests/packaging.rs`; `scripts/quality.d/40-flatpak.sh`
builds the package, installs it, and probes it in its own sandbox.

Flathub submission is explicitly out of scope for now (owner, 2026-08-02).

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
├── build-aux/flatpak/           # Flatpak manifest, offline cargo mirror,
│                                #   pinned sandbox permissions
├── data/                        # Desktop entry, icons, metainfo, gschema
├── designs/
│   ├── RFC-001-mvp-architecture.md
│   ├── MVP-USER-TASKS.md        # User workflows as test plans
│   └── RFC-000-template.md
├── docs/                        # Process, testing, review, commit rules
├── scripts/
│   ├── quality.sh               # The quality gate (one command)
│   ├── install.sh               # Installs a built axiomd into a prefix
│                                #   (--user, system-wide, or --uninstall)
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
