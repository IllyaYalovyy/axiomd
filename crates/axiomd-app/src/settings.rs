//! What the reader has asked for, where it is kept, and the dialog they change it in.
//!
//! Every behaviour axiomd lets the reader choose is a key in one GSettings schema
//! (`data/io.github.etf.axiomd.gschema.xml`), and this module is the only place that
//! knows it: no key name, no schema id and no GSettings type appears anywhere else in
//! the application. What the rest of the app sees is a handful of typed questions —
//! which colour scheme to run under, how wide a document may be — plus a way to be
//! told when the answer changes, and the dialog itself.
//!
//! # Changes apply where the reader is, at once
//!
//! Nothing here asks anyone to restart, reopen or re-render. A preference is answered
//! by whoever consumes it as soon as it changes ([`Settings::watch`]), and the two the
//! reader can see today reach the document without touching it: the colour scheme is
//! a libadwaita style-manager call that WebKit's `prefers-color-scheme` follows
//! (probed on WebKitGTK 2.52.5), and the reading width is a user stylesheet swapped on
//! the view. The document on screen is never parsed, rendered or loaded again to
//! answer either of them (invariant 9).
//!
//! # Where the schema comes from
//!
//! An installed axiomd finds its schema among the system's compiled schemas. A copy
//! running from the build tree — `cargo run`, the test suite, the e2e harness — finds
//! the one its own build compiled ([`build.rs`](../build.rs)). The installed schema
//! wins when both exist, which is what a running system should honour.

mod dialog;

use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::gio;
use gtk::glib;

/// The schema holding every setting axiomd has. Also the application id.
const SCHEMA: &str = "io.github.etf.axiomd";

/// Every setting axiomd has, named once.
///
/// A key is only ever addressed through this, so a typo is a compilation failure
/// rather than a preference that silently does nothing, and
/// `every_setting_the_app_names_is_a_key_the_schema_has` keeps the two in step.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Key {
    /// Follow the desktop's colour scheme, or override it.
    Theme,
    /// Whether a document's text is held to a measure at all.
    ReadingWidthLimited,
    /// That measure, in rem.
    ReadingWidth,
    /// Whether a document's headings are listed beside it.
    Outline,
    /// Whether edits are written back without being asked for.
    Autosave,
    /// How long after the last edit that happens, in seconds.
    AutosaveDelay,
    /// Whether the editor marks misspelled words.
    ///
    /// Not consumed yet: spell checking needs libspelling, whose development package
    /// is not installable in this environment (#18, reported to the owner). The key
    /// and its row are here because the behaviour is the reader's to choose the moment
    /// it can be honoured.
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "the schema's key for spell checking; see above")
    )]
    Spellcheck,
    /// The engine new documents are parsed with (#17 consumes it).
    Engine,
    /// The rendering plugins the reader has switched off (#16 consumes it).
    ///
    /// The one key nothing reads yet: no plugin exists to switch off. It is in the
    /// schema because #16 must find it there rather than migrate a store.
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "the schema's key for #16; no plugin exists yet")
    )]
    DisabledPlugins,
}

impl Key {
    /// Every key, for the tests that hold the app and its schema together.
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "the completeness tests are its only reader")
    )]
    const ALL: [Key; 9] = [
        Key::Theme,
        Key::ReadingWidthLimited,
        Key::ReadingWidth,
        Key::Outline,
        Key::Autosave,
        Key::AutosaveDelay,
        Key::Spellcheck,
        Key::Engine,
        Key::DisabledPlugins,
    ];

    /// The name this setting has in the schema.
    const fn name(self) -> &'static str {
        match self {
            Key::Theme => "theme",
            Key::ReadingWidthLimited => "reading-width-limited",
            Key::ReadingWidth => "reading-width",
            Key::Outline => "outline",
            Key::Autosave => "autosave",
            Key::AutosaveDelay => "autosave-delay",
            Key::Spellcheck => "spellcheck",
            Key::Engine => "engine",
            Key::DisabledPlugins => "disabled-plugins",
        }
    }
}

/// The reader's settings, as the running application uses them.
pub(crate) struct Settings {
    store: gio::Settings,
}

/// A subscription to some of the reader's settings, which ends when it is dropped.
///
/// Held by whoever answers the setting — a window, the application — so that a closed
/// window stops being called and stops being kept alive by the call (invariant 7).
pub(crate) struct Watch {
    store: gio::Settings,
    handlers: Vec<glib::SignalHandlerId>,
}

