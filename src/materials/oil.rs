//! Oil — a light, flammable liquid. Flows like water but is lighter, so it
//! floats on top of it; and it catches fire from anything hot next to it.
//!
//! Motion is the shared [`behaviors::liquid`]; the twist is the ignition check
//! it runs first: a one-sided [`behaviors::transform_on_contact`] against fire
//! and lava turns the oil cell into [`FIRE`] without consuming what lit it, so
//! flames creep across a pool one cell at a time.

use super::{Material, MaterialInfo, FIRE, LAVA};
use crate::behaviors;
use crate::sim::Simulation;

pub struct Oil;

impl Material for Oil {
    fn info(&self) -> MaterialInfo {
        MaterialInfo {
            name: "Oil",
            color: [60, 52, 40, 255],
            jitter: 18,
            // Lighter than water (100) so it floats on top of it.
            density: 80,
            movable: true,
            glow: false,
        }
    }

    fn update(&self, sim: &mut Simulation, x: usize, y: usize) {
        // Ignites when touched by fire or lava — the cell becomes fire while the
        // flame/lava that lit it stays put and goes on to the next cell.
        if behaviors::transform_on_contact(sim, x, y, FIRE, FIRE)
            || behaviors::transform_on_contact(sim, x, y, LAVA, FIRE)
        {
            return;
        }
        // Runny, like water.
        behaviors::liquid(sim, x, y, 4);
    }
}
