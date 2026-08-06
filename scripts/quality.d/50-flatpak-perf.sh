#!/usr/bin/env bash
#
# The package measured against the build (issue #36).
#
# The owner reported the flatpak feeling slower than the native build, and a supported
# distribution that is quietly slower than the recommended one is a defect nobody has a
# number for. `crates/axiomd-app/tests/parity.rs` is those numbers: every metric is
# measured on both forms in the same run, and the packaged form is held to a ceiling of
# its own that only ever comes down. `designs/flatpak-parity.md` is the committed
# evidence, and the metrics in it are what this hook checks were measured.
#
# Why this is a hook rather than an ordinary test, twice over:
#
#   1. it drives the *installed* flatpak, which not every developer has — and which
#      `40-flatpak.sh`, running just before this, is what builds and installs;
#   2. a budget measured in a debug build measures nothing anybody ships, so this runs
#      the target in release, one test at a time, the way `20-perf.sh` does.
#
# Without a flatpak this says what is missing and does not run. An unmeasured package
# must never look like a measured one, so it says it loudly rather than passing quietly.

set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
cd "${repo_root}"

app_id=io.github.etf.axiomd
evidence=designs/flatpak-parity.md
begin='<!-- pinned:begin -->'
end='<!-- pinned:end -->'

fail() {
    printf 'flatpak parity check failed: %s\n' "$*" >&2
    exit 1
}

skip() {
    printf 'flatpak parity: NOT MEASURED — %s\n' "$*"
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

has weston ||
    fail "the parity budgets drive the real application on a headless compositor.
  Install it with: sudo dnf install weston"

has flatpak ||
    skip "flatpak is not installed, so there is no package to measure.
  Install it with: sudo dnf install flatpak"

flatpak info "${app_id}" >/dev/null 2>&1 ||
    skip "no ${app_id} is installed, so there is no package to measure.
  Build and install it with: ./scripts/quality.d/40-flatpak.sh"

# The metrics are read from the committed evidence rather than written out here: that
# file is checked against the harness by `the_committed_table_is_what_this_run_pins`, so
# reading it is reading what the harness pins — and a metric added there is a metric this
# hook starts insisting on without anybody remembering to edit a script.
mapfile -t metrics < <(
    sed -n "/${begin}/,/${end}/p" "${evidence}" |
        grep '^| ' | tail -n +3 | cut -d'|' -f2 | sed 's/^ *//; s/ *$//'
)
[[ ${#metrics[@]} -gt 0 ]] ||
    fail "${evidence} lists no metrics between its markers, so there is nothing to
  insist was measured."

output=$(mktemp)

# `|| true` so the checks below report the failure with the measured numbers in front of
# it rather than a bare non-zero exit. `--test-threads=1`: every sample starts a
# compositor and, half the time, a sandboxed application, and two of those competing for
# the machine is a slow measurement pretending to be a flaky one.
cargo test --release -p axiomd-app --test parity -- \
    --ignored --nocapture --test-threads=1 2>&1 | tee "${output}" || true

summary=$(grep -E '^test result:' "${output}" || true)
[[ -n "${summary}" ]] ||
    fail "the parity suite did not finish. Its output is above."

grep -qE '^test result: ok\.' <<<"${summary}" ||
    fail "the package is over one of its ceilings. The measured numbers are above;
  raising a ceiling to make this pass is the project owner's decision and nobody
  else's (issue #36)."

# A metric that was filtered out is a metric nobody measured, and the run would otherwise
# report a clean pass of less than the whole table.
for metric in "${metrics[@]}"; do
    grep -qF "parity: ${metric}" "${output}" ||
        fail "${metric}: ${evidence} pins it and this run did not measure it."
done

printf '\nflatpak parity: %d metrics measured on both forms; ceilings only ever come down (issue #36)\n' \
    "${#metrics[@]}"
