//! The chrome every GNOME application is expected to have: what its keys do, and what
//! it is.
//!
//! Three things live here because they are one thing — the shortcut table. Every key
//! the application installs is a row of [`SECTIONS`], the accelerators are set from it,
//! and the keyboard-shortcuts dialog is built from it, so a key the reader can press
//! and a key the dialog lists cannot be two different sets. Adding a shortcut anywhere
//! else than this table is what the completeness test in `shell.rs` catches.
//!
//! # Where the About dialog's words come from
//!
//! The AppStream metainfo (`data/io.github.etf.axiomd.metainfo.xml`), compiled in.
//! That file is what a software centre shows and what the package installs, so what
//! axiomd says about itself in the About dialog and what the desktop says about it
//! cannot disagree: change the homepage in one place and both follow.
//!
//! The version is the one thing that does not come from it — `CARGO_PKG_VERSION` is
//! what this binary actually is, and the metainfo's newest release is held to it by a
//! test below rather than trusted.

use adw::prelude::*;
use gtk::gio;

/// One row of the keyboard-shortcuts dialog: what the reader calls it, the action it
/// runs, and the keys it is on.
struct Shortcut {
    /// The words the dialog lists it under, and the words its search matches.
    title: &'static str,
    /// The action the key activates, in full — `app.` for the application's own,
    /// `win.` for a window's.
    action: &'static str,
    /// Every key it is on. More than one where a real keyboard has more than one: a
    /// plus that needs shift and the equal beside it that does not, the keypad's own,
    /// and a redo the whole desktop spells two ways.
    keys: &'static [&'static str],
}

/// Every keyboard shortcut the application installs, grouped the way the shortcuts
/// dialog lists them.
///
/// This is the whole of it. `Ctrl+W` and `Ctrl+Q` are here and *not* in the primary
/// menu: the HIG keeps closing and quitting off it, and this dialog is where a reader
/// finds them (issue #29).
const SECTIONS: &[(&str, &[Shortcut])] = &[
    (
        "General",
        &[
            Shortcut {
                title: "New Window",
                action: "app.new",
                keys: &["<Control>n"],
            },
            Shortcut {
                title: "Open Document",
                action: "app.open",
                keys: &["<Control>o"],
            },
            Shortcut {
                title: "Preferences",
                action: "app.preferences",
                keys: &["<Control>comma"],
            },
            Shortcut {
                title: "Keyboard Shortcuts",
                action: KEYS,
                keys: &["<Control>question"],
            },
            Shortcut {
                title: "Close Window",
                action: "app.close-window",
                keys: &["<Control>w"],
            },
            Shortcut {
                title: "Quit",
                action: "app.quit",
                keys: &["<Control>q"],
            },
        ],
    ),
    (
        "Reading",
        &[
            Shortcut {
                title: "Back",
                action: crate::window::BACK,
                keys: &["<Alt>Left"],
            },
            Shortcut {
                title: "Forward",
                action: crate::window::FORWARD,
                keys: &["<Alt>Right"],
            },
            Shortcut {
                title: "Outline",
                action: crate::window::OUTLINE,
                keys: &["F9"],
            },
            // How big the document is (UT-011). `Ctrl+equal` beside `Ctrl+plus` and
            // `Ctrl+KP_Add` beside both: a plus needs shift on most layouts and lives
            // on the keypad on none of them, and every desktop browser and editor
            // accepts all three.
            Shortcut {
                title: "Zoom In",
                action: crate::zoom::IN,
                keys: &["<Control>plus", "<Control>equal", "<Control>KP_Add"],
            },
            Shortcut {
                title: "Zoom Out",
                action: crate::zoom::OUT,
                keys: &["<Control>minus", "<Control>KP_Subtract"],
            },
            Shortcut {
                title: "Reset Zoom",
                action: crate::zoom::RESET,
                keys: &["<Control>0", "<Control>KP_0"],
            },
            Shortcut {
                title: "Find",
                action: crate::find::FIND,
                keys: &["<Control>f"],
            },
            Shortcut {
                title: "Find Next",
                action: crate::find::FIND_NEXT,
                keys: &["<Control>g"],
            },
            Shortcut {
                title: "Find Previous",
                action: crate::find::FIND_PREVIOUS,
                keys: &["<Shift><Control>g"],
            },
            // Escape, and only while the bar is up: the action is disabled the rest of
            // the time (`find.rs`), and a shortcut whose action is disabled does not
            // fire — probed on GTK 4.20.4, where `gtk_shortcut_action_activate` on a
            // `GtkNamedAction` naming a disabled action answers FALSE and answers TRUE
            // the moment it is enabled. A shortcut that did not activate does not
            // consume the key, so Escape goes on meaning whatever else it means in
            // this window.
            Shortcut {
                title: "Close Search",
                action: crate::find::FIND_CLOSE,
                keys: &["Escape"],
            },
        ],
    ),
    (
        "Editing",
        &[
            Shortcut {
                title: "Switch Between Reading and Editing",
                action: crate::window::MODE,
                keys: &["<Control>e"],
            },
            Shortcut {
                title: "Save",
                action: crate::window::SAVE,
                keys: &["<Control>s"],
            },
            // Ctrl+Shift+S, spelled the way GTK normalises it — `accels_for_action`
            // answers in this order, so writing it the other way round would make the
            // table and the running application disagree.
            Shortcut {
                title: "Save As",
                action: crate::window::SAVE_AS,
                keys: &["<Shift><Control>s"],
            },
            Shortcut {
                title: "Undo",
                action: crate::window::UNDO,
                keys: &["<Control>z"],
            },
            // Shift+Ctrl+Z first, because that is what this desktop's own editors redo
            // with; Ctrl+Y beside it because the readers who arrive from elsewhere
            // press that one (issue #29).
            Shortcut {
                title: "Redo",
                action: crate::window::REDO,
                keys: &["<Shift><Control>z", "<Control>y"],
            },
        ],
    ),
    (
        "Sharing",
        &[
            Shortcut {
                title: "Print",
                action: crate::window::PRINT,
                keys: &["<Control>p"],
            },
            Shortcut {
                title: "Export",
                action: crate::window::EXPORT,
                keys: &["<Shift><Control>e"],
            },
        ],
    ),
];

