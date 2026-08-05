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
    /// How wide they are listed, in pixels. Window state, not a preference.
    SidebarWidth,
    /// How wide a window opens, in pixels. Window state.
    WindowWidth,
    /// How tall it opens. Window state.
    WindowHeight,
    /// Whether it opens filling the screen. Window state.
    WindowMaximized,
    /// Whether edits are written back without being asked for.
    Autosave,
    /// How long after the last edit that happens, in seconds.
    AutosaveDelay,
    /// Whether the editor marks misspelled words.
    Spellcheck,
    /// The engine documents are read with unless a window says otherwise.
    Engine,
    /// The rendering plugins the reader has switched off, by id.
    DisabledPlugins,
}

impl Key {
    /// Every key, for the tests that hold the app and its schema together.
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "the completeness tests are its only reader")
    )]
    const ALL: [Key; 13] = [
        Key::Theme,
        Key::ReadingWidthLimited,
        Key::ReadingWidth,
        Key::Outline,
        Key::SidebarWidth,
        Key::WindowWidth,
        Key::WindowHeight,
        Key::WindowMaximized,
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
            Key::SidebarWidth => "sidebar-width",
            Key::WindowWidth => "window-width",
            Key::WindowHeight => "window-height",
            Key::WindowMaximized => "is-maximized",
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
///
/// Not every question the reader's way of reading depends on is a setting of axiomd's:
/// high contrast belongs to the desktop and arrives from libadwaita's style manager.
/// A subscription therefore holds handlers on whatever objects answer it, and ending it
/// ends all of them.
pub(crate) struct Watch {
    handlers: Vec<(glib::Object, glib::SignalHandlerId)>,
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

    /// Hands `restyle` the way documents are to be shown to this reader — now, and
    /// again every time anything about it changes.
    ///
    /// Four questions answered as one, because the caller has no use for the
    /// difference: the measure the reader chose, which is a preference of axiomd's;
    /// whether the desktop is asking for high contrast or for less movement, which are
    /// accessibility settings of the desktop's and never axiomd's to offer; and which
    /// palette is in force, which decides the colour the pane is painted before a
    /// document has drawn anything (issue #40). All four arrive as one
    /// [`axiomd_render::Reading`] and all four apply the same way — the page on screen
    /// recalculates its style, or the view is repainted, and nothing is parsed,
    /// rendered or loaded again (invariant 9).
    ///
    /// Whoever holds the returned [`Watch`] is styling documents the reader's way; the
    /// moment they drop it they stop being told, and stop being kept alive by being
    /// told (invariant 7).
    pub(crate) fn follow_reading_style(
        self: &Rc<Self>,
        restyle: impl Fn(&axiomd_render::Reading) + 'static,
    ) -> Watch {
        let apply: Rc<dyn Fn()> = {
            let settings = self.clone();
            Rc::new(move || {
                restyle(&axiomd_render::reading(
                    settings.reading_width(),
                    palette(),
                    contrast(),
                    motion(),
                ))
            })
        };
        apply();

        let mut watch = self.watch(&[Key::ReadingWidthLimited, Key::ReadingWidth], {
            let apply = apply.clone();
            move || apply()
        });
        // The desktop's own, which no key of axiomd's holds: libadwaita reports high
        // contrast whether it learned it from the settings portal or from
        // `org.gnome.desktop.a11y.interface`, and it reports a change to it while the
        // reader is reading (probed on libadwaita 1.8.6). Light and dark comes from the
        // same manager, and covers the reader's own theme preference too: that is set
        // by putting a colour scheme on this very manager (`follow_theme`), so both
        // ways of going dark arrive here as one signal.
        let manager = adw::StyleManager::default();
        for handler in [
            manager.connect_high_contrast_notify({
                let apply = apply.clone();
                move |_| apply()
            }),
            manager.connect_dark_notify({
                let apply = apply.clone();
                move |_| apply()
            }),
        ] {
            watch.also(&manager, handler);
        }
        // And whether the desktop wants anything on screen to move, which GTK owns
        // rather than libadwaita.
        if let Some(gtk) = gtk::Settings::default() {
            let handler = gtk.connect_gtk_enable_animations_notify(move |_| apply());
            watch.also(&gtk, handler);
        }
        watch
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
            move || reveal(settings.outline_shown())
        };
        apply();
        self.watch(&[Key::Outline], apply)
    }

    /// Whether documents are read with their outline beside them, right now — what a
    /// sidebar that has been out of the way goes back to when there is finally
    /// something in it (issue #32).
    pub(crate) fn outline_shown(&self) -> bool {
        self.store.boolean(Key::Outline.name())
    }

    /// How wide the outline sits beside the document, in pixels — where the reader
    /// last let go of the divider.
    ///
    /// Window state rather than a preference, and the one setting here with no row in
    /// the dialog and no `follow_` beside it (owner ruling, `ux_decisions.md`, issue
    /// #27): a width is chosen by dragging the thing itself, so it belongs to a window
    /// the way a window's own size does. A window takes it when it is built and gives
    /// it back when the reader lets go — never under the hands of another window that
    /// is open at the time.
    pub(crate) fn sidebar_width(&self) -> i32 {
        self.store.int(Key::SidebarWidth.name())
    }

    /// Remembers the width the reader has just dragged the divider to.
    pub(crate) fn remember_sidebar_width(&self, width: i32) {
        self.remember(Key::SidebarWidth, width.to_variant());
    }

    /// Opens `window` the size and shape the reader last left one, and remembers this
    /// one when it goes (issue #30).
    ///
    /// Window state, like the divider's width and for the same reason: a window's size
    /// is chosen by dragging the window, so there is no row in the dialog to hunt for
    /// (`ux_decisions.md`). The caller says only which window; when it is read, when it
    /// is written and what is worth writing are this module's business.
    ///
    /// The size written down is the window's *default* size and never its allocation.
    /// GTK keeps that in step with every resize the reader makes and holds it still
    /// "unless the window is forced to a size, like when it is maximized or
    /// fullscreened", and its documentation says in as many words that it is the size
    /// to save and that using the allocation "will not work in all circumstances and
    /// can lead to growing or shrinking windows" (`gtk_window_set_default_size`, GTK
    /// 4.20 `Gtk-4.0.gir`). That is also what keeps a maximized window from overwriting
    /// the size the reader chose: they come back to a maximized window, and taking it
    /// out of the screen's hands leaves them the window they had. The allocation, for
    /// its part, is already gone by `unrealize`.
    ///
    /// Written once, as the window goes: `unrealize` is the last signal a closing
    /// window emits, whichever way it was closed (probed on GTK 4.20.4 — `shell.rs`
    /// records which of the four signals a closing window really emits).
    pub(crate) fn keep_window_geometry(self: &Rc<Self>, window: &adw::ApplicationWindow) {
        window.set_default_size(
            self.store.int(Key::WindowWidth.name()),
            self.store.int(Key::WindowHeight.name()),
        );
        if self.store.boolean(Key::WindowMaximized.name()) {
            window.maximize();
        }

        let settings = self.clone();
        window.connect_unrealize(move |window| {
            let (width, height) = window.default_size();
            settings.remember(Key::WindowWidth, width.to_variant());
            settings.remember(Key::WindowHeight, height.to_variant());
            settings.remember(Key::WindowMaximized, window.is_maximized().to_variant());
            // A window closing may be the last thing this process does, so what it
            // learned is put beyond the process's own lifetime rather than left to a
            // flush that may never come.
            gio::Settings::sync();
        });
    }

    /// Writes down one thing the reader has done to their windows, saying so rather
    /// than failing silently if the store will not have it.
    fn remember(&self, key: Key, value: glib::Variant) {
        if self.store.value(key.name()) == value {
            return;
        }
        if let Err(error) = self.store.set_value(key.name(), &value) {
            eprintln!("axiomd: could not remember {}: {error}", key.name());
        }
    }

    /// Forgets it, and answers the width that leaves — what a reader who has never
    /// touched the divider reads at.
    pub(crate) fn forget_sidebar_width(&self) -> i32 {
        self.store.reset(Key::SidebarWidth.name());
        self.sidebar_width()
    }

    /// Tells `check` whether the editor marks misspelled words — now, and again every
    /// time the reader changes their mind.
    ///
    /// Like the sidebar and unlike a plugin, this is a way of *drawing* the source:
    /// turning it reaches the buffer the reader is typing in without anything being
    /// re-read, re-rendered or reloaded for it (invariants 9 and 14).
    pub(crate) fn follow_spellcheck(self: &Rc<Self>, check: impl Fn(bool) + 'static) -> Watch {
        let apply = {
            let settings = self.clone();
            move || check(settings.store.boolean(Key::Spellcheck.name()))
        };
        apply();
        self.watch(&[Key::Spellcheck], apply)
    }

    /// The optional rendering capabilities the reader is reading with — every plugin
    /// this build has, minus the ones they switched off.
    ///
    /// Read at each render rather than remembered, so switching one changes the very
    /// next render of every open document and nothing is restarted or reloaded for it
    /// (invariant 14).
    pub(crate) fn plugins(&self) -> axiomd_render::Plugins {
        axiomd_render::Plugins::builtin(&self.disabled_plugins())
    }

    /// The engine documents are read with unless the window they are in says
    /// otherwise (issue #17).
    ///
    /// Answered through the registry rather than handed back as the reader wrote it,
    /// so a store naming an engine this build does not have — one that has been
    /// renamed, or a preference written by a newer build — reads with the default
    /// instead of with nothing.
    pub(crate) fn engine(&self) -> axiomd_engine::EngineId {
        let stored = self.store.string(Key::Engine.name());
        axiomd_engine::engine(&stored)
            .unwrap_or_else(|| axiomd_engine::engines()[0])
            .id()
    }

    /// Tells `rerender` that the engine has changed, whenever it does.
    ///
    /// Like a plugin and unlike the measure, an engine decides what the document *is*,
    /// so it costs a render — but never a reload: the window patches the page it is
    /// already showing and the reader keeps their place (invariants 5, 9 and 14).
    pub(crate) fn follow_engine(self: &Rc<Self>, rerender: impl Fn() + 'static) -> Watch {
        self.watch(&[Key::Engine], rerender)
    }

    /// Tells `rerender` that the plugins have changed, whenever they do.
    ///
    /// The one preference that is not a restyle: a plugin decides what the document
    /// *is*, not how it looks, so it costs a render — but never a reload. The window
    /// patches the page it is already showing, which is what keeps the reader's place
    /// (invariants 5 and 9).
    pub(crate) fn follow_plugins(self: &Rc<Self>, rerender: impl Fn() + 'static) -> Watch {
        self.watch(&[Key::DisabledPlugins], rerender)
    }

    /// The ids the reader has switched off, as the store holds them.
    fn disabled_plugins(&self) -> Vec<String> {
        self.store
            .strv(Key::DisabledPlugins.name())
            .iter()
            .map(|id| id.to_string())
            .collect()
    }

    /// Switches one plugin on or off, keeping every other id the store holds —
    /// including one belonging to a plugin this build does not have.
    pub(crate) fn set_plugin_enabled(&self, id: &str, enabled: bool) {
        let mut disabled = self.disabled_plugins();
        let already = disabled.iter().any(|off| off == id);
        if already == !enabled {
            return;
        }
        match enabled {
            true => disabled.retain(|off| off != id),
            false => disabled.push(id.to_owned()),
        }
        let disabled: Vec<&str> = disabled.iter().map(String::as_str).collect();
        if let Err(error) = self.store.set_strv(Key::DisabledPlugins.name(), disabled) {
            eprintln!("axiomd: could not write the plugin setting: {error}");
        }
    }

    /// Whether the reader has `id` switched on.
    pub(crate) fn plugin_enabled(&self, id: &str) -> bool {
        !self.disabled_plugins().iter().any(|off| off == id)
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
                let handler = self
                    .store
                    .connect_changed(Some(key.name()), move |_, _| answer());
                (self.store.clone().upcast::<glib::Object>(), handler)
            })
            .collect();
        Watch { handlers }
    }

    /// Opens the preferences dialog over `parent` — what `Ctrl+comma` does.
    pub(crate) fn present_dialog(self: &Rc<Self>, parent: &impl IsA<gtk::Widget>) {
        dialog::present(self, parent);
    }
}

