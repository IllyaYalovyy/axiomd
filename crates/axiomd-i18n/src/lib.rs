//! Every word axiomd says to its reader, in their own language.
//!
//! This is the whole of axiomd's localisation: one translation domain, one place that
//! knows where its catalogues live, and the four calls the rest of the application
//! says its words through. No other crate names the domain, touches the locale, or
//! knows that gettext exists.
//!
//! # How a word gets translated
//!
//! [`setup`] runs once, at the top of `main`, before GTK: it puts the process into the
//! reader's own locale and points the C library at the catalogues installed beside the
//! binary. From then on [`gettext`] answers with the reader's words when a catalogue
//! has them and with the English ones when it has not — which is also what happens
//! under `LC_ALL=C`, where the application reads exactly as it did before any of this
//! existed.
//!
//! # Words that are written down before there is a reader
//!
//! A `const` table — the shortcut list, the theme names, a plugin's own name — is
//! built at compile time, long before a locale exists. Those literals are marked with
//! [`gettext_noop`], which translates nothing and only makes `xgettext` see them, and
//! they are put through [`gettext`] at the moment they reach the screen.
//!
//! # Where the catalogues are
//!
//! Beside the binary, under the prefix it was installed into: `bin/axiomd` and
//! `share/locale` are siblings, in `/usr/local`, in `~/.local` and in a flatpak's
//! `/app` alike. A copy running from the build tree has no such directory and finds no
//! catalogues, which is the right answer — nothing has been installed anywhere.
//!
//! # Keeping the catalogue complete
//!
//! `po/POTFILES.in` lists every file a translatable word is written in, and
//! `tests/catalog.rs` holds that list to what `xgettext` actually finds, so a new
//! screen of words cannot reach a release with no way for a translator to see it.

use std::path::{Path, PathBuf};

/// The name of axiomd's message catalogues: `share/locale/<language>/LC_MESSAGES/axiomd.mo`.
///
/// Also what `po/axiomd.pot` is called, what the metainfo's `<translation>` tag names,
/// and the one string in this project that a translator's tooling has to agree with.
pub const DOMAIN: &str = "axiomd";

/// Puts the process into the reader's locale and finds axiomd's catalogues.
///
/// Called once, from `main`, before GTK — GTK sets the locale itself when it starts,
/// and a word said before that would otherwise be said in the wrong language.
///
/// Nothing here can stop axiomd from running. A prefix with no catalogues in it, or a
/// locale the machine has not got, leaves every word in English and says so on stderr:
/// a reader who cannot read their own language is still reading their document.
pub fn setup() {
    if gettextrs::setlocale(gettextrs::LocaleCategory::LcAll, "").is_none() {
        eprintln!(
            "axiomd: this machine has no locale by the name the environment gives, \
             so axiomd is in English"
        );
    }
    bind(&catalogs());
}

/// `msgid` in the reader's language, or `msgid` itself when nothing has been
/// translated into it.
pub fn gettext(msgid: &str) -> String {
    gettextrs::gettext(msgid)
}

/// The same, for a word whose English is ambiguous on its own — `msgctxt` is the note
/// to the translator, and never reaches the screen.
pub fn pgettext(msgctxt: &str, msgid: &str) -> String {
    gettextrs::pgettext(msgctxt, msgid)
}

/// The same, for a phrase counting something: `n` picks the form the reader's language
/// uses for that number, which is not always the two English has.
pub fn ngettext(msgid: &str, msgid_plural: &str, n: u32) -> String {
    gettextrs::ngettext(msgid, msgid_plural, n)
}

