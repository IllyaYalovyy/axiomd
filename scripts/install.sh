#!/usr/bin/env bash
#
# Installs a built axiomd into a prefix: the binary, and every file the desktop
# needs to know it exists.
#
# This is the whole of axiomd's build system beyond cargo (RFC-001 Q1, answered
# in issue #14): cargo builds, this installs, and the flatpak manifest calls
# both. It exists because `cargo install` places a binary and nothing else — a
# copy installed that way has no desktop entry, no icon, no AppStream data and,
# worse than any of those, no compiled settings schema, so it starts and then
# dies the moment it reads a setting.
#
# Usage:
#   scripts/install.sh [--prefix DIR] [--destdir DIR] [--binary FILE]
#
#   --prefix   where axiomd will run from (default /usr/local; /app in flatpak)
#   --destdir  a staging root prepended to every path, for packaging
#   --binary   the built binary to install (default target/release/axiomd)

set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)

prefix=/usr/local
destdir=""
binary="${repo_root}/target/release/axiomd"

while [[ $# -gt 0 ]]; do
    case $1 in
        --prefix)
            prefix=$2
            shift 2
            ;;
        --destdir)
            destdir=$2
            shift 2
            ;;
        --binary)
            binary=$2
            shift 2
            ;;
        -h | --help)
            sed -n '3,22p' "${BASH_SOURCE[0]}" | sed 's/^# \?//'
            exit 0
            ;;
        *)
            printf 'install.sh: unknown argument %s\n' "$1" >&2
            exit 2
            ;;
    esac
done

if [[ ! -x ${binary} ]]; then
    printf 'install.sh: no built axiomd at %s.\n  Build one first: cargo build --release\n' \
        "${binary}" >&2
    exit 1
fi

data="${repo_root}/data"
app_id=io.github.etf.axiomd
root="${destdir}${prefix}"

install -Dm755 "${binary}" "${root}/bin/axiomd"
install -Dm644 "${data}/${app_id}.desktop" "${root}/share/applications/${app_id}.desktop"
install -Dm644 "${data}/${app_id}.metainfo.xml" "${root}/share/metainfo/${app_id}.metainfo.xml"
install -Dm644 "${data}/${app_id}.gschema.xml" \
    "${root}/share/glib-2.0/schemas/${app_id}.gschema.xml"
install -Dm644 "${data}/icons/hicolor/scalable/apps/${app_id}.svg" \
    "${root}/share/icons/hicolor/scalable/apps/${app_id}.svg"
install -Dm644 "${data}/icons/hicolor/symbolic/apps/${app_id}-symbolic.svg" \
    "${root}/share/icons/hicolor/symbolic/apps/${app_id}-symbolic.svg"

# The schema is compiled here rather than left to whoever installs, because an
# installed axiomd reads its settings out of this file and an uncompiled schema
# is a crash on the first preference read. Compiling into a staging root is
# harmless: a packager who recompiles overwrites it with the same thing.
if command -v glib-compile-schemas >/dev/null 2>&1; then
    glib-compile-schemas "${root}/share/glib-2.0/schemas"
else
    printf 'install.sh: glib-compile-schemas not found; axiomd cannot read its\n  settings until someone compiles %s\n' \
        "${root}/share/glib-2.0/schemas" >&2
    exit 1
fi

# What makes the desktop offer axiomd for a Markdown file. Absent in a minimal
# build environment (and unnecessary there: flatpak-builder does this itself
# when it exports the entry), so its absence is a note rather than a failure.
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "${root}/share/applications"
fi

printf 'axiomd installed into %s\n' "${root}"
