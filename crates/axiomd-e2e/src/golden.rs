//! Screenshot goldens: how "does it still look right" becomes a test.
//!
//! A rendered surface is approved by a human once. From then on the pinned image is
//! the specification and every run is diffed against it, so a change nobody approved
//! fails the suite with the actual picture and a difference map to look at.
//!
//! # Pinning is a human act
//!
//! The harness will not write a golden — not a new one, not a replacement — unless
//! `AXIOMD_PIN_GOLDENS` is set in the environment. That variable belongs to the
//! person reviewing the picture, in the same way the quality gate's skip variables
//! do: an agent may never set it, and the quality gate refuses to run with it set at
//! all (`scripts/quality.d/10-e2e.sh`). A failing visual test is therefore never
//! fixable by re-pinning from inside the machinery that failed.
//!
//! # Tolerance
//!
//! Snapshots taken through WebKit's own painter came back byte-identical across runs
//! and across processes when this was measured, so the tolerance is not there to
//! paper over noise. It is there so that a font-hinting or anti-aliasing difference
//! on an edge pixel is not read as a design change: a pixel counts as changed only if
//! a channel moved by more than [`CHANNEL_TOLERANCE`], and the picture counts as
//! changed only if more than [`CHANGED_PIXEL_BUDGET`] of it did.

use std::path::{Path, PathBuf};

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;

/// The environment variable that lets a human pin a golden. Never set by the harness.
const PIN: &str = "AXIOMD_PIN_GOLDENS";

/// How far one colour channel may move before the pixel counts as changed.
const CHANNEL_TOLERANCE: u8 = 8;

/// How much of the picture may be changed before the picture is.
const CHANGED_PIXEL_BUDGET: f64 = 0.001;

/// How far below a boundary [`Screenshot::shading_below`] looks, in rows.
///
/// Deep enough that the last row is past the reach of any shading Adwaita casts — its
/// raised toolbars blur over four pixels — because that row is the settled colour the
/// rest are measured against.
pub const SHADING_ROWS: usize = 12;

/// What the user would see, as pixels.
///
/// Held in the one layout every path here shares — `gdk`'s download format — so the
/// bytes of a golden read from disk and the bytes of a fresh capture are directly
/// comparable.
pub struct Screenshot {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

/// Whether a golden may be written. Read from the environment at the edge and passed
/// down, so the decision is a value the tests below can hold rather than a global.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pinning {
    /// A human asked for it.
    Allowed,
    /// Nobody did. This is every run that is not a human pinning a golden.
    Blocked,
}

impl Pinning {
    fn from_env() -> Pinning {
        Pinning::of(std::env::var_os(PIN))
    }

    /// Only an explicit `1` counts, so an empty or leftover value cannot silently
    /// arm the one thing a run must not be able to do by accident.
    fn of(value: Option<std::ffi::OsString>) -> Pinning {
        match value.as_deref().and_then(|value| value.to_str()) {
            Some("1") => Pinning::Allowed,
            _ => Pinning::Blocked,
        }
    }
}

impl Screenshot {
    /// Reads a captured or pinned picture from a PNG.
    pub(crate) fn read(path: &Path) -> Result<Screenshot, String> {
        let texture = gdk::Texture::from_filename(path)
            .map_err(|error| format!("read {}: {error}", path.display()))?;
        let (width, height) = (texture.width() as u32, texture.height() as u32);
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        texture.download(&mut pixels, (width * 4) as usize);
        Ok(Screenshot {
            width,
            height,
            pixels,
        })
    }

    /// The size of the captured surface, in pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Whether every pixel is the same colour — what a capture of a window that never
    /// drew looks like, and never what a rendered document looks like.
    pub fn is_blank(&self) -> bool {
        self.pixels
            .chunks_exact(4)
            .all(|pixel| pixel == &self.pixels[..4])
    }

