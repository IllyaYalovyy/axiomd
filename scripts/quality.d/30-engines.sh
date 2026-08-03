#!/usr/bin/env bash
#
# The engine comparison harness's place in the quality gate (issue #17).
#
# Most of the harness is ordinary tests that the gate's own `cargo test --workspace`
# already runs: the capability matrix, both conformance suites, the golden corpus and
# span quality are all deterministic, and `the_committed_report_is_what_this_run_measures`
# fails the gate when `designs/engine-comparison.md` stops matching them.
#
# Parse throughput is the one leg that cannot run there. `cargo test` builds debug,
# where a markdown parser is an order of magnitude off what anybody ships — the same
# reason the perf budgets are `#[ignore]`d (`20-perf.sh`). This hook runs that one test
# in release, one at a time, and shows the numbers, so the throughput column of the
# report is measured rather than remembered.

set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
cd "${repo_root}"

fail() {
    printf 'engine comparison gate check failed: %s\n' "$*" >&2
    exit 1
}

output=$(mktemp)
trap 'rm -f "${output}"' EXIT

# `|| true` so the checks below report the failure with the measured numbers in front
# of it rather than a bare non-zero exit.
cargo test --release -p axiomd-engine --test comparison -- \
    --ignored --nocapture --test-threads=1 2>&1 | tee "${output}" || true

summary=$(grep -E '^test result:' "${output}" || true)
[[ -n "${summary}" ]] ||
    fail "the comparison harness did not finish. Its output is above."

grep -qE '^test result: ok\.' <<<"${summary}" ||
    fail "an engine did not get through the perf fixtures. The output is above."

# A comparison of one engine is not a comparison, and a run that measured nothing
# would otherwise pass quietly: the test itself asserts nothing about the clock.
# Not anchored: the first line is printed on the same line as the test's own name, and
# only the ones after it start at the left margin.
measured=$(grep -c 'engines: ' "${output}" || true)
[[ "${measured}" -ge 6 ]] ||
    fail "only ${measured} throughput line(s) printed. Every engine is measured on
  every perf fixture, so anything less means a fixture or an engine was skipped."

printf '\nengines: throughput measured in release; the rest of the comparison is in the gate\n'
