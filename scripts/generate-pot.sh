#!/usr/bin/env bash
#
# Collects every word axiomd says to its reader into the template translators work
# from — `po/axiomd.pot`.
#
# The template is generated and never committed: it is a view of the source at one
# moment, and a stale copy of it in the repository is worse than none, because a
# translator would work from words the application no longer says. `xgettext` is what
# reads the source; this script is only the flags it has to be read with, in one place
# so that the extraction a translator gets and the extraction the gate checks cannot be
# two different things.
#
# What it reads is `po/POTFILES.in`. `crates/axiomd-i18n/tests/catalog.rs` holds that
# list to the files that really carry a word for the reader, so a new screen of words
# cannot be added with no way for a translator to see it.
#
# Usage:
#   scripts/generate-pot.sh [--files-from FILE] [--output FILE]

set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "${repo_root}"

files_from="po/POTFILES.in"
output="po/axiomd.pot"

while [[ $# -gt 0 ]]; do
    case $1 in
        --files-from)
            files_from=$2
            shift 2
            ;;
        --output)
            output=$2
            shift 2
            ;;
        -h | --help)
            sed -n '3,18p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            printf 'generate-pot.sh: unknown argument %s\n' "$1" >&2
            exit 2
            ;;
    esac
done

if ! command -v xgettext >/dev/null 2>&1; then
    printf 'generate-pot.sh: xgettext is what reads the words out of the source, and it
  is not here (Fedora: sudo dnf install gettext, Debian: sudo apt install gettext).\n' >&2
    exit 1
fi

mkdir -p "$(dirname -- "${output}")"

# No `--language`: the desktop entry, the AppStream metainfo and the Rust source are
# three different formats, and xgettext picks each by its name — including the ITS
# rules that say which AppStream elements are prose and which are identifiers.
#
# `--keyword=gettext_noop` on top of the defaults: `xgettext` already knows `gettext`,
# `pgettext` and `ngettext`, and this is the fourth call `axiomd-i18n` offers — the mark
# on a literal in a `const` table, which is translated later and elsewhere.
#
# `--from-code=UTF-8`: axiomd's own words are full of em dashes and ellipses, and
# without this xgettext refuses to read a source file that is not ASCII.
xgettext \
    --files-from="${files_from}" \
    --directory=. \
    --from-code=UTF-8 \
    --keyword=gettext_noop \
    --add-comments=TRANSLATORS \
    --sort-by-file \
    --package-name=axiomd \
    --package-version="$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -1)" \
    --copyright-holder='Illya Yalovyy' \
    --msgid-bugs-address='https://github.com/IllyaYalovyy/axiomd/issues' \
    --output="${output}"

# One `msgid ""` at the top is the header rather than a message, so it is not counted.
printf '%s holds %d messages for translators\n' \
    "${output}" "$(($(grep -c '^msgid ' "${output}") - 1))"