    /// Whether this is the same picture as `other`, to the tolerance a golden is
    /// compared with.
    ///
    /// What lets a *change* be asserted without pinning anything: a surface that must
    /// redraw when the reader moves is tested by taking it twice and asking whether it
    /// moved, which needs nobody's approval and cannot go stale.
    pub fn looks_like(&self, other: &Screenshot) -> bool {
        if self.size() != other.size() {
            return false;
        }
        let budget = (self.pixels.len() / 4) as f64 * CHANGED_PIXEL_BUDGET;
        (self.changed_pixels(other) as f64) <= budget
    }

    /// How many pixels are drawn in `colour`, to the same tolerance a golden is
    /// compared with.
    ///
    /// What makes "it is drawn in the accent colour" a test rather than a claim: a
    /// stylesheet whose colour did not resolve leaves the surface the theme's own, which
    /// looks almost right and counts zero here.
    pub fn pixels_coloured(&self, colour: (u8, u8, u8)) -> usize {
        let (red, green, blue) = colour;
        self.pixels
            .chunks_exact(4)
            // The layout is the one `gdk` downloads in: blue, green, red, alpha.
            .filter(|pixel| {
                pixel[0].abs_diff(blue) <= CHANNEL_TOLERANCE
                    && pixel[1].abs_diff(green) <= CHANNEL_TOLERANCE
                    && pixel[2].abs_diff(red) <= CHANNEL_TOLERANCE
            })
            .count()
    }

    /// The `rows` rows of this picture starting at `from`, as a picture of their own.
    ///
    /// What lets a golden of a boundary be a golden of *the boundary*: pin the whole
    /// window and the document in it is pinned too, so every unrelated change to how a
    /// paragraph is drawn fails a test about the header's shadow and sends a human back
    /// to approve something nobody changed. A strip across the boundary holds the thing
    /// that was approved and nothing else.
    ///
    /// A strip that runs past the bottom of the picture stops there.
    pub fn band(&self, from: u32, rows: u32) -> Screenshot {
        let from = from.min(self.height);
        let height = rows.min(self.height - from);
        let row = (from * self.width) as usize * 4;
        Screenshot {
            width: self.width,
            height,
            pixels: self.pixels[row..row + (height * self.width) as usize * 4].to_vec(),
        }
    }

    /// How the content below `boundary` is shaded by whatever sits above it, read down
    /// the columns from `columns.0` up to `columns.1`.
    ///
    /// Entry `n` is how much darker row `boundary + n` is than the settled content
    /// further down, averaged across those columns and measured in channel steps: `0`
    /// where the content is already its own colour, and larger the deeper the shading.
    /// The profile is [`SHADING_ROWS`] long, and the last row of it is the settled
    /// colour every other row is measured against — so the columns handed in have to be
    /// ones the content is plain in, which is what makes averaging across them a
    /// measurement rather than a reading of whatever text happened to be there.
    ///
    /// What this makes testable is the difference between the three ways a bar can meet
    /// the content under it, which no other question here can tell apart: nothing at all
    /// is a profile of zeroes, a hard line is one shaded row and then the content, and a
    /// shadow fades over several. A screenshot golden says the picture changed; this
    /// says what it changed *to*, and needs nobody's approval to say it.
    pub fn shading_below(&self, boundary: u32, columns: (u32, u32)) -> Vec<u8> {
        let rows: Vec<f64> = (0..SHADING_ROWS)
            .map(|row| self.mean_luminance(boundary + row as u32, columns))
            .collect();
        let settled = rows[SHADING_ROWS - 1];
        rows.iter()
            .map(|row| (settled - row).clamp(0.0, 255.0).round() as u8)
            .collect()
    }

