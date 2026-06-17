//! Lava — a sluggish, viscous liquid. Same motion as water, but it barely
//! creeps sideways so it pools into blobs instead of fanning out.
//!
//! Motion is delegated to [`behaviors::liquid`], exactly like water; the only
//! difference is the lower `speed` (viscosity) it passes.

use super::{Material, MaterialInfo, FIRE, STONE, WATER};
use crate::behaviors;
use crate::sim::Simulation;

pub struct Lava;

/// Rarity of lava spitting out a flame: roughly one chance in this many ticks
/// per cell, so a pool of lava gently flickers fire above itself.
const SPIT_FIRE: u32 = 40;

/// Local temperature below which lava loses its heat and crusts over into stone.
/// A molten lake keeps its own tile far above this, so it stays liquid; but a
/// thin trickle or a stray gobbet — especially in a cold world like the ocean —
/// can't hold its warmth against the surroundings and solidifies. (This is on top
/// of the instant water-contact quench.)
const FREEZE_TEMP: f32 = 120.0;

/// Once chilled below [`FREEZE_TEMP`], a lava cell hardens with probability
/// `1/this` per tick — a few seconds to set, like a cooling crust forming.
const FREEZE_CHANCE: u32 = 60;

impl Material for Lava {
    fn info(&self) -> MaterialInfo {
        MaterialInfo {
            name: "Lava",
            color: [207, 70, 24, 255],
            jitter: 36,
            // Denser than water (100) so it sinks below it, lighter than sand
            // (150) so sand still sinks through it.
            density: 130,
            movable: true,
            glow: true,
            // Molten rock: the hottest of the everyday materials, so a pool of it
            // makes a fierce local hot-spot in the temperature field.
            source_temp: Some(1100.0),
        }
    }

    fn update(&self, sim: &mut Simulation, x: usize, y: usize) {
        // Quenched by water on contact: both become stone.
        if behaviors::react_on_contact(sim, x, y, WATER, STONE) {
            return;
        }
        // Cooled by its surroundings: a lava cell that can't hold its heat (a
        // thin run, or any lava in a cold climate) hardens into stone.
        if sim.temp_at(x, y) < FREEZE_TEMP && sim.chance(FREEZE_CHANCE) {
            sim.set(x, y, STONE);
            return;
        }
        // Every so often, throw off a flame into the air above.
        behaviors::emit(sim, x, y, FIRE, SPIT_FIRE);
        // Viscous: only flows one cell sideways per tick, so it stays in blobs.
        behaviors::liquid(sim, x, y, 1);
    }
}
