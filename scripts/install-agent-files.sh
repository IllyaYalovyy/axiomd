#!/usr/bin/env bash
# Install agent files from the tracked agent/ directory into the local,
# gitignored .claude/ directory. agent/ is the source of truth; re-run this
# after editing anything under it.
set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "${repo_root}"

if [[ ! -d agent/skills ]]; then
    printf 'No agent/skills directory; nothing to install.\n' >&2
    exit 0
fi

installed=0
while IFS= read -r skill_file; do
    rel=${skill_file#agent/skills/}
    dest=".claude/skills/${rel}"
    mkdir -p "$(dirname -- "${dest}")"
    cp "${skill_file}" "${dest}"
    installed=$((installed + 1))
done < <(find agent/skills -mindepth 2 -name 'SKILL.md' -type f | sort)

# ktask prompt/context: agent/ktask/ is the tracked source of truth; the
# gitignored .ktask/ copies are overwritten on install. Local-only state
# (tasks.md, config.toml, logs/, queue/) is never touched.
if [[ -d agent/ktask && -d .ktask ]]; then
    for f in agent/ktask/prompt.md agent/ktask/context.md; do
        if [[ -f "$f" ]]; then
            cp "$f" ".ktask/$(basename -- "$f")"
            printf 'Installed .ktask/%s\n' "$(basename -- "$f")"
        fi
    done
fi

# Claude Code settings: install only if absent — never clobber local tweaks.
if [[ -f agent/claude-settings.json && ! -f .claude/settings.json ]]; then
    mkdir -p .claude
    cp agent/claude-settings.json .claude/settings.json
    printf 'Installed .claude/settings.json\n'
fi

printf 'Installed %d skill(s) into .claude/skills/\n' "${installed}"
