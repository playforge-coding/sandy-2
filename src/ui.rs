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

use crate::entities::{self, EntityKindId};
use crate::materials::{self, MaterialId};
use crate::worldgen::WorldType;

/// Longest seed the user can type — keeps the value comfortably within `u32`.
const MAX_SEED_DIGITS: usize = 7;

/// What the brush does when the user drags. Most of the time it paints the
/// selected material; the wind tool instead blows a gust in the drag direction
/// (see [`crate::sim::Simulation::add_wind_disk`]) without placing any cells.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    /// Paint the selected [`Controls::material`].
    Paint,
    /// Blow wind the way the cursor is swept.
    Wind,
    /// Summon a meteor at the clicked spot (see
    /// [`crate::sim::Simulation::spawn_meteor`]).
    Meteor,
    /// Summon a tsunami headed toward the clicked spot (see
    /// [`crate::sim::Simulation::spawn_tsunami`]).
    Tsunami,
    /// Call down a gamma-ray burst on the clicked column (see
    /// [`crate::sim::Simulation::spawn_gamma_burst`]).
    GammaBurst,
    /// Place a creature of [`Controls::entity`] at the clicked spot (see
    /// [`crate::sim::Simulation::spawn_entity`]).
    Creature,
}

/// Control state shared between the egui panel and the keyboard shortcuts in
/// [`crate::app`]. egui reads and writes it in place each frame; `app` pokes the
/// same fields from key handlers.
pub struct Controls {
    /// What a drag does — paint, or blow wind.
    pub tool: Tool,
    /// Material the brush paints with (when [`tool`] is [`Tool::Paint`]).
    ///
    /// [`tool`]: Controls::tool
    pub material: MaterialId,
    /// Creature the placement tool drops (when [`tool`] is [`Tool::Creature`]).
    ///
    /// [`tool`]: Controls::tool
    pub entity: EntityKindId,
    /// Brush radius, in grid cells. Doubles as the wind tool's gust radius.
    pub brush: i32,
    /// The world seed, as text so it can be edited in a box. Parsed to a `u32`
    /// when a world is actually built (see [`Controls::seed_value`]).
    pub seed: String,
    /// Which landscape preset the next generation builds.
    pub world: WorldType,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            tool: Tool::Paint,
            material: 1, // Sand
            entity: crate::entities::ANT,
            brush: 4,
            seed: crate::worldgen::DEFAULT_SEED.to_string(),
            world: WorldType::default(),
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
                // Highlight the active material only while the paint tool is the
                // one in use, so it's clear at a glance whether a drag paints.
                if id == c.material && c.tool == Tool::Paint {
                    btn = btn.stroke(Stroke::new(2.0, Color32::WHITE));
                }
                if ui.add(btn).clicked() {
                    c.material = id;
                    c.tool = Tool::Paint; // picking a material returns to painting
                }
            }

            ui.separator();
            // The wind tool: sweep the cursor to blow a gust that way.
            let mut wind_btn = egui::Button::new("💨 Wind").min_size(button_size);
            if c.tool == Tool::Wind {
                wind_btn = wind_btn.stroke(Stroke::new(2.0, Color32::WHITE));
            }
            if ui.add(wind_btn).clicked() {
                c.tool = Tool::Wind;
            }

            // The meteor tool: click anywhere to call down a meteor on that spot.
            let mut meteor_btn = egui::Button::new("☄ Meteor").min_size(button_size);
            if c.tool == Tool::Meteor {
                meteor_btn = meteor_btn.stroke(Stroke::new(2.0, Color32::WHITE));
            }
            if ui.add(meteor_btn).clicked() {
                c.tool = Tool::Meteor;
            }

            // The tsunami tool: click to send a wave rolling across the world.
            let mut tsunami_btn = egui::Button::new("🌊 Tsunami").min_size(button_size);
            if c.tool == Tool::Tsunami {
                tsunami_btn = tsunami_btn.stroke(Stroke::new(2.0, Color32::WHITE));
            }
            if ui.add(tsunami_btn).clicked() {
                c.tool = Tool::Tsunami;
            }

            // The gamma-ray-burst tool: click to annihilate a column from the sky.
            let mut grb_btn = egui::Button::new("☢ Gamma Ray").min_size(button_size);
            if c.tool == Tool::GammaBurst {
                grb_btn = grb_btn.stroke(Stroke::new(2.0, Color32::WHITE));
            }
            if ui.add(grb_btn).clicked() {
                c.tool = Tool::GammaBurst;
            }

            ui.separator();
            // Creatures: pick one, then click in the world to drop it there.
            ui.label("Creatures");
            for id in 0..entities::count() as EntityKindId {
                let info = entities::get(id).info();
                let fill = to_color32(info.color);
                let mut btn = egui::Button::new(RichText::new(info.name).color(contrast(fill)))
                    .fill(fill)
                    .min_size(button_size);
                if id == c.entity && c.tool == Tool::Creature {
                    btn = btn.stroke(Stroke::new(2.0, Color32::WHITE));
                }
                if ui.add(btn).clicked() {
                    c.entity = id;
                    c.tool = Tool::Creature;
                }
            }

            ui.separator();
            let brush_label = if c.tool == Tool::Wind {
                "Gust size"
            } else {
                "Brush"
            };
            ui.add(egui::Slider::new(&mut c.brush, 0..=40).text(brush_label));

            ui.separator();
            // World preset. Switching it regenerates the world right away so the
            // change is visible without a second click.
            ui.horizontal(|ui| {
                ui.label("World");
                let before = c.world;
                egui::ComboBox::from_id_salt("world_type")
                    .selected_text(c.world.name())
                    .show_ui(ui, |ui| {
                        for w in WorldType::ALL {
                            ui.selectable_value(&mut c.world, w, w.name());
                        }
                    });
                if c.world != before {
                    actions.generate = true;
                }
            });

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
                RichText::new(
                    "Hold left-mouse to draw · pick Wind and sweep to blow a gust · \
                     pick Meteor and click to call one down · \
                     pick Tsunami and click to send a wave · \
                     pick Gamma Ray and click to annihilate a column · \
                     pick a creature and click to drop one · \
                     drag a .rhai file to add a material",
                )
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
