#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'EOF'
Usage:
  ./scripts/init-project.sh --name NAME --slug SLUG --description DESCRIPTION --author AUTHOR --email EMAIL [--branch BRANCH]

Example:
  ./scripts/init-project.sh \
    --name "Example App" \
    --slug "example-app" \
    --description "A short project description." \
    --author "Illya Yalovyy" \
    --email "yalovoy@gmail.com"
EOF
}

project_name=""
project_slug=""
project_description=""
author_name=""
author_email=""
default_branch="main"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --name)
            project_name=${2:-}
            shift 2
            ;;
        --slug)
            project_slug=${2:-}
            shift 2
            ;;
        --description)
            project_description=${2:-}
            shift 2
            ;;
        --author)
            author_name=${2:-}
            shift 2
            ;;
        --email)
            author_email=${2:-}
            shift 2
            ;;
        --branch)
            default_branch=${2:-}
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            printf 'Unknown argument: %s\n\n' "$1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

if [[ -z "${project_name}" || -z "${project_slug}" || -z "${project_description}" || -z "${author_name}" || -z "${author_email}" ]]; then
    usage >&2
    exit 2
fi

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "${repo_root}"

replace_in_file() {
    local file=$1
    python3 - "$file" "$project_name" "$project_slug" "$project_description" "$author_name" "$author_email" "$default_branch" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
replacements = {
    "__PROJECT_NAME__": sys.argv[2],
    "__PROJECT_SLUG__": sys.argv[3],
    "__PROJECT_DESCRIPTION__": sys.argv[4],
    "__AUTHOR_NAME__": sys.argv[5],
    "__AUTHOR_EMAIL__": sys.argv[6],
    "__DEFAULT_BRANCH__": sys.argv[7],
}

text = path.read_text()
for old, new in replacements.items():
    text = text.replace(old, new)
path.write_text(text)
PY
}

while IFS= read -r file; do
    replace_in_file "$file"
done < <(find . -type f \
    -not -path './.git/*' \
    -not -path './node_modules/*' \
    -not -path './target/*' \
    -not -path './build/*' \
    -not -path './scripts/init-project.sh' \
    -print)

if [[ -d .git ]]; then
    git config user.name "${author_name}"
    git config user.email "${author_email}"
    ./scripts/install-git-hooks.sh
fi

./scripts/install-agent-files.sh

printf 'Initialized %s (%s).\n' "${project_name}" "${project_slug}"
printf 'Review README.md, docs/PROCESS.md, docs/TESTING.md, and AGENTS.md next.\n'
