//! Fire — a short-lived, rising flame.
//!
//! Fire has no fuel of its own: every tick it has a chance to gutter out and
//! vanish, so a flame lives only a handful of ticks unless something keeps
//! re-lighting it (oil igniting its neighbours, lava spitting fresh flames).
//! While it burns it drifts upward via the shared [`behaviors::gas`] motion.

use super::{Material, MaterialInfo, EMPTY};
use crate::behaviors;
use crate::sim::Simulation;

pub struct Fire;

/// Expected lifetime knob: each tick the flame dies with probability `1/N`, so
/// it lives on the order of this many ticks.
const BURN_OUT: u32 = 12;

impl Material for Fire {
    fn info(&self) -> MaterialInfo {
        MaterialInfo {
            name: "Fire",
            color: [240, 120, 30, 255],
            jitter: 64,
            // Very light, so it rises through air rather than sinking.
            density: 5,
            movable: true,
        }
    }

    fn update(&self, sim: &mut Simulation, x: usize, y: usize) {
        // Burn out over time, leaving empty air behind.
        if sim.chance(BURN_OUT) {
            sim.set(x, y, EMPTY);
            return;
        }
        // Otherwise rise and flicker sideways like any other gas.
        behaviors::gas(sim, x, y, 1);
    }
}
