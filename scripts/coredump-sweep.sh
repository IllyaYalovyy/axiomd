#!/usr/bin/env bash
#
# What a gate run crashed, whatever the tests said about it (issue #45).
#
# The harness fails the test whose application dies, which covers every death a test is
# there to see. It does not cover the deaths nothing is watching: a window's web process
# going down after the test that opened it has finished, an application killed by a
# signal on a path no assertion follows, a crash inside a hook that shells out rather
# than driving a launch. On 2026-08-05 eleven core dumps of axiomd were written in a
# single day and every gate over them reported success, so a run is not green until
# something has looked at what it dumped.
#
#   scripts/coredump-sweep.sh baseline <file> [pattern]   # the moment the run started
#   scripts/coredump-sweep.sh sweep    <file> [pattern]   # what it has dumped since
#
# `pattern` is an extended regular expression matched against a dump's executable, and
# is `/axiomd$` by default: every axiomd however it was launched — the build tree's, an
# installed prefix's, the package's inside its sandbox — and nothing else. A narrower one
# is for the suite that tests this sweep, which plants a dump of its own and needs to be
# told about that one and no others while real suites run beside it.
#
# # Why not everything that dumped
#
# WebKit's web process dumps core on this machine routinely and in numbers, under axiomd
# and under every other WebKitGTK application — thirty-odd on the day issue #45 was filed,
# against eleven of ours. Those are upstream's (`docs/CRASHES.md` records what was found
# in them and where the evidence for that is), and a gate that failed on them would fail
# always, which is the same as a gate that fails never. What this sweep is for is the
# application this project builds: it must not crash, and here it cannot crash quietly.
#
# A dump whose core was not stored still counts. `coredumpctl` records the death whether
# or not the core survived it — a core limit of zero, a full disk, a vacuum — and the
# death is the defect. The core is only how it gets diagnosed.

set -euo pipefail

usage() {
    printf 'usage: %s baseline <file> [pattern] | sweep <file> [pattern]\n' "$0" >&2
    exit 2
}

command=${1:-}
file=${2:-}
# Every axiomd, and only axiomd: `coredumpctl` records the path the dead process was
# started from, which ends in the binary's own name for each of the ways one is launched.
pattern=${3:-/axiomd$}
[[ -n "${command}" && -n "${file}" ]] || usage

# The dumps of this user matching `pattern` that were recorded after `since` — a
# microsecond stamp — one line each, as `<time> <pid> <signal> <executable>`.
#
# Read as JSON rather than off the table: the table's first column is a date with spaces
# in it, and a sweep that mis-parses its way to an empty answer is a sweep that passes
# everything.
dumps_since() {
    local since=$1
    coredumpctl list --no-legend --json=short 2>/dev/null |
        python3 -c '
import json, re, sys

since = int(sys.argv[1])
wanted = re.compile(sys.argv[2])
try:
    dumps = json.load(sys.stdin)
except (json.JSONDecodeError, ValueError):
    # No dumps at all on this machine: `coredumpctl` says so on stderr and prints
    # nothing, which is an empty answer rather than a broken one.
    dumps = []
for dump in dumps:
    if dump.get("time", 0) <= since:
        continue
    exe = dump.get("exe") or ""
    if not wanted.search(exe):
        continue
    print(dump.get("time"), dump.get("pid"), dump.get("sig"), exe)
' "${since}" "${pattern}"
}

# The top of the stack the process died on, which is what a reader needs before they
# decide whether to go and fetch the core itself.
summarise() {
    local pid=$1
    coredumpctl info "${pid}" 2>/dev/null |
        awk -v thread="Stack trace of thread ${pid}:" '
            $0 ~ thread { inside = 1 }
            inside && /^ *$/ { exit }
            inside && shown < 9 { sub(/^ */, "      "); print; shown++ }
        ' || true
}

case "${command}" in
baseline)
    # The moment the run began, to the microsecond `coredumpctl` stamps a dump with, so
    # that a crash from before it can never be blamed on this run and one from during it
    # can never be missed.
    date +%s%6N >"${file}"
    ;;
sweep)
    [[ -f "${file}" ]] || {
        printf 'coredump sweep: no baseline was taken, so nothing can be attributed to this run.\n' >&2
        exit 1
    }
    since=$(<"${file}")

    found=0
    while IFS=' ' read -r when pid signal exe; do
        [[ -n "${pid}" ]] || continue
        if [[ ${found} -eq 0 ]]; then
            printf 'coredump sweep: this run dumped core.\n' >&2
        fi
        found=$((found + 1))
        printf '  %s crashed (pid %s, signal %s)\n' "${exe}" "${pid}" "${signal}" >&2
        summarise "${pid}" >&2
        printf '    the whole of it: coredumpctl info %s\n' "${pid}" >&2
    done < <(dumps_since "${since}")

    if [[ ${found} -eq 0 ]]; then
        printf 'coredump sweep: this run dumped no core.\n'
        exit 0
    fi

    printf 'A crash under test is a failed run whatever the tests said: %d dump(s).\n' \
        "${found}" >&2
    exit 1
    ;;
*)
    usage
    ;;
esac
