//! axiomd — a fast, beautiful Markdown editor and viewer for modern GNOME.

mod chrome;
mod control;
mod document;
mod editor;
mod export;
mod find;
mod links;
mod numbering;
mod outline;
mod places;
mod remote;
mod scheme;
mod settings;
mod shell;
#[cfg(test)]
mod testing;
mod view;
mod watch;
mod window;
mod zoom;

use std::process::ExitCode;

fn main() -> ExitCode {
    // First, before anything says a word: the reader's locale and axiomd's own message
    // catalogues, so that every string built from here on is built in their language
    // (issue #34). Ahead of GTK on purpose — GTK sets the locale when it starts, and
    // anything said before that would be said in the wrong one.
    axiomd_i18n::setup();
    shell::run()
}
