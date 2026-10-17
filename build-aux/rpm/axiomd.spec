# axiomd, packaged for Fedora (issue #61).
#
# The RPM is the owner's own daily channel: a native install is the recommended way
# to run axiomd (design_decisions.md, owner ruling 2026-08-03), and this is that
# install with the distribution doing the placing.
#
# One source of truth about what an installed axiomd *is*: `scripts/install.sh`
# writes the binary, the desktop entry, the metainfo, the settings schema, the icons
# and any message catalogues, and %install calls it rather than repeating the list
# here (where the two would drift). The caches that script refreshes for a live
# prefix are deleted again below — in a package they belong to Fedora's own file
# triggers, which run after every transaction rather than once at build time.
#
# This spec builds with cargo reaching crates.io, which is how it is built on the
# release machine. A mock or COPR build has no network and needs the dependencies
# vendored into a second source tarball first; that is a separate piece of work and
# is NOT what this file does today.
%global appid io.github.etf.axiomd

Name:           axiomd
Version:        0.9.2
Release:        1%{?dist}
Summary:        Read and write Markdown documents

License:        GPL-3.0-or-later
URL:            https://github.com/IllyaYalovyy/axiomd
Source0:        %{url}/archive/refs/tags/v%{version}.tar.gz#/%{name}-%{version}.tar.gz

ExclusiveArch:  %{rust_arches}

BuildRequires:  cargo >= 1.92
BuildRequires:  rust >= 1.92
BuildRequires:  gcc
BuildRequires:  pkgconfig(gtk4) >= 4.20
BuildRequires:  pkgconfig(libadwaita-1) >= 1.8
BuildRequires:  pkgconfig(webkitgtk-6.0)
BuildRequires:  pkgconfig(gtksourceview-5)
BuildRequires:  pkgconfig(libspelling-1)
BuildRequires:  pkgconfig(glib-2.0)
BuildRequires:  gettext
BuildRequires:  desktop-file-utils
BuildRequires:  appstream
BuildRequires:  glib2-devel

# The icons axiomd draws its chrome with come from the desktop's own theme, and the
# directories its own two icons live in are owned by hicolor-icon-theme.
Requires:       adwaita-icon-theme
Requires:       hicolor-icon-theme
# Spell checking needs a dictionary; enchant arrives with libspelling by soname, a
# dictionary does not. English is a recommendation rather than a requirement so that
# a reader in another language is not made to install it.
Recommends:     hunspell-en-US

%description
A fast Markdown reader and editor for GNOME, written in Rust. axiomd opens
Markdown files instantly and renders them with CommonMark and GFM, including
tables, images, diagrams, maths and callouts, with an outline that follows what
is being read, spell checking with corrections while editing, printing, and PDF
or HTML export.

%prep
%autosetup -n %{name}-%{version}

%build
cargo build --release --locked --bin axiomd

%install
scripts/install.sh \
    --prefix %{_prefix} \
    --destdir %{buildroot} \
    --binary target/release/axiomd

# What install.sh refreshes for a prefix somebody is about to run axiomd from, and
# what a package must never carry: these are rebuilt by glib2's, desktop-file-utils'
# and gtk4's file triggers once the transaction is done.
rm -f %{buildroot}%{_datadir}/glib-2.0/schemas/gschemas.compiled
rm -f %{buildroot}%{_datadir}/applications/mimeinfo.cache
rm -f %{buildroot}%{_datadir}/icons/hicolor/icon-theme.cache

# Translations: `po/LINGUAS` is empty today (issue #34 landed the machinery and no
# translation), and rpm refuses an empty `%%find_lang` list — so there is no
# `-f` list below. The moment a language lands, this needs `%%find_lang %%{name}`
# here and `-f %%{name}.lang` on %%files; a gate test fails if LINGUAS gains a
# language while this spec still has neither.

%check
desktop-file-validate %{buildroot}%{_datadir}/applications/%{appid}.desktop
appstreamcli validate --no-net %{buildroot}%{_datadir}/metainfo/%{appid}.metainfo.xml
# The schema the application dies without if it is not compilable.
glib-compile-schemas --strict --dry-run %{buildroot}%{_datadir}/glib-2.0/schemas

%files
%license LICENSE
%doc README.md
%{_bindir}/axiomd
%{_datadir}/applications/%{appid}.desktop
%{_datadir}/metainfo/%{appid}.metainfo.xml
%{_datadir}/glib-2.0/schemas/%{appid}.gschema.xml
%{_datadir}/icons/hicolor/scalable/apps/%{appid}.svg
%{_datadir}/icons/hicolor/symbolic/apps/%{appid}-symbolic.svg

%changelog
* Fri Oct 16 2026 Illya Yalovyy <yalovoy@gmail.com> - 0.9.2-1
- The editor offers corrections for a misspelled word.

* Fri Oct 16 2026 Illya Yalovyy <yalovoy@gmail.com> - 0.9.1-1
- Documents follow the desktop's light and dark again.

* Sun Oct 11 2026 Illya Yalovyy <yalovoy@gmail.com> - 0.9.0-1
- First packaged release.
