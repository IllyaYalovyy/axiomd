//! axiomd — a fast, beautiful Markdown viewer for modern GNOME.

mod control;
mod document;
mod scheme;
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
