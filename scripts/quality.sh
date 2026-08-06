#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "${repo_root}"

log() {
    printf '\n==> %s\n' "$*"
}

has_command() {
    command -v "$1" >/dev/null 2>&1
}

json_has_script() {
    local script_name=$1
    python3 - "$script_name" <<'PY'
import json
import sys
from pathlib import Path

script = sys.argv[1]
path = Path("package.json")
if not path.exists():
    raise SystemExit(1)

try:
    data = json.loads(path.read_text())
except json.JSONDecodeError:
    raise SystemExit(1)

raise SystemExit(0 if script in data.get("scripts", {}) else 1)
PY
}

run_shell_syntax_checks() {
    if compgen -G "scripts/*.sh" >/dev/null; then
        log "Checking shell script syntax"
        bash -n scripts/*.sh
    fi
}

run_rust_checks() {
    if [[ ! -f Cargo.toml ]]; then
        return
    fi

    if ! has_command cargo; then
        log "Skipping Rust checks: cargo not found"
        return
    fi

    log "Rust format"
    cargo fmt --check

    # --workspace: at a workspace root with a root package, bare cargo runs
    # only that package and silently skips every other member.
    # --all-targets: without it clippy never compiles #[cfg(test)] code — a
    # gate missing this flag once merged broken test code green, twice.
    log "Rust clippy"
    cargo clippy --workspace --all-targets --all-features -- -D warnings

    log "Rust tests"
    cargo test --workspace --all-targets --all-features
}

run_node_checks() {
    if [[ ! -f package.json ]]; then
        return
    fi

    if ! has_command npm; then
        log "Skipping Node checks: npm not found"
        return
    fi

    if [[ -f package-lock.json && ! -d node_modules ]]; then
        log "Installing Node dependencies"
        npm ci
    fi

    if json_has_script lint; then
        log "Node lint"
        npm run lint
    fi

    if json_has_script test; then
        log "Node tests"
        npm test
    fi

    if json_has_script build; then
        log "Node build"
        npm run build
    fi
}

run_python_checks() {
    if [[ ! -d tests && ! -d test ]]; then
        return
    fi

    if ! has_command python3; then
        log "Skipping Python checks: python3 not found"
        return
    fi

    if python3 -m pytest --version >/dev/null 2>&1; then
        log "Python tests"
        python3 -m pytest
    else
        log "Skipping Python tests: pytest not found"
    fi
}

run_project_hooks() {
    if [[ ! -d scripts/quality.d ]]; then
        return
    fi

    local hook
    local found=0
    # -u+x, not -111: git records only the owner's execute bit (mode 100755), and a
    # checkout under the usual umask 027 lands as rwxr-x---. Requiring all three
    # execute bits made every project hook invisible on a normal clone — and
    # invisible silently, which is the worst way for a gate check to be missing.
    while IFS= read -r hook; do
        found=1
        log "Project quality hook: ${hook}"
        "${hook}"
    done < <(find scripts/quality.d -maxdepth 1 -type f -perm -u+x | sort)

    # A hook directory holding nothing runnable means checks that are meant to be
    # part of the gate are not in it. Say so rather than pass quietly.
    if [[ ${found} -eq 0 ]] && compgen -G "scripts/quality.d/*" >/dev/null; then
        printf 'scripts/quality.d holds files but none are executable; no project hook ran.\n' >&2
        return 1
    fi
}

# Nothing this run starts may still be running when it ends (issue #44): not an
# application, not a compositor, not a sandbox. The baseline is taken before anything
# runs so that only what *this* run added is ever blamed on it, and the sweep is a trap
# rather than a last line, so a gate that fails half way through — or is killed — still
# ends what it started. See scripts/leak-sweep.sh.
leak_baseline=$(mktemp)
scripts/leak-sweep.sh baseline "${leak_baseline}"
sweep_before_exiting() {
    local status=$?
    if ! scripts/leak-sweep.sh sweep "${leak_baseline}"; then
        status=1
    fi
    rm -f "${leak_baseline}"
    exit "${status}"
}
trap sweep_before_exiting EXIT

run_shell_syntax_checks
run_rust_checks
run_node_checks
run_python_checks
run_project_hooks

log "Quality gate passed"
