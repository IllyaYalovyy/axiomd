#!/usr/bin/env bash
#
# The package built from what git holds, and nothing else (issue #56).
#
# `40-flatpak.sh` builds `io.github.etf.axiomd.json`, whose axiomd module is a
# `"type": "dir"` source pointing at the working tree. That is the right source for a
# development machine, and it is a source that cannot tell the difference between a file
# this project ships and a file that only exists on the machine doing the building: an
# uncommitted module, a generated asset nobody added, a stylesheet left over from a
# branch. Flathub gets none of those. It clones a tag and builds what is in it, so a
# build that quietly leaned on an untracked file succeeds here and fails there — after
# the tag is cut and in front of a reviewer.
#
# This hook is the difference, made visible: `git archive HEAD` writes out exactly the
# bytes a tag of this commit would hand a builder, and the *release* manifest inside that
# archive is what gets built and installed from it. Its one substitution is the source
# type — `{"type": "git", ...}` becomes `{"type": "dir", "path": "../.."}` over the
# archive — because the commit this checks has no tag yet and no push behind it. Every
# other thing about the build is the submission's: the same modules, the same offline
# mirror, the same install script, the same finish-args.
#
# It shares `40-flatpak.sh`'s state directory on purpose. The crate mirror is some eight
# hundred downloads and libspelling is a meson build; both are cached by content, so this
# hook pays for the one module that really differs — axiomd itself, compiled from the
# archive. That is also why the archive is written under `target/` rather than into
# `/tmp`: flatpak-builder hard-links out of its state directory and refuses a build
# directory on another filesystem, and `target/` is both on this one and the one place
# the development manifest's dir source already skips.
#
# It runs after every hook that drives the *installed* flatpak (`40`, `50`), because it
# installs over it, and what it leaves behind is the more honest of the two: the package
# built from the sources a release is made of.
#
# Without flatpak-builder or the runtime this says what is missing and does not run — the
# same rule `40-flatpak.sh` is written to. An unchecked package must never look like a
# checked one.

set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
cd "${repo_root}"

app_id=io.github.etf.axiomd
manifest_name="${app_id}.release.json"
manifest="build-aux/flatpak/${manifest_name}"
runtime_version=$(sed -n 's/.*"runtime-version": "\([^"]*\)".*/\1/p' "${manifest}")
state_dir="${repo_root}/build-aux/flatpak/.flatpak-builder"

fail() {
    printf 'pristine-source flatpak build failed: %s\n' "$*" >&2
    exit 1
}

skip() {
    printf 'flatpak (pristine sources): NOT CHECKED — %s\n' "$*"
    exit 0
}

has() {
    command -v "$1" >/dev/null 2>&1
}

archive=
clean_up() {
    local status=$?
    [[ -n "${archive}" ]] && rm -rf -- "${archive}"
    exit "${status}"
}
trap clean_up EXIT

has flatpak ||
    skip "flatpak is not installed, so the package cannot be built here.
  Install it with: sudo dnf install flatpak"
has flatpak-builder ||
    skip "flatpak-builder is not installed, so the package cannot be built here.
  Install it with: sudo dnf install flatpak-builder"

for needed in "org.gnome.Platform//${runtime_version}" "org.gnome.Sdk//${runtime_version}" \
    org.freedesktop.Sdk.Extension.rust-stable; do
    flatpak info "${needed}" >/dev/null 2>&1 ||
        skip "${needed} is not installed, so the package cannot be built here.
  Install what the manifest needs with:
    flatpak install --user flathub org.gnome.Platform//${runtime_version} \\
      org.gnome.Sdk//${runtime_version} org.freedesktop.Sdk.Extension.rust-stable"
done

commit=$(git rev-parse HEAD)
printf '\n==> Writing out what a tag of %s would hand a builder\n' "${commit:0:12}"
if ! git diff --quiet HEAD || [[ -n $(git ls-files --others --exclude-standard) ]]; then
    printf '    (the working tree has changes this commit does not; they are not built here,\n'
    printf '     which is the point — commit them and the gate builds them.)\n'
fi

mkdir -p "${repo_root}/target"
archive=$(mktemp -d "${repo_root}/target/pristine-flatpak-XXXXXXXX")
git archive --format=tar HEAD | tar -x -C "${archive}"

[[ -f "${archive}/${manifest}" ]] ||
    fail "${manifest} is not committed, so a tag would not carry the manifest a
  submission is made of."

# The one substitution, and it is written by a JSON parser rather than a regex so that a
# manifest it cannot read fails here rather than building something unintended.
python3 - "${archive}/${manifest}" <<'PY' || fail "the release manifest could not be rewritten to build from the archive."
import json
import sys
from collections import OrderedDict
from pathlib import Path

path = Path(sys.argv[1])
manifest = json.loads(path.read_text(), object_pairs_hook=OrderedDict)
modules = [m for m in manifest["modules"] if isinstance(m, dict) and m.get("name") == "axiomd"]
if len(modules) != 1:
    raise SystemExit("the release manifest has no single axiomd module")
sources = modules[0]["sources"]
git = [s for s in sources if isinstance(s, dict) and s.get("type") == "git"]
if len(git) != 1:
    raise SystemExit("the release manifest's axiomd module does not build from git")
modules[0]["sources"] = [
    OrderedDict([("type", "dir"), ("path", "../..")]) if s is git[0] else s for s in sources
]
path.write_text(json.dumps(manifest, indent=4) + "\n")
PY

printf '\n==> Building and installing %s from those sources alone (org.gnome.Platform//%s)\n' \
    "${app_id}" "${runtime_version}"
(
    cd "${archive}/build-aux/flatpak"
    flatpak-builder --user --install --force-clean \
        --state-dir="${state_dir}" pristine-build "${manifest_name}"
) || fail "the package does not build from the sources git holds. Its output is above.
  The usual cause is a file the build needs that nobody committed: the dir-source build
  in 40-flatpak.sh finds it in the working tree, a tag does not have it, and Flathub's
  builders would fail exactly here."

flatpak info "${app_id}" >/dev/null 2>&1 ||
    fail "the pristine build did not install."

printf '\nflatpak: %s builds and installs from git-held sources alone (commit %s) —
  the release manifest, its offline mirror and its install script, with nothing from
  this working tree behind them\n' "${app_id}" "${commit:0:12}"