    /// How light row `y` is on average between `columns.0` and `columns.1`.
    ///
    /// A row past the bottom of the picture reads as the last row there is, rather than
    /// as black: the profile's own reference row can fall off a boundary measured near
    /// the foot of a window, and a reference of black would report every row above it as
    /// unshaded — a wrong answer that looks exactly like the right one.
    fn mean_luminance(&self, y: u32, columns: (u32, u32)) -> f64 {
        let (from, to) = (columns.0.min(self.width), columns.1.min(self.width));
        if self.height == 0 || from >= to {
            return 0.0;
        }
        let row = (y.min(self.height - 1) * self.width) as usize * 4;
        let total: u32 = (from..to)
            .map(|x| {
                let pixel = &self.pixels[row + x as usize * 4..][..3];
                u32::from(pixel[0]) + u32::from(pixel[1]) + u32::from(pixel[2])
            })
            .sum();
        f64::from(total) / f64::from((to - from) * 3)
    }

    /// Fails the test unless this is still the picture a human approved as `golden`.
    ///
    /// On a mismatch the captured picture and a map of what moved are written under
    /// `target/e2e-artifacts/` and named in the failure.
    pub fn assert_matches(&self, golden: &str) {
        let outcome = self.check(
            &goldens_dir().join(format!("{golden}.png")),
            &artifacts_dir(),
            Pinning::from_env(),
        );
        if let Err(complaint) = outcome {
            panic!("{complaint}");
        }
    }

    /// The whole golden policy, with the two things a test needs to vary — where the
    /// golden lives and whether pinning is allowed — passed in rather than read from
    /// the environment.
    fn check(&self, golden: &Path, artifacts: &Path, pinning: Pinning) -> Result<(), String> {
        let pinned = match Screenshot::read(golden) {
            Ok(pinned) => Some(pinned),
            Err(_) if golden.exists() => {
                return Err(format!("{} is not a readable PNG", golden.display()));
            }
            Err(_) => None,
        };

        let complaint = match &pinned {
            None => format!("no golden is pinned at {}", golden.display()),
            Some(pinned) if pinned.size() != self.size() => format!(
                "the rendered surface is {:?}, the golden {} is {:?}",
                self.size(),
                golden.display(),
                pinned.size()
            ),
            Some(pinned) => {
                let changed = self.changed_pixels(pinned);
                let budget = (self.pixels.len() / 4) as f64 * CHANGED_PIXEL_BUDGET;
                if (changed as f64) <= budget {
                    return Ok(());
                }
                format!(
                    "{changed} pixels of {} changed against the golden {} (budget {:.0})",
                    self.pixels.len() / 4,
                    golden.display(),
                    budget
                )
            }
        };

        if pinning == Pinning::Allowed {
            if let Some(parent) = golden.parent() {
                std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            self.write(golden)?;
            return Ok(());
        }

        let mut evidence = self.write_artifacts(artifacts, golden, pinned.as_ref())?;
        evidence.insert_str(
            0,
            &format!(
                "{complaint}.\nPinning a golden is a human decision: look at the pictures below \
                 and, if the new one is right, re-run with {PIN}=1 set. Nothing else may set it.\n"
            ),
        );
        Err(evidence)
    }

    /// How many pixels moved further than anti-aliasing explains.
    fn changed_pixels(&self, other: &Screenshot) -> usize {
        self.pixels
            .chunks_exact(4)
            .zip(other.pixels.chunks_exact(4))
            .filter(|(here, there)| {
                here.iter()
                    .zip(there.iter())
                    .any(|(a, b)| a.abs_diff(*b) > CHANNEL_TOLERANCE)
            })
            .count()
    }

    /// Writes what the run saw and what moved, and says where they are.
    fn write_artifacts(
        &self,
        artifacts: &Path,
        golden: &Path,
        pinned: Option<&Screenshot>,
    ) -> Result<String, String> {
        let name = golden
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "golden".to_owned());
        std::fs::create_dir_all(artifacts).map_err(|error| error.to_string())?;

        let actual = artifacts.join(format!("{name}.actual.png"));
        self.write(&actual)?;
        let mut evidence = format!("  captured: {}\n", actual.display());

        if let Some(pinned) = pinned.filter(|pinned| pinned.size() == self.size()) {
            let difference = artifacts.join(format!("{name}.diff.png"));
            self.difference_from(pinned).write(&difference)?;
            evidence.push_str(&format!("  what moved: {}\n", difference.display()));
        }
        evidence.push_str(&format!("  golden: {}\n", golden.display()));
        Ok(evidence)
    }

