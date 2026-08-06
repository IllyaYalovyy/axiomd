#!/usr/bin/env bash
#
# The package's place in the quality gate (issue #14).
#
# Most of what a package can get wrong is checked by ordinary tests that the gate's
# own `cargo test --workspace` already runs: the manifest's permissions, the icons,
# the AppStream metainfo, the offline cargo mirror and the prefix
# `scripts/install.sh` produces are all in `crates/axiomd-app/tests/packaging.rs`,
# where they fail on the machine of whoever changed them.
#
# Three things cannot be checked there, and this hook is why they are checked at all:
#
#   1. the manifest still builds. `flatpak-builder` is the only thing that can say
#      so, and it says it by building — which also proves the mirror really is
#      complete, because the build has no network to fall back on.
#   2. the packaged application runs. The probes `docs/TESTING.md` category 3 asks
#      for drive the *installed* flatpak: it opens a document, renders it, and a
#      document full of remote images reaches the network only when the reader
#      presses one. They are `#[ignore]`d, so they run here and nowhere else.
#   3. the installed sandbox's permissions are the pinned ones — including after a
#      `flatpak override` somebody ran on this machine, which the manifest cannot
#      know about.
#
# Without flatpak-builder or the runtime this hook says what is missing and does not
# run, the way the issue asks: a packaging toolchain is not something every developer
# has installed, and a gate that demanded one would be a gate nobody could run. It
# says so loudly rather than passing quietly — an unchecked package must never look
# like a checked one.

set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
cd "${repo_root}"

app_id=io.github.etf.axiomd
manifest="build-aux/flatpak/${app_id}.json"
runtime_version=$(sed -n 's/.*"runtime-version": "\([^"]*\)".*/\1/p' "${manifest}")

fail() {
    printf 'flatpak gate check failed: %s\n' "$*" >&2
    exit 1
}

skip() {
    printf 'flatpak: NOT CHECKED — %s\n' "$*"
    exit 0
}

has() {
    command -v "$1" >/dev/null 2>&1
}

# This hook starts sandboxed applications and the compositors they draw on, and it exits
# on the first failure — including failures inside a probe, which is exactly when a launch
# is most likely to be left behind. So what it started is ended when it leaves, however it
# leaves (issue #44).
leak_baseline=$(mktemp)
"${repo_root}/scripts/leak-sweep.sh" baseline "${leak_baseline}"
sweep_before_exiting() {
    local status=$?
    "${repo_root}/scripts/leak-sweep.sh" sweep "${leak_baseline}" || status=1
    rm -f "${leak_baseline}" "${output:-}"
    exit "${status}"
}
trap sweep_before_exiting EXIT

has flatpak ||
    skip "flatpak is not installed, so the package cannot be built or probed here.
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

# 1. The manifest is valid, and is the manifest flatpak-builder will read. This also
#    catches JSON that no longer parses, which the tests in packaging.rs deliberately
#    leave to the tool that owns the format.
flatpak-builder --show-manifest "${manifest}" >/dev/null ||
    fail "flatpak-builder cannot read ${manifest}."

# 2. Build and install it. `--force-clean` so a half-finished earlier run cannot be
#    mistaken for a successful one; the state directory is inside build-aux/flatpak so
#    the cache lives with the manifest rather than in whatever directory this was run
#    from. The build itself has no network: every crate comes from the committed
#    mirror, so a stale mirror fails here.
printf '\n==> Building and installing %s (org.gnome.Platform//%s)\n' \
    "${app_id}" "${runtime_version}"
(
    cd build-aux/flatpak
    flatpak-builder --user --install --force-clean \
        --state-dir=.flatpak-builder build "${app_id}.json"
) || fail "the flatpak build failed. Its output is above."

# 3. What the installed sandbox may reach, shown in the gate's own output: an exit
#    criterion of issue #14 is that this is read rather than assumed. The probe below
#    is what asserts it; printing it is so that the person running the gate sees it.
printf '\n==> flatpak info --show-permissions %s\n' "${app_id}"
flatpak info --show-permissions "${app_id}"

# 4. The probes, and what containment they are held to. `--test-threads=1`: each one
#    starts a compositor and a sandboxed application, and two of those competing for the
#    machine is a slow test pretending to be a flaky one. `--nocapture` so what the
#    packaged application said reaches whoever ran the gate.
#
#    Two targets: `packaging` is what the package does, `containment` is what it must not
#    do to the session it is probed in — draw on the developer's display, register on
#    their session bus, or be counted by their desktop as a running application
#    (issue #44). Both are `#[ignore]`d and neither runs anywhere else.
output=$(mktemp)

cargo test -p axiomd-app --test packaging --test containment -- \
    --ignored --nocapture --test-threads=1 2>&1 | tee "${output}" || true

mapfile -t summaries < <(grep -E '^test result:' "${output}" || true)
[[ ${#summaries[@]} -eq 2 ]] ||
    fail "the probes did not finish: ${#summaries[@]} of the 2 targets reported a result.
  Their output is above."

for summary in "${summaries[@]}"; do
    grep -qE '^test result: ok\.' <<<"${summary}" ||
        fail "a probe against the installed flatpak failed. Its output is above."
done

# A probe that was filtered out is a probe nobody ran, and the gate would otherwise
# report a clean run of nothing at all.
ran=$(printf '%s\n' "${summaries[@]}" |
    sed -n 's/^test result: ok\. \([0-9]*\) passed.*/\1/p' |
    awk '{ total += $1 } END { print total + 0 }')
[[ "${ran}" -ge 10 ]] ||
    fail "only ${ran} probe(s) ran against the installed flatpak. Every #[ignore]d test
  in the packaging and containment targets is one of these probes and every one of them
  must run."

printf '\nflatpak: built, installed, probed in its own sandbox, and contained — no window,
  no bus name and no session scope in the session the gate ran in (issue #44)\n'
