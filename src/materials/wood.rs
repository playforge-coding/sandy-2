//! Wood — the trunk and branches of a tree. A static solid that burns.
//!
//! Wood doesn't move (it's held up like stone), but it's combustible: a flame
//! or lava touching it will, over a few ticks, set it alight via the shared
//! [`behaviors::flammable`] helper. It catches more slowly than leaves, so a
//! fire creeps down a trunk rather than consuming the whole tree at once.

use super::{Material, MaterialInfo};
use crate::behaviors;
use crate::sim::Simulation;

pub struct Wood;

/// Each tick a flame/lava touches it, wood ignites with probability `1/this`.
/// Larger than leaves' value, so trunks resist the fire longer.
const IGNITE_RARITY: u32 = 12;

impl Material for Wood {
    fn info(&self) -> MaterialInfo {
        MaterialInfo {
            name: "Wood",
            color: [96, 64, 36, 255],
            jitter: 16,
            density: 255,
            movable: false,
            glow: false,
        }
    }

    fn update(&self, sim: &mut Simulation, x: usize, y: usize) {
        if behaviors::flammable(sim, x, y, IGNITE_RARITY) {
            return;
        }
        behaviors::solid(sim, x, y);
    }
}
