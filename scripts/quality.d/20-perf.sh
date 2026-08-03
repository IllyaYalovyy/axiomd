#!/usr/bin/env bash
#
# The performance budgets' place in the quality gate (issue #9, invariant 8).
#
# The budgets live in `crates/axiomd-app/tests/perf.rs` and are ordinary tests, which
# is what keeps them under `cargo clippy --all-targets`. They are `#[ignore]`d, and
# this hook is why: the gate's own test step builds debug, where comrak, syntect and
# ammonia are an order of magnitude off what anybody ships, so a number measured there
# is not a number. This hook runs the same target in release, where it means something.
#
# What it guarantees beyond running them:
#
#   1. the numbers reach the person who ran the gate — `--nocapture`, because a budget
#      whose measurement is swallowed cannot be ratcheted;
#   2. no two budgets are measured while competing for the machine — `--test-threads=1`;
#   3. every budget in the target really ran, rather than being quietly filtered out.
#
# The ten-megabyte budgets take minutes rather than seconds and are left out by
# default; they say so in the output rather than passing silently. Ask for the whole
# picture with:
#
#   AXIOMD_PERF_SOAK=1 ./scripts/quality.d/20-perf.sh

set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
cd "${repo_root}"

fail() {
    printf 'perf gate check failed: %s\n' "$*" >&2
    exit 1
}

command -v weston >/dev/null 2>&1 ||
    fail "the perf budgets drive the real application on a headless compositor.
  Install it with: sudo dnf install weston"

output=$(mktemp)
trap 'rm -f "${output}"' EXIT

# `|| true` so the summary below is what reports a failure, with the measured numbers
# in front of it rather than a bare non-zero exit.
cargo test --release -p axiomd-app --test perf -- \
    --ignored --nocapture --test-threads=1 2>&1 | tee "${output}" || true

# Not anchored: the first budget of a test is printed on the same line as the test's
# own name, and only a second one in the same test starts at the left margin.
grep -q 'perf: ' "${output}" ||
    fail "no budget printed a number. Every budget prints one whether it passes or
  fails, so an empty run means none of them ran."

# A budget that was filtered out is a budget nobody measured, and a run that reported
# no result at all is a run that died before the summary.
summary=$(grep -E '^test result:' "${output}" || true)
[[ -n "${summary}" ]] ||
    fail "the perf suite did not finish. Its output is above."

grep -qE '^test result: ok\.' <<<"${summary}" ||
    fail "a budget is not met. The measured numbers are above; raising a ceiling to
  make this pass is the project owner's decision and nobody else's (issue #9)."

grep -qE '0 filtered out' <<<"${summary}" ||
    fail "some budgets were filtered out of this run: ${summary}
  Every test in the perf target is a budget and every one of them must be measured."

printf '\nperf: budgets measured in release; ceilings only ever come down (issue #9)\n'