/// The two menu items the HIG asks every primary menu to end with, named once so the
/// menu and the actions behind it cannot drift apart.
pub(crate) const KEYS: &str = "app.shortcuts";
pub(crate) const ABOUT: &str = "app.about";

/// What the About dialog says about axiomd, and what the package installs beside it.
const METAINFO: &str = include_str!("../../../data/io.github.etf.axiomd.metainfo.xml");

/// Installs every keyboard shortcut, and the two dialogs that explain the application
/// to the reader.
pub(crate) fn arm(app: &adw::Application) {
    for shortcut in SECTIONS.iter().flat_map(|(_, shortcuts)| *shortcuts) {
        app.set_accels_for_action(shortcut.action, shortcut.keys);
    }
    present(app, KEYS, |window| {
        keyboard_shortcuts().present(Some(window));
    });
    present(app, ABOUT, |window| {
        about().present(Some(window));
    });
}

/// What the dialog `of` names is showing, as the reader reads it, or nothing at all
/// when the window is not showing that dialog.
///
/// Answers for the two dialogs this module raises and for no other name, so the
/// caller can ask about anything and get `None` for what is not ours.
pub(crate) fn showing(window: &adw::ApplicationWindow, of: &str) -> Option<String> {
    let dialog = window.visible_dialog();
    match of {
        "about" => Some(
            dialog
                .and_downcast::<adw::AboutDialog>()
                .map(|about| said_by(&about))
                .unwrap_or_default(),
        ),
        "shortcuts" => Some(
            dialog
                .and_downcast::<adw::ShortcutsDialog>()
                .map(|shortcuts| listed_by(&shortcuts))
                .unwrap_or_default(),
        ),
        _ => None,
    }
}

/// Adds an application action that puts a dialog in front of the window the reader is
/// in. Nothing happens when there is no window, which is the only state where there is
/// nowhere to put one.
fn present(app: &adw::Application, name: &str, raise: impl Fn(&adw::ApplicationWindow) + 'static) {
    let action = gio::SimpleAction::new(
        name.strip_prefix("app.").expect("an application action"),
        None,
    );
    let weak = app.downgrade();
    action.connect_activate(move |_, _| {
        if let Some(window) = weak
            .upgrade()
            .and_then(|app| app.active_window())
            .and_downcast::<adw::ApplicationWindow>()
        {
            raise(&window);
        }
    });
    app.add_action(&action);
}

