//! axiomd — a fast, beautiful Markdown editor and viewer for modern GNOME.

mod control;
mod document;
mod editor;
mod export;
mod links;
mod remote;
mod scheme;
mod settings;
mod shell;
#[cfg(test)]
mod testing;
mod view;
mod watch;
mod window;

use std::process::ExitCode;

fn main() -> ExitCode {
    shell::run()
}