impl Watch {
    /// Adds one more thing this subscription is listening to, so that it ends with the
    /// rest of it.
    fn also(&mut self, listening_to: &impl IsA<glib::Object>, handler: glib::SignalHandlerId) {
        self.handlers.push((listening_to.clone().upcast(), handler));
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        for (listening_to, handler) in self.handlers.drain(..) {
            listening_to.disconnect(handler);
        }
    }
}

/// How much contrast the desktop is asking documents to be read at.
///
/// libadwaita's style manager is the source of truth for this as it is for light and
/// dark (`ux_decisions.md`). It is not a preference of axiomd's and never becomes one:
/// a reader who needs high contrast has already said so once, to their desktop.
fn contrast() -> axiomd_render::Contrast {
    match adw::StyleManager::default().is_high_contrast() {
        true => axiomd_render::Contrast::High,
        false => axiomd_render::Contrast::Normal,
    }
}

/// Which palette documents are being read in.
///
/// The same style manager, and the same source of truth: the reader's own light/dark
/// preference is put into force by setting a colour scheme on it (`follow_theme`), so
/// this one answer covers both their choice and the desktop's.
fn palette() -> axiomd_render::Palette {
    match adw::StyleManager::default().is_dark() {
        true => axiomd_render::Palette::Dark,
        false => axiomd_render::Palette::Light,
    }
}

