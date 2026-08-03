//! axiomd — a fast, beautiful Markdown editor and viewer for modern GNOME.

mod control;
mod document;
mod editor;
mod export;
mod find;
mod links;
mod outline;
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
    shell::run()
}
