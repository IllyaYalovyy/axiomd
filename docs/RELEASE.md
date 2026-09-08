# Releasing axiomd

axiomd is distributed through **Flathub**, and for now Flathub only. The
distribution targets are Fedora and Ubuntu; one flatpak covers both, and both
install it through GNOME Software (Fedora out of the box, Ubuntu after a
one-time `apt install flatpak gnome-software-plugin-flatpak`). Snap, deb/PPA,
COPR/RPM and AppImage are separate channels, separately decided, and are not
part of this procedure.

Everything below is meant to be followed top to bottom. Where a step can be
got wrong silently, the quality gate has a test that catches it — those are
named as they come up, so that a green gate is the evidence and not a hope.

## What ships

Two files go to Flathub, and both are in this repository:

| In this repository | In the Flathub repository |
| --- | --- |
| `build-aux/flatpak/io.github.etf.axiomd.release.json` | `io.github.etf.axiomd.json` |
| `build-aux/flatpak/cargo-sources.json` | `cargo-sources.json` |

`build-aux/flatpak/io.github.etf.axiomd.json` — the *development* manifest —
never leaves this repository. It builds from a directory, which is what a
development machine and the gate want and what Flathub cannot take.

The release manifest is not maintained separately. It is **held equal** to the
development manifest by
`the_release_manifest_is_the_development_manifest_built_from_a_tag` in
`crates/axiomd-app/tests/packaging.rs`: the two must be the same package in
every module, finish-arg, build-option and cleanup entry, and may differ only
in the axiomd module's `sources` — a directory here, a tag of the public
repository there. A runtime bump, a new module, a changed build command or a
permission change therefore has to be made in **both files**, and the gate
fails until it is.

The renaming in the table above is the whole of the difference between the two
columns. It is needed because Flathub requires the submitted manifest to be
named after the app ID, and this repository cannot have two files with that
name. Nothing inside the file changes; `cargo-sources.json` sits beside the
manifest in both places, which is how the manifest's `"cargo-sources.json"`
source finds it.

## Before the first submission: two things only the owner can settle

**1. The app ID has to be reachable.** Flathub's linter resolves an
`io.github.<user>.<app>` ID to `https://github.com/<user>/<app>` and requires
it to exist. `io.github.etf.axiomd` resolves to `https://github.com/etf/axiomd`,
which is 404 (verified 2026-09-07); the repository is
`https://github.com/IllyaYalovyy/axiomd`. The linter reports this as
`appid-url-not-reachable`, and it is an error, not a warning — a submission
will not pass review with it outstanding.

There is no fix that an implementer can pick, because the app ID is the
application's identity: it is the D-Bus name, the desktop-entry name, the
settings-schema path, the icon names, the metainfo ID and the flatpak ref, and
it is what an already-installed copy would be replaced by. The owner's options:

- rename the application to an ID under an account that does hold the
  repository (for example `io.github.illyayalovyy.axiomd` — Flathub requires
  the segment to match the GitHub account, and IDs are lowercase); or
- move or mirror the repository to `github.com/etf/axiomd`, if that account is
  the owner's; or
- adopt an ID under a domain the owner controls (`com.example.Axiomd`), which
  Flathub verifies by a file served from that domain.

Whichever it is, it is a change across the whole repository and its own piece
of work — not part of cutting a release.

