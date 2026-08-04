//! How big the document is, and the four ways the reader says so.
//!
//! One window's zoom, held by that window and no other (invariant 7), lasting exactly
//! as long as the window does: it is a way of looking at what is on screen now, not a
//! preference, so nothing is written down and a new window opens at 100%
//! (`designs/MVP-USER-TASKS.md`, UT-011).
//!
//! # One ladder, four ways up it
//!
//! `Ctrl+plus`, `Ctrl+minus` and `Ctrl+0`; the two buttons in the primary menu;
//! `Ctrl` with the scroll wheel; and a pinch on a touchpad. All four move along the
//! same [`LADDER`] and end in the same call, so there is one definition of what a step
//! is and one place a bound is enforced — the ends of the ladder, where the step that
//! would leave it simply is not offered.
//!
//! # What zoom costs the document
//!
//! Nothing but a relayout. WebKit's own zoom level scales text *and* layout — measured
//! on WebKitGTK 2.52.5: at 1.5 an 800px viewport reports a `clientWidth` of 533 and a
//! `devicePixelRatio` of 1.5 — so the reader gets a bigger document rather than a
//! magnified picture of one, and the parser, the renderer and the page load are all
//! untouched (invariant 9). The measure the reading width holds text to is in rem, so
//! it scales with the text and the column keeps its proportions.
//!
//! # What the reader sees, and what a test sees
//!
//! The level is a button in the primary menu carrying the words `125%`, between the
//! two that change it; pressing it is `Ctrl+0`. That button is the whole of the
//! module's visible state, and it is what the test-control channel reads back — so a
//! test asserts the words the reader is looking at rather than a number beside them.
//!
//! The one thing no test here drives is a real input device: a headless compositor has
//! no pointer and no touchpad, and GTK 4 offers no way to inject one. So the scroll
//! wheel and the pinch arrive at [`Zoom::scrolled`] and [`Zoom::pinched`] — the calls
//! the two controllers make, with the values they pass — and the tests come in there,
//! exactly as pressing a button in this suite emits the button's own `clicked` signal
//! (`control.rs`). What that leaves untested is the two closures below that read the
//! modifier state and the gesture scale; everything they decide is covered.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use axiomd_i18n::gettext;
use gtk::gdk;
use gtk::gio;
use webkit6::prelude::WebViewExt;

/// The steps a document is scaled in, as percentages, from the smallest the reader may
/// ask for to the largest (issue #10: 50% to 200%).
const LADDER: [u32; 10] = [50, 67, 80, 90, 100, 110, 125, 150, 175, 200];

/// Where on the ladder a window starts.
const HUNDRED_PERCENT: usize = 4;

/// How far a pinch has to travel before it is another step. Below this a resting hand
/// would walk the document up and down the ladder.
const PINCH_STEP: f64 = 1.2;

/// The zoom actions, named once. A widget addresses an action by its full name and a
/// window registers it by its bare one (`window.rs`).
pub(crate) const IN: &str = "win.zoom-in";
pub(crate) const OUT: &str = "win.zoom-out";
pub(crate) const RESET: &str = "win.zoom-reset";

/// One window's zoom.
pub(crate) struct Zoom {
    view: webkit6::WebView,
    /// Where on [`LADDER`] the document is.
    at: Cell<usize>,
    /// The button in the primary menu that says so and resets it. The single piece of
    /// visible state this module has.
    level: gtk::Button,
    row: gtk::Box,
    /// The two steps, held so that the one at the end of the ladder can stop being
    /// offered — which is what makes the bound something the reader can see rather
    /// than a press that does nothing.
    steps: [gio::SimpleAction; 2],
    /// The gesture scale the last pinch step was taken at.
    pinched_at: Cell<f64>,
}

impl Zoom {
    /// Gives `window` its zoom: the three actions, the two gestures over `view`, and
    /// the row for the primary menu.
    pub(crate) fn attach(window: &adw::ApplicationWindow, view: &webkit6::WebView) -> Rc<Zoom> {
        let level = gtk::Button::builder().action_name(RESET).build();
        level.add_css_class("flat");
        crate::chrome::name(&level, &gettext("Reset Zoom"));
        // A fixed width, so walking the ladder does not shuffle the buttons beside it.
        level.set_width_request(72);

        let steps = [
            gio::SimpleAction::new(bare(OUT), None),
            gio::SimpleAction::new(bare(IN), None),
        ];
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .build();

        let zoom = Rc::new(Zoom {
            view: view.clone(),
            at: Cell::new(HUNDRED_PERCENT),
            level,
            row,
            steps,
            pinched_at: Cell::new(1.0),
        });

        zoom.row
            .append(&step_button("zoom-out-symbolic", &gettext("Zoom Out"), OUT));
        zoom.row.append(&zoom.level);
        zoom.row
            .append(&step_button("zoom-in-symbolic", &gettext("Zoom In"), IN));

        for (action, towards) in zoom.steps.iter().zip([Step::Out, Step::In]) {
            let stepping = Rc::downgrade(&zoom);
            action.connect_activate(move |_, _| {
                if let Some(zoom) = stepping.upgrade() {
                    zoom.step(towards);
                }
            });
            window.add_action(action);
        }

        let reset = gio::SimpleAction::new(bare(RESET), None);
        let resetting = Rc::downgrade(&zoom);
        reset.connect_activate(move |_, _| {
            if let Some(zoom) = resetting.upgrade() {
                zoom.settle(HUNDRED_PERCENT);
            }
        });
        window.add_action(&reset);

        // Capture, so the wheel is answered before the page it is over gets it: a
        // document that scrolled *and* zoomed on the same turn of the wheel is the bug
        // this phase exists to prevent.
        let wheel = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        wheel.set_propagation_phase(gtk::PropagationPhase::Capture);
        let scrolling = Rc::downgrade(&zoom);
        wheel.connect_scroll(move |wheel, _, delta| {
            let Some(zoom) = scrolling.upgrade() else {
                return glib_propagation(false);
            };
            let held = wheel
                .current_event_state()
                .contains(gdk::ModifierType::CONTROL_MASK);
            glib_propagation(zoom.scrolled(delta, held))
        });
        view.add_controller(wheel);

        let pinch = gtk::GestureZoom::new();
        let beginning = Rc::downgrade(&zoom);
        pinch.connect_begin(move |_, _| {
            if let Some(zoom) = beginning.upgrade() {
                zoom.pinched_at.set(1.0);
            }
        });
        let pinching = Rc::downgrade(&zoom);
        pinch.connect_scale_changed(move |_, scale| {
            if let Some(zoom) = pinching.upgrade() {
                zoom.pinched(scale);
            }
        });
        view.add_controller(pinch);

        zoom.settle(HUNDRED_PERCENT);
        zoom
    }