/// Marks a literal as one to translate, without translating it here.
///
/// For the `const` tables that are written down before a locale exists. `xgettext`
/// collects the literal for the translator; whoever puts it on screen puts it through
/// [`gettext`] first.
#[must_use]
pub const fn gettext_noop(msgid: &'static str) -> &'static str {
    msgid
}

/// Points the C library at `catalogs` for axiomd's domain, complaining where a
/// developer will see it rather than failing.
fn bind(catalogs: &Path) {
    if let Err(trouble) = gettextrs::bindtextdomain(DOMAIN, catalogs) {
        eprintln!(
            "axiomd: {} is not somewhere translations can be read from, so axiomd is \
             in English: {trouble}",
            catalogs.display(),
        );
        return;
    }
    // Said out loud, because the C library would otherwise hand back a catalogue in
    // the locale's own encoding and every accented character in it would reach GTK as
    // bytes that are not UTF-8.
    if let Err(trouble) = gettextrs::bind_textdomain_codeset(DOMAIN, "UTF-8") {
        eprintln!("axiomd: translations could not be asked for in UTF-8: {trouble}");
    }
    if let Err(trouble) = gettextrs::textdomain(DOMAIN) {
        eprintln!("axiomd: translations could not be switched on: {trouble}");
    }
}

/// Where this copy of axiomd's catalogues are: `share/locale` under the prefix its
/// binary was installed into.
///
/// Derived from the running binary rather than baked in at build time, so one build
/// works from `/usr/local`, from `~/.local` and from a flatpak's `/app` without being
/// told which — the same thing that makes a per-user install possible at all
/// (issue #25). A build tree has no such directory; the fallback is the system's own,
/// where a distribution package puts them.
fn catalogs() -> PathBuf {
    let installed = std::env::current_exe()
        .ok()
        .and_then(|binary| Some(binary.parent()?.parent()?.join("share/locale")))
        .filter(|catalogs| catalogs.is_dir());
    installed.unwrap_or_else(|| PathBuf::from("/usr/share/locale"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// A catalogue in a language nobody speaks, so that a word coming back changed can
    /// only have come through gettext. One of each kind the application uses: a plain
    /// word, a word under a context, and a phrase that counts.
    const CATALOGUE: &str = r#"
msgid ""
msgstr "Content-Type: text/plain; charset=UTF-8\nPlural-Forms: nplurals=2; plural=(n != 1);\n"

msgid "A door to open"
msgstr "Malfermu pordon"

msgctxt "a doorway"
msgid "Aa"
msgstr "aA"

msgid "{n} door"
msgid_plural "{n} doors"
msgstr[0] "{n} pordo"
msgstr[1] "{n} pordoj"
"#;

    /// A message this test asks about, built rather than written down.
    ///
    /// `xgettext` collects a *literal* handed to one of the calls above, so a test that
    /// wrote its fixtures out in full would put its own invented words into the template
    /// translators work from. Built here, they are this test's and nobody else's — and
    /// `tests/catalog.rs` checks that this file offers a translator nothing at all.
    fn asked(msgid: &str) -> String {
        msgid.to_owned()
    }

    /// A locale this machine really has, other than `C` — the only state in which the
    /// C library reads a message catalogue at all.
    ///
    /// Insisted on rather than skipped past: a test that quietly does not run is a test
    /// that is not there.
    fn a_locale_this_machine_has() -> String {
        let listed = Command::new("locale")
            .arg("-a")
            .output()
            .expect("run `locale -a` to find a locale to translate into");
        let names = String::from_utf8_lossy(&listed.stdout);
        names
            .lines()
            .map(str::trim)
            .find(|name| {
                !name.starts_with('C') && !name.starts_with("POSIX") && name.contains("utf8")
            })
            .map(str::to_owned)
            .unwrap_or_else(|| {
                panic!(
                    "this machine has no UTF-8 locale but C, so nothing can check that \
                     axiomd reads a translation at all. Install one \
                     (Fedora: sudo dnf install glibc-langpack-en, \
                     Debian: sudo locale-gen en_US.UTF-8).",
                )
            })
    }

    /// The directory a catalogue for `locale` is looked for under: the language and
    /// territory, without the encoding the C library adds and strips for itself.
    fn catalog_directory(locale: &str) -> &str {
        locale.split(['.', '@']).next().unwrap_or(locale)
    }

    /// Compiles `po` into a catalogue for `locale` under a directory of this test's
    /// own, and answers that directory.
    fn catalog_for(locale: &str, po: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("axiomd-i18n-{}", std::process::id()));
        let messages = root.join(catalog_directory(locale)).join("LC_MESSAGES");
        std::fs::create_dir_all(&messages).expect("make a directory to compile a catalogue into");
        let source = root.join("test.po");
        std::fs::write(&source, po).expect("write a catalogue to compile");

        let compiled = Command::new("msgfmt")
            .arg(&source)
            .arg("-o")
            .arg(messages.join(format!("{DOMAIN}.mo")))
            .output()
            .expect(
                "run msgfmt to compile a catalogue \
                 (Fedora: sudo dnf install gettext, Debian: sudo apt install gettext)",
            );
        assert!(
            compiled.status.success(),
            "msgfmt refused the test catalogue: {}",
            String::from_utf8_lossy(&compiled.stderr),
        );
        root
    }

    /// The whole of the wiring, in the two states a reader can be in: with a catalogue
    /// in their language they read their own words, and under `C` — the locale the e2e
    /// suite pins and the one a build machine runs in — they read the English the
    /// source is written in, unchanged.
    ///
    /// One test rather than two, because `setlocale` is the process's and not a
    /// thread's: two tests asserting about it would be asserting about each other.
    #[test]
    fn a_catalogue_in_the_readers_language_is_what_the_reader_reads() {
        let locale = a_locale_this_machine_has();
        let catalogs = catalog_for(&locale, CATALOGUE);
        bind(&catalogs);

        // Nothing has asked for a locale yet, so this process is in `C`: every word is
        // the English one the source says, catalogue or no catalogue.
        assert_eq!(gettext(&asked("A door to open")), "A door to open");
        assert_eq!(
            ngettext(&asked("{n} door"), &asked("{n} doors"), 2),
            "{n} doors",
        );
        assert_eq!(pgettext(&asked("a doorway"), &asked("Aa")), "Aa");

        assert!(
            gettextrs::setlocale(gettextrs::LocaleCategory::LcAll, locale.clone()).is_some(),
            "`locale -a` named {locale} and the C library does not have it",
        );

        assert_eq!(gettext(&asked("A door to open")), "Malfermu pordon");
        assert_eq!(
            gettext(&asked("A door the catalogue never heard of")),
            "A door the catalogue never heard of",
            "a word the catalogue does not translate stopped being readable",
        );
        assert_eq!(
            ngettext(&asked("{n} door"), &asked("{n} doors"), 1),
            "{n} pordo",
        );
        assert_eq!(
            ngettext(&asked("{n} door"), &asked("{n} doors"), 5),
            "{n} pordoj",
        );
        assert_eq!(
            pgettext(&asked("a doorway"), &asked("Aa")),
            "aA",
            "a word translated under a context did not come back under it",
        );

        std::fs::remove_dir_all(&catalogs).ok();
    }

    /// A marked literal is the literal, whatever the locale: it exists for `xgettext`
    /// and for nothing else, and a `const` table built out of it must read the same in
    /// every language until somebody puts it through [`gettext`].
    #[test]
    fn a_marked_literal_is_left_exactly_as_it_was_written() {
        // Named rather than written into the call, for the reason `asked` exists.
        const FIXTURE: &str = "A door to open";
        const MARKED: &str = gettext_noop(FIXTURE);

        assert_eq!(MARKED, "A door to open");
    }
}