/// Whether the desktop wants anything on screen to move.
///
/// GTK's own setting, which is what a desktop's "reduce animation" switch reaches
/// (`org.gnome.desktop.interface enable-animations`, and `gtk-enable-animations` in a
/// `settings.ini`). An accessibility answer like high contrast, so it is read from the
/// desktop and never offered as a preference of axiomd's.
fn motion() -> axiomd_render::Motion {
    match gtk::Settings::default().is_none_or(|gtk| gtk.is_gtk_enable_animations()) {
        true => axiomd_render::Motion::Full,
        false => axiomd_render::Motion::Reduced,
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
            (Key::SidebarWidth, "260"),
            (Key::WindowWidth, "900"),
            (Key::WindowHeight, "700"),
            (Key::WindowMaximized, "false"),
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

    /// The same for the divider: a width the reader can drag to that the schema
    /// refuses is a drag GSettings drops on the floor, leaving the outline a width it
    /// will not have next time.
    #[test]
    fn the_widths_the_divider_allows_are_the_ones_the_schema_does() {
        let range = schema().key(Key::SidebarWidth.name()).range();
        let (kind, bounds) = range
            .get::<(String, glib::Variant)>()
            .unwrap_or_else(|| panic!("the sidebar width has an unreadable range: {range}"));

        assert_eq!(
            kind, "range",
            "the sidebar width declares no range in the schema"
        );
        assert_eq!(
            bounds.get::<(i32, i32)>(),
            Some(crate::outline::BOUNDS),
            "the divider lets the reader drag the outline outside what the schema \
             will store",
        );
    }

    /// Forgetting the width is what a double-click on the divider does, and what it
    /// leaves behind has to be the width a reader who never touched it reads at —
    /// otherwise the way back leads somewhere the reader has never been.
    #[test]
    fn forgetting_the_dragged_width_leaves_the_one_a_first_run_has() {
        let scratch = ScratchDir::new("settings-sidebar");
        let settings = stored(&scratch, "dragged.keyfile", "sidebar-width=400");
        assert_eq!(settings.sidebar_width(), 400);

        let usual = settings.forget_sidebar_width();

        assert_eq!(
            usual,
            schema()
                .key(Key::SidebarWidth.name())
                .default_value()
                .get::<i32>()
                .expect("a width"),
        );
        assert_eq!(settings.sidebar_width(), usual);
    }

    /// The engine a first run reads with has to be one this build actually has, or
    /// every document opens with a preference nothing can honour.
    #[test]
    fn the_engine_a_first_run_reads_with_is_one_this_build_has() {
        let default = schema().key(Key::Engine.name()).default_value();
        let default = default.get::<String>().expect("an engine name");

        assert!(
            axiomd_engine::engine(&default).is_some(),
            "documents default to the {default} engine and this build has none",
        );
        // And it is the engine the registry itself calls the default, so the schema
        // and the code cannot drift into naming two different ones.
        assert_eq!(
            default,
            axiomd_engine::engines()[0].id().as_str(),
            "the schema and the registry disagree about the default engine",
        );
    }

    /// A store naming an engine this build does not have still reads documents.
    ///
    /// The failure this guards is a reader upgrading — or downgrading — into a window
    /// that renders nothing because of a string nobody can act on.
    #[test]
    fn an_engine_the_build_does_not_have_falls_back_to_the_default() {
        let scratch = ScratchDir::new("settings-engine");

        assert_eq!(
            stored(&scratch, "known.keyfile", "engine='pulldown-cmark'").engine(),
            axiomd_engine::PulldownEngine::ID,
        );
        assert_eq!(
            stored(&scratch, "unknown.keyfile", "engine='no-such-engine'").engine(),
            axiomd_engine::engines()[0].id(),
            "a store naming an engine this build has never heard of left the reader \
             with no engine at all",
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
