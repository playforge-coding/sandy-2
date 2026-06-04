//! Lava — a sluggish, viscous liquid. Same motion as water, but it barely
//! creeps sideways so it pools into blobs instead of fanning out.
//!
//! Motion is delegated to [`behaviors::liquid`], exactly like water; the only
//! difference is the lower `speed` (viscosity) it passes.

use super::{Material, MaterialInfo, STONE, WATER};
use crate::behaviors;
use crate::sim::Simulation;

pub struct Lava;

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
        }
    }

    fn update(&self, sim: &mut Simulation, x: usize, y: usize) {
        // Quenched by water on contact: both become stone.
        if behaviors::react_on_contact(sim, x, y, WATER, STONE) {
            return;
        }
        // Viscous: only flows one cell sideways per tick, so it stays in blobs.
        behaviors::liquid(sim, x, y, 1);
    }
}