impl Settings {
    /// The reader's own settings, wherever this system keeps them.
    pub(crate) fn new() -> Rc<Self> {
        Rc::new(Self {
            store: gio::Settings::new_full(&schema(), gio::SettingsBackend::NONE, None),
        })
    }

    /// A settings store kept in one file — how a test gets settings of its own.
    #[cfg(test)]
    pub(crate) fn in_file(path: &Path) -> Rc<Self> {
        let backend = gio::keyfile_settings_backend_new(
            path.to_str()
                .expect("a settings file with a printable name"),
            "/",
            None,
        );
        Rc::new(Self {
            store: gio::Settings::new_full(&schema(), Some(&backend), None),
        })
    }

    /// Puts the reader's colour scheme in force, and keeps it there when they change
    /// their mind, until the returned [`Watch`] is dropped.
    ///
    /// libadwaita's style manager is the source of truth (`ux_decisions.md`), and it
    /// is what the rendered document follows too: WebKit answers
    /// `prefers-color-scheme` from it, so a document restyles with the window and is
    /// never re-parsed or reloaded to change colour (invariant 9; probed on WebKitGTK
    /// 2.52.5).
    pub(crate) fn follow_theme(self: &Rc<Self>) -> Watch {
        let settings = self.clone();
        let apply = move || {
            adw::StyleManager::default().set_color_scheme(settings.color_scheme());
        };
        apply();
        self.watch(&[Key::Theme], apply)
    }

    /// Hands `restyle` the stylesheet documents are to be laid out with — now, and
    /// again every time the reader changes how they want to read.
    ///
    /// Whoever holds the returned [`Watch`] is styling documents the reader's way; the
    /// moment they drop it they stop being told, and stop being kept alive by being
    /// told (invariant 7).
    pub(crate) fn follow_reading_style(self: &Rc<Self>, restyle: impl Fn(&str) + 'static) -> Watch {
        let apply = {
            let settings = self.clone();
            move || restyle(&axiomd_render::reader_stylesheet(settings.reading_width()))
        };
        apply();
        self.watch(&[Key::ReadingWidthLimited, Key::ReadingWidth], apply)
    }

    /// Tells `reveal` whether documents are read with their outline beside them — now,
    /// and again every time the reader changes their mind.
    ///
    /// Live in the strictest sense (invariant 14): the sidebar appears or goes in every
    /// open window as the switch is turned, and no document is re-read, re-rendered or
    /// reloaded for it.
    pub(crate) fn follow_outline(self: &Rc<Self>, reveal: impl Fn(bool) + 'static) -> Watch {
        let apply = {
            let settings = self.clone();
            move || reveal(settings.store.boolean(Key::Outline.name()))
        };
        apply();
        self.watch(&[Key::Outline], apply)
    }

    /// How long after the reader stops typing their work is written back, or `None`
    /// when they have asked axiomd not to write it for them.
    ///
    /// One question rather than two keys, because "autosave off" and "autosave in
    /// zero seconds" are not two things a caller should be able to confuse.
    pub(crate) fn autosave(&self) -> Option<Duration> {
        self.store
            .boolean(Key::Autosave.name())
            .then(|| Duration::from_secs(self.store.int(Key::AutosaveDelay.name()).max(1) as u64))
    }

    /// The colour scheme the application runs under: the desktop's own, or the
    /// override the reader chose.
    fn color_scheme(&self) -> adw::ColorScheme {
        match self.store.string(Key::Theme.name()).as_str() {
            "light" => adw::ColorScheme::ForceLight,
            "dark" => adw::ColorScheme::ForceDark,
            // Including anything unknown: a document is still readable under the
            // desktop's own scheme, which is the answer that can never be wrong.
            _ => adw::ColorScheme::Default,
        }
    }

    /// The measure a document's text is held to, in rem — or `None` when the reader
    /// has asked for documents that fill the window.
    fn reading_width(&self) -> Option<u32> {
        self.store
            .boolean(Key::ReadingWidthLimited.name())
            .then(|| self.store.int(Key::ReadingWidth.name()).max(1) as u32)
    }

    /// Calls `answer` whenever any of `keys` changes, until the returned [`Watch`] is
    /// dropped.
    fn watch(&self, keys: &[Key], answer: impl Fn() + 'static) -> Watch {
        let answer: Rc<dyn Fn()> = Rc::new(answer);
        let handlers = keys
            .iter()
            .map(|key| {
                let answer = answer.clone();
                self.store
                    .connect_changed(Some(key.name()), move |_, _| answer())
            })
            .collect();
        Watch {
            store: self.store.clone(),
            handlers,
        }
    }

