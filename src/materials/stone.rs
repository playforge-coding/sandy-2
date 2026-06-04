//! Stone — an immovable solid. Never moves on its own; blocks falling powders.
//!
//! Motion (or lack of it) is delegated to [`behaviors::solid`]. Copy this file
//! to add another static material (wall, bedrock, …).

use super::{Material, MaterialInfo};
use crate::behaviors;
use crate::sim::Simulation;

pub struct Stone;

impl Material for Stone {
    fn info(&self) -> MaterialInfo {
        MaterialInfo {
            name: "Stone",
            color: [128, 128, 134, 255],
            jitter: 18,
            density: 255,
            movable: false,
        }
    }

    fn update(&self, sim: &mut Simulation, x: usize, y: usize) {
        behaviors::solid(sim, x, y);
    }
}
