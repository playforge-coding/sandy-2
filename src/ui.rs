//! The on-screen controls, built with [egui](https://docs.rs/egui) (immediate
//! mode).
//!
//! egui renders as its own pass on top of the simulation (see
//! [`crate::gpu::State::render`]), so — unlike the old hand-rolled overlay that
//! stamped pixels into the sand buffer — nothing here touches the grid. The
//! panel is a material picker, a brush-size slider, and seed entry plus
//! world-generation buttons. Keyboard shortcuts in [`crate::app`] drive the
//! very same [`Controls`], so the two stay in sync.

use egui::{Color32, RichText, Stroke};

use crate::materials::{self, MaterialId};

/// Longest seed the user can type — keeps the value comfortably within `u32`.
const MAX_SEED_DIGITS: usize = 7;

/// Control state shared between the egui panel and the keyboard shortcuts in
/// [`crate::app`]. egui reads and writes it in place each frame; `app` pokes the
/// same fields from key handlers.
pub struct Controls {
    /// Material the brush paints with.
    pub material: MaterialId,
    /// Brush radius, in grid cells.
    pub brush: i32,
    /// The world seed, as text so it can be edited in a box. Parsed to a `u32`
    /// when a world is actually built (see [`Controls::seed_value`]).
    pub seed: String,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            material: 1, // Sand
            brush: 4,
            seed: crate::worldgen::DEFAULT_SEED.to_string(),
        }
    }
}

impl Controls {
    /// The seed as a number (`0` if the box is empty or not a valid `u32`).
    pub fn seed_value(&self) -> u32 {
        self.seed.trim().parse().unwrap_or(0)
    }

    /// Replace the seed text (used by the "random seed" shortcut/button).
    pub fn set_seed(&mut self, seed: u32) {
        self.seed = seed.to_string();
    }
}

/// World-generation requests raised by the panel this frame. The keyboard
/// shortcuts in [`crate::app`] perform the same actions directly, so this only
/// carries what the *buttons* asked for.
#[derive(Default)]
pub struct Actions {
    pub clear: bool,
    pub generate: bool,
    pub randomize: bool,
}

/// Build the control panel for this frame and report which buttons were hit.
pub fn draw(ctx: &egui::Context, c: &mut Controls) -> Actions {
    let mut actions = Actions::default();

    // On a narrow viewport (phones) lay the panel out for fingertips: bigger
    // touch targets and a wider, fixed panel so buttons don't shrink to a
    // hairline. On a roomy desktop window, keep the original compact layout.
    let compact = ctx.screen_rect().width() < 550.0;
    let button_size = if compact {
        egui::vec2(200.0, 38.0)
    } else {
        egui::vec2(130.0, 18.0)
    };

    egui::Window::new("Sandy")
        .default_pos([8.0, 8.0])
        .resizable(false)
        .show(ctx, |ui| {
            if compact {
                // Roomier spacing and taller sliders/text fields for touch.
                let s = ui.spacing_mut();
                s.interact_size.y = 32.0;
                s.item_spacing = egui::vec2(8.0, 8.0);
                s.slider_width = 160.0;
            }

            ui.label("Material");
            for id in 0..materials::count() as MaterialId {
                let mat = materials::get(id);
                // Some materials (rain) are spawned by others and never painted.
                if !mat.pickable() {
                    continue;
                }
                let info = mat.info();
                let fill = to_color32(info.average_color());
                let mut btn = egui::Button::new(RichText::new(info.name).color(contrast(fill)))
                    .fill(fill)
                    .min_size(button_size);
                if id == c.material {
                    btn = btn.stroke(Stroke::new(2.0, Color32::WHITE));
                }
                if ui.add(btn).clicked() {
                    c.material = id;
                }
            }

            ui.separator();
            ui.add(egui::Slider::new(&mut c.brush, 0..=40).text("Brush"));

            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Seed");
                let edit = egui::TextEdit::singleline(&mut c.seed)
                    .char_limit(MAX_SEED_DIGITS)
                    .desired_width(70.0);
                let resp = ui.add(edit);
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    actions.generate = true;
                }
            });
            // Keep the field numeric so it always parses to a seed.
            c.seed.retain(|ch| ch.is_ascii_digit());

            ui.horizontal(|ui| {
                actions.generate |= ui.button("Generate").clicked();
                actions.randomize |= ui.button("Random").clicked();
                actions.clear |= ui.button("Clear").clicked();
            });

            ui.separator();
            ui.label(
                RichText::new("Hold left-mouse to draw · drag a .rhai file to add a material")
                    .small()
                    .weak(),
            );
        });

    actions
}

/// A material's swatch colour as an opaque egui [`Color32`] (the stored alpha is
/// the renderer's glow flag, not real transparency, so force it opaque here).
fn to_color32(c: [u8; 4]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// Pick black or white text for readability over a swatch colour (Rec. 601
/// luma).
fn contrast(c: Color32) -> Color32 {
    let luma = 0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32;
    if luma > 140.0 {
        Color32::BLACK
    } else {
        Color32::WHITE
    }
}
