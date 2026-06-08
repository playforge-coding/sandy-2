//! Leaves — a tree's canopy. A static solid that catches fire readily.
//!
//! Like wood, leaves don't fall (the canopy hangs in place), but they're far
//! more flammable: a much smaller ignition rarity than the trunk means fire
//! races through foliage and only then works its way down into the wood. Motion
//! is otherwise [`behaviors::solid`].

use super::{Material, MaterialInfo};
use crate::behaviors;
use crate::sim::Simulation;

pub struct Leaves;

/// Leaves catch quickly — a low rarity, so a flame sweeps through a canopy.
const IGNITE_RARITY: u32 = 3;

impl Material for Leaves {
    fn info(&self) -> MaterialInfo {
        MaterialInfo {
            name: "Leaves",
            color: [58, 132, 56, 255],
            jitter: 30,
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