    /// Opens the preferences dialog over `parent` — what `Ctrl+comma` does.
    pub(crate) fn present_dialog(self: &Rc<Self>, parent: &impl IsA<gtk::Widget>) {
        dialog::present(self, parent);
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        for handler in self.handlers.drain(..) {
            self.store.disconnect(handler);
        }
    }
}

/// The compiled schema, from wherever this copy of axiomd can find one.
fn schema() -> gio::SettingsSchema {
    if let Some(installed) =
        gio::SettingsSchemaSource::default().and_then(|source| source.lookup(SCHEMA, true))
    {
        return installed;
    }

    let built = Path::new(env!("AXIOMD_SCHEMAS"));
    gio::SettingsSchemaSource::from_directory(built, None, true)
        .ok()
        .and_then(|source| source.lookup(SCHEMA, false))
        .unwrap_or_else(|| {
            panic!(
                "axiomd's settings schema is neither installed nor in {}. \
                 An installed axiomd needs {SCHEMA}.gschema.xml in the system's \
                 compiled schemas.",
                built.display(),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::ScratchDir;

    /// Settings kept in a file of this test's own, containing `keys` — written the
    /// way GLib's keyfile backend writes them, which is what the reader's own store
    /// holds after the dialog has been used.
    fn stored(scratch: &ScratchDir, name: &str, keys: &str) -> Rc<Settings> {
        let file = scratch.write(name, format!("[io/github/etf/axiomd]\n{keys}\n"));
        Settings::in_file(&file)
    }

    /// A key the application asks for and the schema does not have is a preference
    /// that aborts the process on first use, so the two lists are checked against
    /// each other rather than trusted.
    #[test]
    fn every_setting_the_app_names_is_a_key_the_schema_has() {
        let schema = schema();
        for key in Key::ALL {
            assert!(
                schema.has_key(key.name()),
                "the schema has no {:?} key; the app calls it {}",
                key,
                key.name(),
            );
        }
    }

    /// And the other direction: a key in the schema that nothing names is a setting
    /// the reader can never reach — the shape #16, #17 and #18 must not leave behind
    /// when they add theirs.
    #[test]
    fn every_key_in_the_schema_is_a_setting_the_app_names() {
        let named: Vec<&str> = Key::ALL.iter().map(|key| key.name()).collect();
        for key in schema().list_keys() {
            assert!(
                named.contains(&key.as_str()),
                "{key} is in the schema and nothing in the app names it",
            );
        }
    }

    /// What a reader who has never opened the dialog gets. These are the defaults
    /// every later feature inherits, so they are pinned here rather than left to the
    /// schema file to change quietly.
    #[test]
    fn a_first_run_follows_the_desktop_and_reads_at_a_comfortable_measure() {
        let scratch = ScratchDir::new("settings-defaults");
        let settings = stored(&scratch, "empty.keyfile", "");

        assert_eq!(settings.color_scheme(), adw::ColorScheme::Default);
        assert_eq!(settings.reading_width(), Some(46));

        let schema = schema();
        for (key, default) in [
            (Key::Outline, "true"),
            (Key::Autosave, "true"),
            (Key::AutosaveDelay, "2"),
            (Key::Spellcheck, "true"),
            (Key::Engine, "'comrak'"),
            (Key::DisabledPlugins, "@as []"),
        ] {
            assert_eq!(
                schema.key(key.name()).default_value().print(true),
                default,
                "{key:?} does not start out as {default}",
            );
        }
    }

    /// A spin button that offers a number the schema refuses writes nothing at all —
    /// GSettings drops the value and the row goes back to what it was. The dialog's
    /// bounds are therefore checked against the schema's own rather than eyeballed.
    #[test]
    fn the_numbers_the_dialog_offers_are_the_ones_the_schema_allows() {
        let schema = schema();
        for (key, low, high) in dialog::BOUNDS {
            let range = schema.key(key.name()).range();
            let (kind, bounds) = range
                .get::<(String, glib::Variant)>()
                .unwrap_or_else(|| panic!("{key:?} has an unreadable range: {range}"));
            assert_eq!(kind, "range", "{key:?} declares no range in the schema");
            assert_eq!(
                bounds.get::<(i32, i32)>(),
                Some((low, high)),
                "the dialog offers {key:?} between {low} and {high}",
            );
        }
    }

    /// The engine a first run reads with has to be one this build actually has, or
    /// every document opens with a preference nothing can honour.
    #[test]
    fn the_engine_a_first_run_reads_with_is_one_this_build_has() {
        let default = schema().key(Key::Engine.name()).default_value();
        let default = default.get::<String>().expect("an engine name");

        assert!(
            crate::document::engines()
                .iter()
                .any(|engine| engine.as_str() == default),
            "documents default to the {default} engine and this build has none",
        );
    }

    /// The three answers to "which colour scheme", including the one a store that
    /// somehow holds nonsense gets.
    #[test]
    fn the_theme_override_is_the_reader_choosing_over_the_desktop() {
        let scratch = ScratchDir::new("settings-theme");

        assert_eq!(
            stored(&scratch, "light.keyfile", "theme='light'").color_scheme(),
            adw::ColorScheme::ForceLight,
        );
        assert_eq!(
            stored(&scratch, "dark.keyfile", "theme='dark'").color_scheme(),
            adw::ColorScheme::ForceDark,
        );
        assert_eq!(
            stored(&scratch, "system.keyfile", "theme='system'").color_scheme(),
            adw::ColorScheme::Default,
        );
    }

    /// The width is two settings the reader sees as one thing: a document either
    /// fills the window or is held to the measure they chose.
    #[test]
    fn a_document_fills_the_window_only_when_the_reader_switched_the_measure_off() {
        let scratch = ScratchDir::new("settings-width");

        assert_eq!(
            stored(&scratch, "held.keyfile", "reading-width=80").reading_width(),
            Some(80),
        );
        assert_eq!(
            stored(
                &scratch,
                "filled.keyfile",
                "reading-width=80\nreading-width-limited=false",
            )
            .reading_width(),
            None,
            "the reader switched the measure off and still got one",
        );
    }

    /// A setting the reader changed is there when they come back: what makes
    /// preferences preferences rather than a session's mood.
    #[test]
    fn a_setting_outlives_the_store_that_wrote_it() {
        let scratch = ScratchDir::new("settings-persist");
        let file = scratch.path().join("settings.keyfile");

        let written = Settings::in_file(&file);
        written.store.set_int(Key::ReadingWidth.name(), 72).unwrap();
        written.store.set_string(Key::Theme.name(), "dark").unwrap();
        gio::Settings::sync();
        drop(written);

        let reopened = Settings::in_file(&file);
        assert_eq!(reopened.reading_width(), Some(72));
        assert_eq!(reopened.color_scheme(), adw::ColorScheme::ForceDark);
    }

    /// What makes a preference apply live: whoever answers it is told, without asking.
    ///
    /// On a main context of this test's own, because GSettings delivers its changes
    /// through the one that was current when the store was opened — sharing the
    /// process-wide one with every other test running beside this would be a race.
    #[test]
    fn changing_a_setting_tells_whoever_answers_it_and_stops_when_they_are_gone() {
        let scratch = ScratchDir::new("settings-watch");
        let context = glib::MainContext::new();

        context
            .with_thread_default(|| {
                let settings = stored(&scratch, "watched.keyfile", "");
                let answered = Rc::new(std::cell::Cell::new(0u32));
                let watch = settings.watch(&[Key::ReadingWidth], {
                    let answered = answered.clone();
                    move || answered.set(answered.get() + 1)
                });

                settings
                    .store
                    .set_int(Key::ReadingWidth.name(), 60)
                    .unwrap();
                delivered(&context);
                assert_eq!(answered.get(), 1, "nobody was told the width changed");

                // Another setting, which this watcher never asked about.
                settings
                    .store
                    .set_boolean(Key::Spellcheck.name(), false)
                    .unwrap();
                delivered(&context);
                assert_eq!(
                    answered.get(),
                    1,
                    "a watcher was woken for a setting it never asked about",
                );

                drop(watch);
                settings
                    .store
                    .set_int(Key::ReadingWidth.name(), 61)
                    .unwrap();
                delivered(&context);
                assert_eq!(
                    answered.get(),
                    1,
                    "a window that has closed is still being told about settings",
                );
            })
            .expect("a main context of this test's own");
    }

    /// Runs everything `context` has to do right now: GSettings hands a change to
    /// the main loop rather than calling back from inside the write.
    fn delivered(context: &glib::MainContext) {
        for _ in 0..1000 {
            if !context.iteration(false) {
                return;
            }
        }
        panic!("the main context never went quiet");
    }
}