/// The keyboard-shortcuts dialog, built from the table above.
///
/// Each row is made from its action rather than from its keys: libadwaita reads the
/// accelerators back out of the running application when it draws the row (probed on
/// libadwaita 1.8.6 — `AdwShortcutsItem:accelerator` stays empty while the drawn
/// `AdwShortcutLabel` carries `<Control>plus <Control>equal <Control>KP_Add`). So the
/// dialog shows the keys that are installed, not a second copy of them that could be
/// stale.
fn keyboard_shortcuts() -> adw::ShortcutsDialog {
    let dialog = adw::ShortcutsDialog::new();
    for (title, shortcuts) in SECTIONS {
        let section = adw::ShortcutsSection::new(Some(title));
        for shortcut in *shortcuts {
            section.add(adw::ShortcutsItem::from_action(
                shortcut.title,
                shortcut.action,
            ));
        }
        dialog.add(section);
    }
    dialog
}

/// The About dialog, in the metainfo's own words.
fn about() -> adw::AboutDialog {
    let dialog = adw::AboutDialog::new();
    dialog.set_application_name(text("name"));
    dialog.set_application_icon(text("id"));
    dialog.set_comments(text("summary"));
    dialog.set_developer_name(developer());
    dialog.set_version(env!("CARGO_PKG_VERSION"));
    dialog.set_license_type(license(text("project_license")));
    dialog.set_website(link("homepage"));
    dialog.set_issue_url(link("bugtracker"));
    dialog
}

/// The licence GTK names, from the one AppStream names.
///
/// Anything this does not know becomes [`gtk::License::Unknown`] rather than the
/// nearest guess: a dialog claiming a licence the project is not under is worse than
/// one that says nothing, and the test below is what keeps the two in step.
fn license(spdx: &str) -> gtk::License {
    match spdx {
        "GPL-3.0-or-later" => gtk::License::Gpl30,
        "GPL-3.0-only" => gtk::License::Gpl30Only,
        _ => gtk::License::Unknown,
    }
}

/// The text of the first `<tag>` in the metainfo.
///
/// Panics when it is not there, because the file is compiled into this binary: a
/// missing element is a broken build rather than something a reader can hit.
fn text(tag: &str) -> &'static str {
    element(METAINFO, tag).unwrap_or_else(|| panic!("the metainfo has no <{tag}>"))
}

/// Who wrote it: the `<name>` inside `<developer>`, which is not the application's own.
fn developer() -> &'static str {
    element(METAINFO, "developer")
        .and_then(|developer| element(developer, "name"))
        .expect("the metainfo names a developer")
}

/// The address AppStream files under `<url type="…">`.
fn link(kind: &'static str) -> &'static str {
    let opening = format!("<url type=\"{kind}\">");
    let (_, rest) = METAINFO
        .split_once(&opening)
        .unwrap_or_else(|| panic!("the metainfo has no {kind} url"));
    let (url, _) = rest
        .split_once("</url>")
        .unwrap_or_else(|| panic!("the metainfo's {kind} url is not closed"));
    url.trim()
}

/// The text between the first `<tag …>` and its `</tag>`, attributes and all.
///
/// The whole of the XML this needs to read: the metainfo is a file in this repository
/// with a validator of its own over it (`packaging.rs`), so what is wanted here is the
/// four elements the About dialog shows rather than a parser.
fn element<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let (_, rest) = xml.split_once(&format!("<{tag}"))?;
    let (_, inside) = rest.split_once('>')?;
    let (found, _) = inside.split_once(&format!("</{tag}>"))?;
    Some(found.trim())
}

/// What the About dialog on screen says, a field to a line.
fn said_by(about: &adw::AboutDialog) -> String {
    [
        ("name", about.application_name().to_string()),
        ("developer", about.developer_name().to_string()),
        ("version", about.version().to_string()),
        // The enumerated licence rather than its text: it is what decides both the
        // words on the dialog's own legal page and the link beside them.
        ("license", format!("{:?}", about.license_type())),
        ("website", about.website().to_string()),
        ("issues", about.issue_url().to_string()),
    ]
    .map(|(field, said)| format!("{field} {said}"))
    .join("\n")
}

