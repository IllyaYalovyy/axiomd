//! axiomd — a fast, beautiful Markdown viewer for modern GNOME.

mod shell;

use std::process::ExitCode;

fn main() -> ExitCode {
    shell::run()
}
