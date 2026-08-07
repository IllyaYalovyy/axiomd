#!/usr/bin/env bash
#
# The e2e suite's place in the quality gate, and the golden contract around it.
#
# The suite itself is run by the gate's own `cargo test --workspace --all-targets`
# — `crates/axiomd-app/tests/e2e.rs` is an ordinary integration test target, which
# is also what keeps it under `cargo clippy --all-targets`. This hook exists for
# the three things that run cannot check about itself:
#
#   1. that nobody is re-pinning screenshot goldens from inside a gate run,
#   2. that the headless compositor the suite needs is actually here, so a green
#      gate can never mean "the e2e suite quietly had nowhere to run",
#   3. that the e2e target is still part of what the gate runs at all.
#
# and for the one thing worth checking after it: that no approved picture moved.

set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
cd "${repo_root}"

goldens="crates/axiomd-e2e/tests/goldens"

fail() {
    printf 'e2e gate check failed: %s\n' "$*" >&2
    exit 1
}

# 1. Pinning a screenshot golden is a human act, reviewed once, and never a side
#    effect of running the gate. This is the same contract as the gate's skip
#    variables: it belongs to the person looking at the picture. An agent may not
#    set it, and with it set the gate does not run at all — so a failing visual
#    test can never be resolved by the machinery that failed it.
if [[ -n "${AXIOMD_PIN_GOLDENS:-}" ]]; then
    fail "AXIOMD_PIN_GOLDENS is set. Pinning a golden is a human decision made by
  looking at the picture, not something a gate run may do as a side effect.
  Unset it and run the gate again; to pin, run the golden test alone."
fi

# 2. Without a compositor every e2e test fails loudly rather than silently — but
#    saying so here names the missing package instead of making the reader work
#    it out from a panic inside a test.
command -v weston >/dev/null 2>&1 ||
    fail "the e2e suite needs the headless compositor it drives the app on.
  Install it with: sudo dnf install weston"

# 3. The suite is only a gate if the gate still runs it. `--no-run` reuses what
#    the test step already built, so this costs a cargo no-op rather than a
#    second run of the suite.
#
#    The list is taken whole and searched afterwards, the way every other hook
#    here does it. Piped straight into `grep -q` it failed at random: `grep -q`
#    stops at the first match, cargo goes on writing into a closed pipe, and
#    `set -o pipefail` reports the SIGPIPE that follows as the pipeline's status
#    — so the check said the e2e target had been removed on runs where it was
#    right there in the list. Whether it flaked came down to how much cargo had
#    left to say, which is to say, to how many test targets the workspace has.
targets=$(cargo test --workspace --all-targets --all-features --no-run --message-format=short 2>&1)
grep -q 'tests/e2e.rs' <<<"${targets}" ||
    fail "the e2e target is no longer part of \`cargo test --workspace --all-targets\`.
  It must stay an ordinary test target: that is what runs it in the gate and what
  keeps it under clippy."

# 4. Whatever happened during this run, the approved pictures must be exactly the
#    ones that were approved. This catches a re-pin however it was reached —
#    including one that happened earlier in the gate than this hook runs.
if [[ -d "${goldens}" ]] && git rev-parse --git-dir >/dev/null 2>&1; then
    moved=$(git status --porcelain -- "${goldens}")
    if [[ -n "${moved}" ]]; then
        fail "pinned screenshot goldens changed during this run:
${moved}
  A golden only changes when a human approves the new picture and commits it."
    fi
fi

printf 'e2e: harness contract intact (goldens pinned, compositor present, suite wired in)\n'