**2. The `--filesystem=host:ro` exception, and how axiomd is disclosed.**
The permission is frozen; what has to be decided is the explanation Flathub is
given for it, and — inseparably — how this project's AI-generated content is
disclosed under Flathub's generative AI policy, which refuses sandbox-widening
exceptions where it sees signs of LLM usage. See
[The `host:ro` rationale](#the-hostro-rationale) and
[Flathub's generative AI policy](#flathubs-generative-ai-policy). Both are the
owner's to write, personally; nothing about the sandbox changes either way.

## Releasing

### 1. Decide the version and its kind

The workspace version in `Cargo.toml` is the version. The metainfo `<release>`
entry carries `type="development"` or `type="stable"`; a software centre shows
a development release differently and some will not offer it at all. Which one
this release is, and whether the first published version is `0.1.0` or
`1.0.0`, is the owner's call.

### 2. Bump the version

```bash
# Cargo.toml, [workspace.package]
version = "<version>"
```

Then refresh the lock file so the workspace members carry the new version:

```bash
cargo check --workspace
```

### 3. Add the metainfo release entry

In `data/io.github.etf.axiomd.metainfo.xml`, newest first inside `<releases>`:

```xml
<release version="<version>" date="<YYYY-MM-DD>" type="stable">
  <description>
    <p>What a reader gets that they did not have before.</p>
  </description>
</release>
```

This is what GNOME Software shows as "What's new". Written for readers, not for
the commit log.

### 4. Move the release manifest to the new tag

In `build-aux/flatpak/io.github.etf.axiomd.release.json`, in the axiomd
module's `sources`:

```json
{
    "type": "git",
    "url": "https://github.com/IllyaYalovyy/axiomd.git",
    "tag": "v<version>",
    "commit": "TAG-COMMIT-SHA"
}
```

The tag is `v` and the workspace version — the gate asserts exactly that, and
asserts the URL is the one the metainfo publishes as its `vcs-browser`, so a
version bump that forgot this file fails before it reaches Flathub. The commit
goes back to the `TAG-COMMIT-SHA` placeholder here and is filled in at step 8,
once the tag exists to be pinned to.

### 5. Regenerate the offline mirror

A flatpak build has no network: every crate comes from the committed mirror.

```bash
build-aux/flatpak/cargo-sources.py Cargo.lock -o build-aux/flatpak/cargo-sources.json
```

Two gate tests hold this honest —
`the_offline_cargo_sources_are_the_lock_file_this_build_uses` regenerates it
and compares bytes, and
`the_offline_mirror_carries_exactly_the_locked_dependencies` reads the mirror
and `Cargo.lock` against each other and names any crate that is in one and not
the other. A forgotten regeneration is a failing gate, not a build failure on
Flathub's builders a week later.

### 6. Run the gate

```bash
./scripts/quality.sh
```

Green, with no skip variables set. Two of its hooks matter most here:

- `scripts/quality.d/40-flatpak.sh` builds and installs the development
  manifest and drives the installed package;
- `scripts/quality.d/60-flatpak-pristine.sh` writes out what a tag of the
  current commit would hand a builder (`git archive HEAD`) and builds and
  installs the **release** manifest from that and nothing else. If the build
  ever leaned on a file nobody committed, this is where it fails — here rather
  than on Flathub's builders, after the tag is cut.

If either says `NOT CHECKED`, the packaging toolchain is missing on this
machine and the release is not verified. Install what it names and run again.

### 7. Commit, tag, push

```bash
git commit -am "release: v<version>"
git tag -a "v<version>" -m "axiomd v<version>"
git push origin main "v<version>"
```

### 8. Pin the release manifest to the tag

```bash
git rev-parse "v<version>^{commit}"
```

Put that sha in the release manifest's `"commit"`, in place of
`TAG-COMMIT-SHA`, and push it:

```bash
git commit -am "packaging: pin the release manifest to v<version>"
git push origin main
```

`the_release_manifest_is_pinned_to_the_tag_it_names_once_that_tag_exists`
checks the pairing whenever the tag is present in the checkout, so a manifest
left pointing at the previous release's commit fails the gate.

### 9. Publish the GitHub release

Create the release from the tag at
`https://github.com/IllyaYalovyy/axiomd/releases`, with the same notes as the
metainfo entry.

### 10. Run Flathub's own linter

See [Running Flathub's linter](#running-flathubs-linter). Every finding is
either fixed or one of the four recorded there; a new one is neither.

### 11. Submit

Flathub's own instructions are at
<https://docs.flathub.org/docs/for-app-authors/submission>; this is that
procedure with axiomd's names filled in. Read Flathub's page as well — it is
the authority and it changes.

Before opening anything, build the submission the way Flathub asks you to,
which is with its own builder rather than the distribution's:

```bash
flatpak remote-add --if-not-exists --user flathub \
    https://dl.flathub.org/repo/flathub.flatpakrepo
cd "${submission}"   # the directory prepared in step 10
flatpak run --command=flathub-build org.flatpak.Builder --install \
    io.github.etf.axiomd.json
flatpak run io.github.etf.axiomd
```

**First release** — a new-app pull request to
[`flathub/flathub`](https://github.com/flathub/flathub):

```bash
gh repo fork --clone flathub/flathub && cd flathub
git checkout --track origin/new-pr
git checkout -b axiomd-submission new-pr
```

Add exactly two files at the top of that branch and nothing else:

- `io.github.etf.axiomd.json` — a copy of
  `build-aux/flatpak/io.github.etf.axiomd.release.json`;
- `cargo-sources.json` — a copy of `build-aux/flatpak/cargo-sources.json`.

Push, and open the pull request **against the `new-pr` base branch** — not
`master` — with the title `Add io.github.etf.axiomd`. Fill in the template the
repository gives you rather than replacing it; a PR with the template removed
may be closed unreviewed. Flathub's bot builds the manifest on the PR and a
reviewer reads the permissions and the metainfo.

The submission PR is also where the two disclosures below belong: the
[`host:ro` rationale](#the-hostro-rationale) and the
[AI disclosure](#flathubs-generative-ai-policy). Both are yours to write —
see those sections for why that is not a formality.

Renaming the app ID after acceptance means resubmitting from scratch, which is
the other half of why
[the app ID has to be settled first](#before-the-first-submission-two-things-only-the-owner-can-settle).

After acceptance, accept the invite to the new
`flathub/io.github.etf.axiomd` repository.

**Every release after that** — a pull request to
`flathub/io.github.etf.axiomd` changing the manifest's `tag` and `commit`,
with `cargo-sources.json` regenerated alongside if dependencies moved. That is
the whole of it; an update never goes through the submission process again.

## The `host:ro` rationale

Flathub's linter reports `finish-args-host-ro-filesystem-access` on this
manifest. It is expected, it is permanent, and **the answer is never a sandbox
change**: the permission is a ratified owner ruling (issue #23, 2026-08-03),
its reasons are recorded in `build-aux/flatpak/permissions.pinned`, and
`crates/axiomd-app/tests/packaging.rs` pins the manifest's `finish-args` and
the installed application's own permissions to that file so it cannot be
widened — or narrowed — without a failing gate.

Flathub's criterion for this particular error is "granted on sufficient
explanation being provided" (verified against
<https://docs.flathub.org/docs/for-app-authors/linter>, 2026-09-07), and an
exception is a pull request against
[`exceptions.json`](https://github.com/flathub/flatpak-builder-lint/blob/master/flatpak_builder_lint/staticfiles/exceptions.json)
in the linter's repository, in this shape:

```json
"io.github.etf.axiomd": {
    "stable": {
        "finish-args-host-ro-filesystem-access": "<the explanation>"
    }
}
```

What is already published for an app can be read back with
`curl -s https://flathub.org/api/v2/exceptions/io.github.etf.axiomd`.

**Write the explanation yourself.** Flathub's exception policy is explicit
that an LLM must not be used to handle an exception PR, and that exceptions
for sandbox-widening permissions — `host` among them — will not be granted if
there are signs of LLM usage in the exception PR or in the software. See
[the AI policy section](#flathubs-generative-ai-policy) below, which is not a
formality for this project.

What follows is therefore **not text to paste**. It is the record of why the
permission exists and what holds it in place, so that the explanation you write
is accurate:

- axiomd is a Markdown *viewer*, and a Markdown document is not self-contained:
  it references pictures by relative path, and authors keep those pictures
  beside the document or in a folder above it.
- Under the document portal alone, a document opened from Files arrives in a
  directory containing that one file. Every relative image in it is broken, and
  no portal would hand the others over: the application cannot ask for files
  whose names it does not know until it has parsed the document, and asking
  would interrupt a reading path this project deliberately keeps free of
  dialogs (`ux_decisions.md`, invariant 12).
- The grant is read-only, and that is the whole of it. axiomd writes nothing
  outside the one document the portal granted it; saving goes through the
  portal-provided handle.
- That is tested, not asserted. Probes drive the *installed* flatpak and
  assert that a picture kept beside the document arrives, that one a folder
  above it does not arrive through any write path, and that the sandbox writes
  nothing but the granted document (`crates/axiomd-app/tests/packaging.rs`,
  run by `scripts/quality.d/40-flatpak.sh`).
- `--filesystem=host` (writable), `home` and the `xdg-` directories were each
  considered and each rejected as wider than the application needs. The
  permission set is pinned in-tree (`build-aux/flatpak/permissions.pinned`) and
  asserted against both the manifest and the installed application, so it
  cannot drift — including by a hand-run `flatpak override`.
- Network access is likewise minimal and explicit. Documents fetch nothing on
  render: remote images are drawn as placeholders with a load button, and a
  test counts requests from the far end to prove a rendered document makes
  zero. `--share=network` exists only so that pressing that button can work.

## Flathub's generative AI policy

Read <https://docs.flathub.org/docs/for-app-authors/requirements#generative-ai-policy>
before submitting. Verified 2026-09-07, it requires that:

- submitters **disclose** AI-generated code, documentation, packaging or other
  material included in the application or its Flathub packaging, identifying
  the affected parts and the approximate extent;
- AI tools or agents **must not** open or automate Flathub submission pull
  requests, or generate their commit messages, descriptions, review comments or
  replies, and submitters must not request AI-agent reviews;
- disclosed material is evaluated at reviewer discretion, and a submission may
  be rejected on the extent or role of that material.

This matters to axiomd more than to most submissions, and pretending otherwise
would waste a reviewer's time and risk a ban: this repository is largely
AI-implemented, including this document, the manifests and the packaging tests.
The `host:ro` exception policy compounds it — exceptions for sandbox-widening
permissions are refused where there are signs of LLM usage in the software.

None of that is an implementer's call to make. Before the first submission the
owner needs to decide how axiomd is disclosed and whether the `host:ro`
exception is realistic on those terms, and to write the submission PR, its
description and every reply personally.

## Running Flathub's linter

The linter is `flatpak-builder-lint`, run the way Flathub's CI runs it — out of
the `org.flatpak.Builder` flatpak, so that the version is Flathub's and not a
distribution's:

```bash
flatpak install --user flathub org.flatpak.Builder
```

It insists the manifest be named after the app ID, so lint a copy under the
name it is submitted as rather than the one it has here:

```bash
submission=$(mktemp -d)
cp build-aux/flatpak/io.github.etf.axiomd.release.json \
   "${submission}/io.github.etf.axiomd.json"
cp build-aux/flatpak/cargo-sources.json "${submission}/"

flatpak run --command=flatpak-builder-lint --filesystem="${submission}" \
    org.flatpak.Builder manifest "${submission}/io.github.etf.axiomd.json"

flatpak run --command=flatpak-builder-lint org.flatpak.Builder \
    appstream data/io.github.etf.axiomd.metainfo.xml
```

Keep `${submission}` — step 11 builds out of it. Flathub runs a third check
against the exported repository, which needs the build of step 11 to have
happened first:

```bash
cd "${submission}"
flatpak run --command=flathub-build org.flatpak.Builder --repo=repo \
    io.github.etf.axiomd.json
flatpak run --command=flatpak-builder-lint org.flatpak.Builder repo repo
```

Then `rm -rf "${submission}"`.

The linter reaches the network (it resolves the app ID's URL and asks about
runtime versions), which is why it is a release step and not a gate check: the
quality gate is offline by design.

What it says today, on `flatpak-builder-lint 3.0.0` (run 2026-09-07), and what
each finding means:

| Finding | Kind | Response |
| --- | --- | --- |
| `finish-args-host-ro-filesystem-access` | error | Expected and permanent. Answered by an exception request the owner writes from [the rationale above](#the-hostro-rationale). Never answered by changing the sandbox. |
| `appid-url-not-reachable` | error | Real, and blocking. `io.github.etf.axiomd` resolves to a repository that does not exist — see [Before the first submission](#before-the-first-submission-two-things-only-the-owner-can-settle). Must be settled before the first PR. |
| `runtime-update-available-to-org.gnome.Platform-50` | warning | Not blocking. The runtime is pinned to 49 on purpose: the code's GTK 4.20 / libadwaita 1.8 feature gates are written against it, and moving it is one human-approved change that moves the runtime and those pins together (issue #14, asserted by `the_manifest_pins_the_runtime_the_code_is_written_against`). |
| `appid-filename-mismatch` | error, only in-tree | An artefact of this repository's filename, not of the manifest. It does not appear when the file is linted under its submission name, which is what the recipe above does — lint the copy, not `…release.json` in place. |

The `appstream` run is clean.

Anything not in that table is a new finding: fix it, or record it here with its
reason before submitting.

## Rolling back

A release that turns out broken is withdrawn by publishing the next one —
Flathub has no unpublish for a version that users already have. Fix forward:
bump the patch version and follow this document again from step 1.
