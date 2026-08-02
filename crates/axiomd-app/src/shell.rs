//! The application shell: everything between the process entry point and a
//! window on screen.
//!
//! Hides the GTK/libadwaita stack behind a single call. Each window builds its
//! own widget tree and owns its own state, so nothing is shared between windows.

use std::process::ExitCode;

use adw::prelude::*;
use gtk::glib;

/// Owner-chosen application id (2026-08-02). Also the D-Bus well-known name and
/// the base name of the desktop file, so it cannot change casually.
const APP_ID: &str = "io.github.etf.axiomd";

/// Runs axiomd to completion and reports the process exit status.
pub fn run() -> ExitCode {
    // Answered before touching GTK so that `axiomd --version` works over ssh,
    // in a container, or anywhere else without a display.
    if std::env::args_os().skip(1).any(|arg| arg == "--version") {
        println!("axiomd {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }

    let app = build_application();
    app.connect_activate(present_window);

    if app.run() == glib::ExitCode::SUCCESS {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn build_application() -> adw::Application {
    adw::Application::builder().application_id(APP_ID).build()
}

fn present_window(app: &adw::Application) {
    let content = adw::StatusPage::builder()
        .icon_name("text-x-generic-symbolic")
        .title("No document open")
        .description("Open a Markdown file to start reading.")
        .build();

    let layout = adw::ToolbarView::builder().content(&content).build();
    layout.add_top_bar(&adw::HeaderBar::new());

    adw::ApplicationWindow::builder()
        .application(app)
        .title("axiomd")
        .default_width(900)
        .default_height(700)
        .content(&layout)
        .build()
        .present();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The id is the app's identity on D-Bus, in the desktop file and to the
    /// session's single-instance handling; a silent change breaks all three.
    #[test]
    fn application_carries_the_owner_chosen_app_id() {
        let id = build_application().application_id();
        assert_eq!(id.as_deref(), Some("io.github.etf.axiomd"));
    }
}