/// What the keyboard-shortcuts dialog on screen lists: every row the reader is looking
/// at, as `title<TAB>keys`.
///
/// Only the page they are looking at. The dialog keeps a second copy of every row for
/// its own search results (probed on libadwaita 1.8.6: the `AdwViewStack` holds the
/// list, a search page and an empty-search page, each list holding the same rows), and
/// counting those would report every shortcut twice.
fn listed_by(dialog: &adw::ShortcutsDialog) -> String {
    let page = find(dialog.upcast_ref::<gtk::Widget>(), &|widget| {
        widget.downcast_ref::<adw::ViewStack>().is_some()
    })
    .and_then(|stack| stack.downcast::<adw::ViewStack>().ok())
    .and_then(|stack| stack.visible_child())
    .unwrap_or_else(|| dialog.clone().upcast());

    let mut rows = Vec::new();
    collect(&page, &mut rows);
    rows.join("\n")
}

/// Every shortcut row under `widget`, in the order the reader reads them.
///
/// A row is found by the `AdwShortcutLabel` that draws its keys; its title is the
/// first label beside that one (probed on libadwaita 1.8.6: an `AdwShortcutRow` holds
/// a box of title and subtitle next to the shortcut label, and the labels *inside* the
/// shortcut label are the individual keys).
fn collect(widget: &gtk::Widget, rows: &mut Vec<String>) {
    if let Some(keys) = widget.downcast_ref::<adw::ShortcutLabel>() {
        let title = widget
            .parent()
            .and_then(|row| beside(&row, widget))
            .unwrap_or_default();
        rows.push(format!("{title}\t{}", keys.accelerator()));
        return;
    }
    let mut child = widget.first_child();
    while let Some(candidate) = child {
        collect(&candidate, rows);
        child = candidate.next_sibling();
    }
}

/// The first thing said beside `keys` in the row holding it.
fn beside(row: &gtk::Widget, keys: &gtk::Widget) -> Option<String> {
    let mut child = row.first_child();
    while let Some(candidate) = child {
        if &candidate != keys
            && let Some(label) = find(&candidate, &|widget| {
                widget
                    .downcast_ref::<gtk::Label>()
                    .is_some_and(|label| !label.label().is_empty())
            })
        {
            return Some(label.downcast::<gtk::Label>().ok()?.label().to_string());
        }
        child = candidate.next_sibling();
    }
    None
}

/// The first widget in `widget` and everything under it that `wanted` accepts.
fn find(widget: &gtk::Widget, wanted: &dyn Fn(&gtk::Widget) -> bool) -> Option<gtk::Widget> {
    if wanted(widget) {
        return Some(widget.clone());
    }
    let mut child = widget.first_child();
    while let Some(candidate) = child {
        if let Some(found) = find(&candidate, wanted) {
            return Some(found);
        }
        child = candidate.next_sibling();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The About dialog is only as good as what it reads: every field it shows comes
    /// out of the metainfo, and an element that moved or was renamed would otherwise
    /// only be noticed by a reader looking at an empty dialog.
    #[test]
    fn the_about_dialog_reads_what_the_metainfo_says() {
        assert_eq!(text("id"), "io.github.etf.axiomd");
        // Lowercase on purpose, everywhere (`ux_decisions.md`).
        assert_eq!(text("name"), "axiomd");
        assert_eq!(text("summary"), "Read and write Markdown documents");
        assert_eq!(text("project_license"), "GPL-3.0-or-later");
        assert_eq!(developer(), "Illya Yalovyy");
        assert_eq!(link("homepage"), "https://github.com/IllyaYalovyy/axiomd");
        assert_eq!(
            link("bugtracker"),
            "https://github.com/IllyaYalovyy/axiomd/issues",
        );
        assert_eq!(license(text("project_license")), gtk::License::Gpl30);
    }

    /// The About dialog shows the version of the binary the reader is running, and the
    /// software centre shows the metainfo's newest release. A build whose two versions
    /// disagree tells the reader one thing and the desktop another.
    #[test]
    fn the_metainfo_names_the_version_this_binary_is() {
        let newest = METAINFO
            .split_once("<release version=\"")
            .and_then(|(_, rest)| rest.split_once('"'))
            .map(|(version, _)| version)
            .expect("the metainfo has a release");
        assert_eq!(newest, env!("CARGO_PKG_VERSION"));
    }
}
