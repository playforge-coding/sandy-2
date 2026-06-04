//! Water — a runny liquid. Falls, tumbles, and spreads fast to find its level.
//!
//! All of the actual motion lives in [`behaviors::liquid`]; this file just
//! supplies water's identity and its flow speed. Copy this file to add another
//! liquid (oil, acid, …) — change the `info()` and the `speed` argument.

use super::{Material, MaterialInfo, LAVA, STONE};
use crate::behaviors;
use crate::sim::Simulation;

pub struct Water;

impl Material for Water {
    fn info(&self) -> MaterialInfo {
        MaterialInfo {
            name: "Water",
            color: [64, 120, 220, 255],
            jitter: 16,
            // Lighter than sand (150) so sand sinks through it.
            density: 100,
            movable: true,
        }
    }

    fn update(&self, sim: &mut Simulation, x: usize, y: usize) {
        // Quenches lava on contact: both become stone.
        if behaviors::react_on_contact(sim, x, y, LAVA, STONE) {
            return;
        }
        // Runny: flows several cells sideways per tick, so it levels off fast.
        behaviors::liquid(sim, x, y, 5);
    }
}
