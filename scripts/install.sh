#!/usr/bin/env bash
#
# Installs a built axiomd into a prefix — and takes it back out again: the binary,
# and every file the desktop needs to know it exists.
#
# This is the whole of axiomd's build system beyond cargo (RFC-001 Q1, answered
# in issue #14): cargo builds, this installs, and the flatpak manifest calls
# both. It exists because `cargo install` places a binary and nothing else — a
# copy installed that way has no desktop entry, no icon, no AppStream data and,
# worse than any of those, no compiled settings schema, so it starts and then
# dies the moment it reads a setting.
#
# A per-user install (`--user`) is the recommended way to run axiomd
# (design_decisions.md, owner ruling 2026-08-03, issue #25): it needs no root, it
# writes nothing outside the reader's own home, and the desktop picks it up from
# there — `~/.local/share` is a schema, application and icon directory the
# desktop reads exactly as it reads the system's.
#
# Usage:
#   scripts/install.sh [--user | --prefix DIR] [--destdir DIR] [--binary FILE]
#   scripts/install.sh --uninstall [--user | --prefix DIR] [--destdir DIR]
#
# Translations (issue #34): a language `po/LINGUAS` names is compiled and installed here
# too — the message catalogue the application reads, and the same words merged into the
# desktop entry the shell draws and the AppStream data a software centre shows. With no
# translations in the repository this does nothing at all, which is where axiomd is
# today.

set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)

app_id=io.github.etf.axiomd

usage() {
    cat <<'USAGE'
Installs a built axiomd into a prefix, or removes what was installed.

Usage:
  scripts/install.sh [--user | --prefix DIR] [--destdir DIR] [--binary FILE]
  scripts/install.sh --uninstall [--user | --prefix DIR] [--destdir DIR]

  --user       install into ~/.local, for this user alone and without root.
               This is the recommended way to run axiomd.
  --prefix     where axiomd will run from (default /usr/local; /app in flatpak)
  --destdir    a staging root prepended to every path, for packaging
  --binary     the built binary to install (default target/release/axiomd)
  --uninstall  remove exactly the files an install of the same prefix wrote,
               and rebuild the desktop's caches around what is left
USAGE
}

prefix=""
destdir=""
binary="${repo_root}/target/release/axiomd"
user_install=false
uninstalling=false

while [[ $# -gt 0 ]]; do
    case $1 in
        --user)
            user_install=true
            shift
            ;;
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
        --uninstall)
            uninstalling=true
            shift
            ;;
        -h | --help)
            usage
            exit 0
            ;;
        *)
            printf 'install.sh: unknown argument %s\n' "$1" >&2
            exit 2
            ;;
    esac
done

# Two ways of naming the same thing, and no way to tell which one was meant.
if [[ ${user_install} == true && -n ${prefix} ]]; then
    printf 'install.sh: --user and --prefix name the same thing; pass one of them\n' >&2
    exit 2
fi

if [[ ${user_install} == true ]]; then
    if [[ -z ${HOME:-} ]]; then
        printf 'install.sh: --user installs into ~/.local and there is no HOME to find\n' >&2
        exit 2
    fi
    prefix="${HOME}/.local"
fi
prefix=${prefix:-/usr/local}

data="${repo_root}/data"
root="${destdir}${prefix}"

# What an installed axiomd is, in one place: the file in the repository, where it goes
# under the prefix, and the mode it lands with. Installing writes exactly these and
# uninstalling removes exactly these, so the two directions cannot drift apart.
payload=(
    "${binary}|bin/axiomd|755"
    "${data}/${app_id}.desktop|share/applications/${app_id}.desktop|644"
    "${data}/${app_id}.metainfo.xml|share/metainfo/${app_id}.metainfo.xml|644"
    "${data}/${app_id}.gschema.xml|share/glib-2.0/schemas/${app_id}.gschema.xml|644"
    "${data}/icons/hicolor/scalable/apps/${app_id}.svg|share/icons/hicolor/scalable/apps/${app_id}.svg|644"
    "${data}/icons/hicolor/symbolic/apps/${app_id}-symbolic.svg|share/icons/hicolor/symbolic/apps/${app_id}-symbolic.svg|644"
)

po="${repo_root}/po"