    /// A picture of the disagreement: changed pixels in red, the rest faded, so a
    /// human sees where to look rather than being handed two near-identical images.
    fn difference_from(&self, other: &Screenshot) -> Screenshot {
        let mut pixels = Vec::with_capacity(self.pixels.len());
        for (here, there) in self
            .pixels
            .chunks_exact(4)
            .zip(other.pixels.chunks_exact(4))
        {
            let moved = here
                .iter()
                .zip(there.iter())
                .any(|(a, b)| a.abs_diff(*b) > CHANNEL_TOLERANCE);
            if moved {
                pixels.extend_from_slice(&[0, 0, 255, 255]);
            } else {
                let faded = 128 + (here[0] / 4 + here[1] / 4 + here[2] / 4) / 3;
                pixels.extend_from_slice(&[faded, faded, faded, 255]);
            }
        }
        Screenshot {
            width: self.width,
            height: self.height,
            pixels,
        }
    }

    fn write(&self, path: &Path) -> Result<(), String> {
        let bytes = glib::Bytes::from_owned(self.pixels.clone());
        gdk::MemoryTexture::new(
            self.width as i32,
            self.height as i32,
            gdk::MemoryFormat::B8g8r8a8Premultiplied,
            &bytes,
            (self.width * 4) as usize,
        )
        .save_to_png(path)
        .map_err(|error| format!("write {}: {error}", path.display()))
    }
}

/// Where pinned goldens live: with the harness that owns the contract, so every
/// suite that uses it pins into one place.
fn goldens_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/goldens")
}