    /// The row the primary menu shows: what the document is scaled to, and the two
    /// buttons that change it.
    pub(crate) fn indicator(&self) -> &gtk::Widget {
        self.row.upcast_ref()
    }

    /// Answers what the reader can see of the zoom, or `None` for a question about
    /// something else. `zoom` is the words on the button in the menu.
    pub(crate) fn shown(&self, name: &str) -> Option<String> {
        match name {
            "zoom" => Some(self.level.label().unwrap_or_default().to_string()),
            _ => None,
        }
    }

    /// A turn of the scroll wheel over the document, with `control` saying whether the
    /// reader was holding Ctrl. Answers whether the document was zoomed — and so
    /// whether the page must not also scroll by it.
    pub(crate) fn scrolled(&self, delta: f64, control: bool) -> bool {
        if !control || delta == 0.0 {
            return false;
        }
        // Up the page is up the ladder, which is how a wheel has zoomed since it had
        // a notch.
        self.step(if delta < 0.0 { Step::In } else { Step::Out });
        true
    }

    /// A pinch on a touchpad, `scale` being how far it has spread since it began.
    pub(crate) fn pinched(&self, scale: f64) {
        if scale <= 0.0 {
            return;
        }
        let travelled = scale / self.pinched_at.get();
        if travelled >= PINCH_STEP {
            self.pinched_at.set(scale);
            self.step(Step::In);
        } else if travelled <= 1.0 / PINCH_STEP {
            self.pinched_at.set(scale);
            self.step(Step::Out);
        }
    }

    /// One step along the ladder, or nothing at all at the end of it.
    fn step(&self, towards: Step) {
        let at = match towards {
            Step::In => self.at.get() + 1,
            Step::Out => self.at.get().wrapping_sub(1),
        };
        if at < LADDER.len() {
            self.settle(at);
        }
    }

    /// Puts the document at `at` on the ladder and says so everywhere it is said.
    fn settle(&self, at: usize) {
        self.at.set(at);
        let percent = LADDER[at];
        self.view.set_zoom_level(f64::from(percent) / 100.0);
        // TRANSLATORS: how big the document is drawn, as a percentage of its normal
        // size — the whole phrase, so a language that writes a percentage another way
        // can.
        self.level
            .set_label(&gettext("{percent}%").replace("{percent}", &percent.to_string()));
        self.steps[0].set_enabled(at > 0);
        self.steps[1].set_enabled(at + 1 < LADDER.len());
    }
}

/// Which way along the ladder.
#[derive(Clone, Copy)]
enum Step {
    In,
    Out,
}

/// One of the two step buttons, bound to its action — so GTK makes it insensitive
/// exactly when the document is already as big or as small as it goes.
fn step_button(icon: &str, saying: &str, action: &str) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name(icon)
        .action_name(action)
        .build();
    button.add_css_class("flat");
    crate::chrome::name(&button, saying);
    button
}

fn glib_propagation(handled: bool) -> gtk::glib::Propagation {
    match handled {
        true => gtk::glib::Propagation::Stop,
        false => gtk::glib::Propagation::Proceed,
    }
}

/// An action's bare name, as a window registers it, from the full one a widget uses.
fn bare(action: &str) -> &str {
    action.strip_prefix("win.").unwrap_or(action)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The range issue #10 states, and the shape the ladder has to have for a step to
    /// mean anything: increasing, and standing on 100% so that `Ctrl+0` is a rung
    /// rather than a place between two.
    #[test]
    fn the_ladder_runs_from_half_size_to_double_through_100_percent() {
        assert_eq!(LADDER[0], 50);
        assert_eq!(LADDER[LADDER.len() - 1], 200);
        assert_eq!(LADDER[HUNDRED_PERCENT], 100);
        for pair in LADDER.windows(2) {
            assert!(pair[0] < pair[1], "the ladder goes backwards at {pair:?}");
        }
    }

    /// A zoom action registered under its full name would leave every accelerator and
    /// every button in the menu silently doing nothing.
    #[test]
    fn every_zoom_action_is_registered_under_the_name_its_shortcut_uses() {
        for action in [IN, OUT, RESET] {
            assert!(
                action.starts_with("win."),
                "{action} is not a window action"
            );
            assert_eq!(bare(action), &action["win.".len()..]);
        }
    }
}