# The languages axiomd is translated into: what `po/LINGUAS` names, comments and blank
# lines aside.
#
# Empty is the normal state today (issue #34 landed the machinery and no translation),
# and every step below is written so that empty means "install exactly what axiomd
# installed before any of this existed".
languages() {
    local code
    [[ -f ${po}/LINGUAS ]] || return 0
    while read -r code; do
        code=${code%%#*}
        code=${code//[[:space:]]/}
        if [[ -n ${code} ]]; then
            printf '%s\n' "${code}"
        fi
    done <"${po}/LINGUAS"
}

# A language named with no `po/<code>.po` beside it stops the install before it starts.
#
# Not skipped: `msgfmt` reads `LINGUAS` itself when it merges the desktop entry and the
# metainfo, so a language this script passed over would fail there anyway — halfway
# through, with half an axiomd installed. And it is a mistake in the repository rather
# than anything about the machine installing it, so it is worth saying plainly.
check_catalogs() {
    local code missing=()
    while read -r code; do
        [[ -f ${po}/${code}.po ]] || missing+=("${code}")
    done < <(languages)

    if [[ ${#missing[@]} -gt 0 ]]; then
        printf 'install.sh: po/LINGUAS names %s and there is no po/%s.po for it.\n  Either add the translation or take the language out of po/LINGUAS.\n' \
            "${missing[*]}" "${missing[0]}" >&2
        exit 1
    fi
}

installed_paths() {
    local entry destination code
    for entry in "${payload[@]}"; do
        destination=${entry#*|}
        printf '%s\n' "${root}/${destination%%|*}"
    done
    while read -r code; do
        printf '%s\n' "${root}/share/locale/${code}/LC_MESSAGES/axiomd.mo"
    done < <(languages)
}

has_command() {
    command -v "$1" >/dev/null 2>&1
}

# The reader's own language, in the three places axiomd is read in it: the application's
# message catalogue, the desktop entry the shell draws before axiomd is started, and the
# AppStream description a software centre shows before it is installed.
#
# `msgfmt` compiles all three — `--desktop` and `--xml` merge the translations *into* the
# files already installed above, so the entry and the metainfo in `data/` stay the whole
# truth about axiomd and gain their translations here rather than being generated
# somewhere else. A repository with no translations does none of this and leaves the
# untranslated originals exactly as they were.
install_catalogs() {
    local code any=false
    while read -r code; do
        any=true
        if ! has_command msgfmt; then
            printf 'install.sh: this repository has translations (po/%s.po) and msgfmt is
  not here to compile them (Fedora: sudo dnf install gettext,
  Debian: sudo apt install gettext)\n' "${code}" >&2
            exit 1
        fi
        install -d "${root}/share/locale/${code}/LC_MESSAGES"
        msgfmt "${po}/${code}.po" \
            --output-file="${root}/share/locale/${code}/LC_MESSAGES/axiomd.mo"
    done < <(languages)

    if [[ ${any} == false ]]; then
        return 0
    fi

    msgfmt --desktop --template="${data}/${app_id}.desktop" -d "${po}" \
        --output-file="${root}/share/applications/${app_id}.desktop"
    chmod 644 "${root}/share/applications/${app_id}.desktop"
    msgfmt --xml --template="${data}/${app_id}.metainfo.xml" -d "${po}" \
        --output-file="${root}/share/metainfo/${app_id}.metainfo.xml"
    chmod 644 "${root}/share/metainfo/${app_id}.metainfo.xml"
}

# The icon cache is written by whichever of GTK's two cache builders is here; a machine
# with neither still shows the icon, because GTK reads the directories themselves when
# there is no cache to read. So its absence is a note, not a failure.
icon_cache_builder() {
    local builder
    for builder in gtk4-update-icon-cache gtk-update-icon-cache; do
        if has_command "${builder}"; then
            printf '%s\n' "${builder}"
            return 0
        fi
    done
    return 1
}

# Everything the desktop reads *about* the installed files rather than from them: the
# compiled schemas, the MIME cache the file manager answers "what opens this" from, and
# the icon cache the shell draws from.
#
# One function for both directions, because the question is the same either way — what
# should these caches say about the directory as it is now? After an install that is
# "axiomd is here"; after an uninstall it is "axiomd is gone", and the caches of a
# directory somebody else's application still lives in must survive saying so.
refresh_desktop_caches() {
    local schemas="${root}/share/glib-2.0/schemas"
    if compgen -G "${schemas}/*.gschema.xml" >/dev/null; then
        # The schema is compiled here rather than left to whoever installs, because an
        # installed axiomd reads its settings out of this file and an uncompiled schema
        # is a crash on the first preference read. Compiling into a staging root is
        # harmless: a packager who recompiles overwrites it with the same thing.
        if ! has_command glib-compile-schemas; then
            printf 'install.sh: glib-compile-schemas not found; axiomd cannot read its\n  settings until someone compiles %s\n' \
                "${schemas}" >&2
            exit 1
        fi
        glib-compile-schemas "${schemas}"
    else
        # Nothing left to compile: a compiled file naming a schema that is no longer
        # installed is worse than none at all.
        rm -f "${schemas}/gschemas.compiled"
    fi

    local applications="${root}/share/applications"
    if compgen -G "${applications}/*.desktop" >/dev/null; then
        # What makes the desktop offer axiomd for a Markdown file. Absent in a minimal
        # build environment (and unnecessary there: flatpak-builder does this itself
        # when it exports the entry), so its absence is a note rather than a failure.
        if has_command update-desktop-database; then
            update-desktop-database "${applications}"
        fi
    else
        rm -f "${applications}/mimeinfo.cache"
    fi

    local icons="${root}/share/icons/hicolor"
    local builder
    if compgen -G "${icons}/*/apps/*" >/dev/null; then
        if builder=$(icon_cache_builder); then
            # `-t`: hicolor under a user prefix has no index.theme of its own, and
            # without this the builder refuses the directory (probed with
            # gtk4-update-icon-cache 4.20.4).
            "${builder}" --force --ignore-theme-index "${icons}"
        fi
    else
        rm -f "${icons}/icon-theme.cache"
    fi
}

# The directories an install creates, deepest first — every one of them a directory the
# desktop reads and other applications live in, so each is removed only if this
# uninstall has left it empty. `rmdir` refuses a directory that is not, which is exactly
# the wanted answer and not an error.
prune_empty_directories() {
    local relative code
    # A language's own two directories first: they sit under `share/locale`, which other
    # applications' catalogues live in and which therefore survives unless axiomd was the
    # last thing in it.
    while read -r code; do
        rmdir "${root}/share/locale/${code}/LC_MESSAGES" 2>/dev/null || true
        rmdir "${root}/share/locale/${code}" 2>/dev/null || true
    done < <(languages)
    for relative in \
        bin \
        share/applications \
        share/metainfo \
        share/locale \
        share/glib-2.0/schemas share/glib-2.0 \
        share/icons/hicolor/scalable/apps share/icons/hicolor/scalable \
        share/icons/hicolor/symbolic/apps share/icons/hicolor/symbolic \
        share/icons/hicolor share/icons \
        share; do
        rmdir "${root}/${relative}" 2>/dev/null || true
    done
}

uninstall() {
    local path
    local removed=0
    while IFS= read -r path; do
        if [[ -e ${path} ]]; then
            removed=$((removed + 1))
        fi
        rm -f "${path}"
    done < <(installed_paths)

    refresh_desktop_caches
    prune_empty_directories

    if [[ ${removed} -eq 0 ]]; then
        printf 'axiomd was not installed in %s; nothing to remove\n' "${root}"
    else
        printf 'axiomd removed from %s\n' "${root}"
    fi
}

install_files() {
    if [[ ! -x ${binary} ]]; then
        printf 'install.sh: no built axiomd at %s.\n  Build one first: cargo build --release\n' \
            "${binary}" >&2
        exit 1
    fi

    # Before a single file is written: a half-installed axiomd is worse than an
    # uninstalled one.
    check_catalogs

    local entry source destination mode
    for entry in "${payload[@]}"; do
        source=${entry%%|*}
        destination=${entry#*|}
        mode=${destination#*|}
        destination=${destination%%|*}
        install -Dm"${mode}" "${source}" "${root}/${destination}"
    done

    # Before the entry is rewritten below, because this is what writes the entry the
    # reader's desktop draws when their language is one axiomd has been translated into.
    install_catalogs

    # A per-user prefix's `bin` is not reliably on the session's PATH, and a desktop
    # entry whose Exec is a bare command is a launcher that does nothing at all when it
    # is not. So the entry a user install writes names the binary beside it — the app
    # grid, the file manager and `gtk-launch` all start it whatever PATH says. The
    # system and flatpak prefixes keep the shipped `Exec=axiomd`, which is on every
    # PATH there is (and which flatpak rewrites for its own sandbox when it exports the
    # entry).
    if [[ ${user_install} == true ]]; then
        sed -Ei "s|^Exec=axiomd( \|\$)|Exec=${prefix}/bin/axiomd\1|" \
            "${root}/share/applications/${app_id}.desktop"
    fi

    refresh_desktop_caches

    printf 'axiomd installed into %s\n' "${root}"

    # Said only when it is true, and said with the line to paste: the desktop entry
    # works either way, but `axiomd` typed in a terminal does not.
    if [[ ${user_install} == true && ":${PATH}:" != *":${prefix}/bin:"* ]]; then
        printf '\n%s is not on your PATH, so `axiomd` typed in a terminal will not be\nfound (the app grid and the file manager start it regardless). Add it with:\n    export PATH="%s/bin:$PATH"\n' \
            "${prefix}/bin" '$HOME/.local'
    fi
}

if [[ ${uninstalling} == true ]]; then
    uninstall
else
    install_files
fi