/// Where a failed comparison leaves its evidence: beside the build, never in the
/// source tree, so a failing run cannot look like a pending change to commit.
fn artifacts_dir() -> PathBuf {
    let executable = std::env::current_exe().expect("the running test binary");
    let build = executable
        .parent()
        .and_then(|deps| deps.parent())
        .expect("the build directory the test binary sits in");
    build.join("e2e-artifacts")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Scratch;

    /// A picture with enough structure that a change to it is a real change: a dark
    /// panel on a light field.
    fn picture() -> Screenshot {
        let (width, height) = (40u32, 30u32);
        let mut pixels = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let dark = (8..32).contains(&x) && (6..24).contains(&y);
                let value = if dark { 32 } else { 240 };
                pixels.extend_from_slice(&[value, value, value, 255]);
            }
        }
        Screenshot {
            width,
            height,
            pixels,
        }
    }

    /// The same picture as another renderer's anti-aliasing would draw it: every
    /// pixel nudged, none of them further than a hinting difference.
    fn anti_aliased_twin(of: &Screenshot) -> Screenshot {
        let pixels = of
            .pixels
            .chunks_exact(4)
            .enumerate()
            .flat_map(|(index, pixel)| {
                let nudge = CHANNEL_TOLERANCE - (index % CHANNEL_TOLERANCE as usize) as u8;
                [
                    pixel[0].saturating_sub(nudge),
                    pixel[1].saturating_sub(nudge),
                    pixel[2].saturating_add(nudge),
                    255,
                ]
            })
            .collect();
        Screenshot {
            width: of.width,
            height: of.height,
            pixels,
        }
    }

    /// The picture after somebody changed the design: the panel moved.
    fn perturbed(of: &Screenshot) -> Screenshot {
        let mut moved = Screenshot {
            width: of.width,
            height: of.height,
            pixels: of.pixels.clone(),
        };
        for y in 6..24u32 {
            for x in 10..34u32 {
                let start = ((y * of.width + x) * 4) as usize;
                moved.pixels[start..start + 4].copy_from_slice(&[32, 32, 32, 255]);
            }
        }
        moved
    }

    fn pin(scratch: &Scratch, name: &str, picture: &Screenshot) -> PathBuf {
        let golden = scratch.path().join(format!("{name}.png"));
        picture.write(&golden).expect("pin the golden");
        golden
    }

    #[test]
    fn a_picture_matches_the_golden_it_was_pinned_from() {
        let scratch = Scratch::new("golden-same");
        let golden = pin(&scratch, "same", &picture());

        picture()
            .check(&golden, &scratch.path().join("artifacts"), Pinning::Blocked)
            .expect("the same picture must match its golden");
    }

    /// Anti-aliasing and hinting move edge pixels a little on every machine. That is
    /// not a design change and must not read as one.
    #[test]
    fn an_anti_aliased_twin_is_still_the_approved_picture() {
        let scratch = Scratch::new("golden-twin");
        let golden = pin(&scratch, "twin", &picture());

        anti_aliased_twin(&picture())
            .check(&golden, &scratch.path().join("artifacts"), Pinning::Blocked)
            .expect("a twin within tolerance must match");
    }

    /// The tolerance must not be wide enough to hide a real change — and the failure
    /// must hand the human the pictures.
    #[test]
    fn a_changed_picture_fails_and_leaves_the_evidence() {
        let scratch = Scratch::new("golden-moved");
        let golden = pin(&scratch, "moved", &picture());
        let artifacts = scratch.path().join("artifacts");

        let complaint = perturbed(&picture())
            .check(&golden, &artifacts, Pinning::Blocked)
            .expect_err("a moved panel must not match");

        assert!(complaint.contains("pixels of"), "{complaint}");
        assert!(
            artifacts.join("moved.actual.png").exists(),
            "the captured picture was not written: {complaint}",
        );
        assert!(
            artifacts.join("moved.diff.png").exists(),
            "the difference map was not written: {complaint}",
        );
    }

    /// A golden of a different shape is a mismatch, not a comparison over the
    /// overlapping part.
    #[test]
    fn a_golden_of_another_size_is_a_mismatch() {
        let scratch = Scratch::new("golden-size");
        let golden = pin(&scratch, "size", &picture());
        let taller = Screenshot {
            width: 40,
            height: 31,
            pixels: vec![240; 40 * 31 * 4],
        };

        let complaint = taller
            .check(&golden, &scratch.path().join("artifacts"), Pinning::Blocked)
            .expect_err("a differently-sized capture must not match");

        assert!(complaint.contains("is (40, 30)"), "{complaint}");
    }

    /// The rule the whole scheme rests on: nothing pins a golden but a human.
    #[test]
    fn a_missing_golden_is_never_pinned_without_a_human() {
        let scratch = Scratch::new("golden-unpinned");
        let golden = scratch.path().join("never.png");

        let complaint = picture()
            .check(&golden, &scratch.path().join("artifacts"), Pinning::Blocked)
            .expect_err("an unpinned golden must fail rather than appear");

        assert!(complaint.contains(PIN), "{complaint}");
        assert!(
            !golden.exists(),
            "the harness pinned a golden nobody approved",
        );
    }

    /// A failing comparison must not be able to overwrite the approved picture.
    #[test]
    fn a_failing_comparison_never_replaces_the_golden() {
        let scratch = Scratch::new("golden-keep");
        let golden = pin(&scratch, "keep", &picture());
        let approved = std::fs::read(&golden).expect("read the pinned golden");

        perturbed(&picture())
            .check(&golden, &scratch.path().join("artifacts"), Pinning::Blocked)
            .expect_err("a moved panel must not match");

        assert_eq!(
            std::fs::read(&golden).expect("read the golden again"),
            approved,
            "the golden was rewritten by a failing comparison",
        );
    }

    /// And with the human's approval, pinning works — otherwise the escape hatch the
    /// failure message points at would be a dead end.
    #[test]
    fn a_human_can_pin_a_new_picture() {
        let scratch = Scratch::new("golden-pin");
        let golden = scratch.path().join("fresh.png");

        picture()
            .check(&golden, &scratch.path().join("artifacts"), Pinning::Allowed)
            .expect("pinning must succeed when a human asked");

        assert!(golden.exists(), "no golden was written");
        picture()
            .check(&golden, &scratch.path().join("artifacts"), Pinning::Blocked)
            .expect("the picture that was just pinned must now match");
    }

    /// Only an explicit `1` arms pinning: an empty or leftover value must not.
    #[test]
    fn pinning_is_off_unless_a_human_asks_for_it_exactly() {
        use std::ffi::OsString;

        assert_eq!(Pinning::of(None), Pinning::Blocked);
        assert_eq!(Pinning::of(Some(OsString::from(""))), Pinning::Blocked);
        assert_eq!(Pinning::of(Some(OsString::from("0"))), Pinning::Blocked);
        assert_eq!(Pinning::of(Some(OsString::from("true"))), Pinning::Blocked);
        assert_eq!(Pinning::of(Some(OsString::from("1"))), Pinning::Allowed);
    }

    /// The colour count, which is how a test says "drawn in the accent colour" without
    /// a human having to look.
    #[test]
    fn the_pixels_of_one_colour_are_counted_and_no_others_are() {
        // The picture is a dark panel on a light field: 24 x 18 dark pixels.
        assert_eq!(picture().pixels_coloured((32, 32, 32)), 24 * 18);
        assert_eq!(
            picture().pixels_coloured((240, 240, 240)),
            40 * 30 - 24 * 18
        );
        assert_eq!(picture().pixels_coloured((53, 132, 228)), 0);
        // And the channels are counted in the order a caller writes a colour in, not
        // the order the bytes happen to be in: a picture of accent blue is found by
        // asking for accent blue and never by asking for its mirror.
        let accent = Screenshot {
            width: 2,
            height: 1,
            pixels: vec![0xe4, 0x84, 0x35, 255, 0xe4, 0x84, 0x35, 255],
        };
        assert_eq!(accent.pixels_coloured((0x35, 0x84, 0xe4)), 2);
        assert_eq!(accent.pixels_coloured((0xe4, 0x84, 0x35)), 0);
        // And a channel nudged within tolerance is still that colour, as it is for a
        // golden.
        assert_eq!(
            anti_aliased_twin(&picture()).pixels_coloured((32, 32, 32)),
            24 * 18,
        );
    }

    /// The unpinned comparison, which a test of a surface that must redraw when the
    /// reader moves rests on: the same tolerance as a golden, and no approval.
    #[test]
    fn two_pictures_are_the_same_picture_exactly_when_a_golden_would_say_so() {
        assert!(picture().looks_like(&picture()));
        assert!(
            picture().looks_like(&anti_aliased_twin(&picture())),
            "a hinting difference must not read as a redraw",
        );
        assert!(
            !picture().looks_like(&perturbed(&picture())),
            "a moved panel must read as a redraw",
        );
        assert!(
            !picture().looks_like(&Screenshot {
                width: 40,
                height: 31,
                pixels: vec![240; 40 * 31 * 4],
            }),
            "a picture of another size is not the same picture",
        );
    }

    /// A capture of a window that never drew is all one colour, and a test that
    /// asserted only "a picture came back" would pass on it.
    #[test]
    fn a_blank_capture_is_recognised_as_blank() {
        let blank = Screenshot {
            width: 4,
            height: 4,
            pixels: vec![255; 4 * 4 * 4],
        };

        assert!(blank.is_blank());
        assert!(!picture().is_blank());
    }

    /// A surface meeting a bar above it, shaded `profile` deep and its own colour under
    /// that — the three ways a boundary can be drawn, made into pictures.
    fn boundary(profile: &[u8]) -> Screenshot {
        let (width, height) = (20u32, 20u32);
        let mut pixels = Vec::new();
        for y in 0..height {
            let shade = profile.get(y as usize).copied().unwrap_or(0);
            for _ in 0..width {
                let value = 240 - shade;
                pixels.extend_from_slice(&[value, value, value, 255]);
            }
        }
        Screenshot {
            width,
            height,
            pixels,
        }
    }

    /// The measurement the boundary suite rests on, held to the three cases it exists to
    /// tell apart. Read wrong in either direction it would pass a hairline as a shadow
    /// or fail a shadow as nothing, and the suite would say so about the application.
    #[test]
    fn the_shading_below_a_boundary_tells_a_shadow_from_a_line_and_from_nothing() {
        let rows = |profile: &[u8]| boundary(profile).shading_below(0, (0, 20));

        assert_eq!(
            rows(&[]),
            vec![0; SHADING_ROWS],
            "bare content is not shaded"
        );
        assert_eq!(
            rows(&[16]),
            [16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            "a hard line is one shaded row and then the content",
        );
        assert_eq!(
            rows(&[24, 12, 6, 2]),
            [24, 12, 6, 2, 0, 0, 0, 0, 0, 0, 0, 0],
            "a shadow is read as the fade it is",
        );
    }

    /// Read from further down, the same picture says what is under *that* row — which is
    /// how one capture answers for the header, a banner and a search bar at once.
    #[test]
    fn the_shading_is_measured_from_the_row_it_is_asked_about() {
        let shadow = boundary(&[24, 12, 6, 2]);

        assert_eq!(shadow.shading_below(1, (0, 20))[..3], [12, 6, 2]);
        assert_eq!(shadow.shading_below(4, (0, 20)), vec![0; SHADING_ROWS]);
        // And a row past the bottom of the picture is unshaded rather than a panic.
        assert_eq!(shadow.shading_below(19, (0, 20)), vec![0; SHADING_ROWS]);
        assert_eq!(shadow.shading_below(40, (0, 20)), vec![0; SHADING_ROWS]);
    }

    /// Columns are read as asked and no others, so a test can keep the window's corners
    /// and its scrollbar out of a measurement of the boundary between them.
    #[test]
    fn only_the_columns_asked_for_are_measured() {
        let (width, height) = (20u32, 4u32);
        let mut pixels = Vec::new();
        for y in 0..height {
            for x in 0..width {
                // Shaded on the left half of the top row only.
                let value = if y == 0 && x < 10 { 200 } else { 240 };
                pixels.extend_from_slice(&[value, value, value, 255]);
            }
        }
        let lopsided = Screenshot {
            width,
            height,
            pixels,
        };

        assert_eq!(lopsided.shading_below(0, (0, 10))[0], 40);
        assert_eq!(lopsided.shading_below(0, (10, 20))[0], 0);
        // Half in and half out, averaged: the reason the columns handed in have to be
        // ones the content is plain in.
        assert_eq!(lopsided.shading_below(0, (0, 20))[0], 20);
    }

    /// A band is the rows it was asked for and nothing else — the whole of what keeps a
    /// golden of a boundary from being a golden of the document beneath it.
    #[test]
    fn a_band_is_the_rows_of_the_picture_it_was_cut_from() {
        let whole = picture();
        let band = whole.band(6, 18);

        assert_eq!(band.size(), (40, 18));
        // Rows 6..24 of the source are the dark panel's rows, so the band is the panel
        // from edge to edge and nothing above or below it.
        assert_eq!(band.pixels_coloured((32, 32, 32)), 24 * 18);
        assert_eq!(band.pixels_coloured((240, 240, 240)), 40 * 18 - 24 * 18);
        // Two bands of the same rows are the same picture, which is what pinning one
        // means; a band of other rows is not.
        assert!(band.looks_like(&picture().band(6, 18)));
        assert!(!band.looks_like(&whole.band(0, 18)));
    }

    /// A band running off the bottom stops at the bottom, so a boundary near the foot of
    /// a window is still a picture rather than a panic.
    #[test]
    fn a_band_past_the_bottom_stops_at_the_bottom() {
        let whole = picture();

        assert_eq!(whole.band(24, 18).size(), (40, 6));
        assert_eq!(whole.band(30, 18).size(), (40, 0));
        assert_eq!(whole.band(99, 18).size(), (40, 0));
    }
}
