#!/usr/bin/env bash
#
# What a test run left running in the developer's session (issue #44).
#
# The e2e harness ends every launch it owns, and every launch is started in a process
# group of its own so that ending one ends the tree. That covers the paths the harness
# reaches: a test that finishes, a test that fails, a test that panics. It does not cover
# the paths where nothing of the harness runs at all — a worker killed mid-gate, a script
# that exits on a failure before its own cleanup, a `cargo test` stopped by hand. Those
# are exactly how the owner ended up with axiomd processes they never started, so a run
# is not finished until something has looked.
#
#   scripts/leak-sweep.sh baseline <file> [mark]   # what was already running before a run
#   scripts/leak-sweep.sh sweep <file> [mark]      # what the run added, and is still running
#
# `mark` is what a process has to have in its environment to belong to a run, and is
# every harness scratch directory by default. A narrower one is for the suite that tests
# this sweep: it puts a process of its own in the way and needs the sweep to find that one
# and nothing else, while other suites are running launches of their own beside it.
#
# The sweep ends what it finds and then fails: leaving it running would hand the problem
# back to the person whose session it is, and passing quietly would let the next run add
# to it. A failure here is a defect in whatever started the process, not in the sweep.
#
# # What counts as this harness's
#
# A process belongs to a run when its environment names one of the harness's scratch
# directories — `axiomd-e2e-…`, which every launch is given as its control socket, its
# home, its configuration and its runtime directory, and which every child of a launch
# inherits. Nothing on the machine has that in its environment by accident, so the rule
# names weston, the sandbox, the application and its web processes at once, and can never
# name the reader's own axiomd: theirs has no such variable.

set -euo pipefail

usage() {
    printf 'usage: %s baseline <file> [mark] | sweep <file> [mark]\n' "$0" >&2
    exit 2
}

# Every process of this user whose environment names a harness scratch directory.
#
# `/proc/<pid>/environ` is readable for this user's own processes and unreadable for
# everybody else's, which is exactly the right set: a run can only leak into the session
# it is running in.
ours() {
    local entry pid
    for entry in /proc/[0-9]*; do
        pid=${entry#/proc/}
        if grep -qa -- "${mark}" "${entry}/environ" 2>/dev/null; then
            printf '%s\n' "${pid}"
        fi
    done
}

# What a process was, for a report that can be acted on after it has been ended.
describe() {
    local pid=$1
    local command
    command=$(tr '\0' ' ' <"/proc/${pid}/cmdline" 2>/dev/null || true)
    printf '  %s  %s\n' "${pid}" "${command:-(gone)}"
}

command=${1:-}
file=${2:-}
# The mark of a launch: the prefix `Scratch::new` builds every directory of one from.
mark=${3:-axiomd-e2e-}
[[ -n "${command}" && -n "${file}" ]] || usage

case "${command}" in
baseline)
    ours >"${file}"
    ;;
sweep)
    [[ -f "${file}" ]] || {
        printf 'leak sweep: no baseline was taken, so nothing can be attributed to this run.\n' >&2
        exit 1
    }

    survivors=()
    while IFS= read -r pid; do
        grep -qx -- "${pid}" "${file}" || survivors+=("${pid}")
    done < <(ours)

    if [[ ${#survivors[@]} -eq 0 ]]; then
        printf 'leak sweep: this run left nothing running.\n'
        exit 0
    fi

    printf 'leak sweep: this run left %d process(es) running in this session:\n' \
        "${#survivors[@]}" >&2
    for pid in "${survivors[@]}"; do
        describe "${pid}" >&2
    done

    # Ended rather than reported and left: the session belongs to the person running the
    # gate, and a sweep that only complained would still be handing them the mess.
    kill -TERM "${survivors[@]}" 2>/dev/null || true
    for _ in 1 2 3 4 5 6 7 8 9 10; do
        still=()
        for pid in "${survivors[@]}"; do
            [[ -d "/proc/${pid}" ]] && still+=("${pid}")
        done
        [[ ${#still[@]} -eq 0 ]] && break
        sleep 0.2
    done
    kill -KILL "${survivors[@]}" 2>/dev/null || true

    printf 'They have been ended. Whatever started them has to end them itself: a run\n' >&2
    printf 'that needs this sweep to clean up after it is the defect issue #44 reports.\n' >&2
    exit 1
    ;;
*)
    usage
    ;;
esac
