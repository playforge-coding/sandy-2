//! Empty — air / nothing.
//!
//! It has no behaviour and is never updated directly (the simulation skips
//! empty cells), but it lives in the registry as id 0 so colour lookups and
//! density comparisons work uniformly for every cell. Its colour is the daytime
//! sky blue the whole world is drawn against, so clouds, rain and terrain read
//! as sitting in open air rather than the void.

use super::{Material, MaterialInfo};
use crate::sim::Simulation;

pub struct Empty;

impl Material for Empty {
    fn info(&self) -> MaterialInfo {
        MaterialInfo {
            name: "Empty",
            // Daytime sky blue — the background the whole sim is drawn over.
            color: [124, 173, 222, 255],
            jitter: 0,
            density: 0,
            movable: false,
            glow: false,
        }
    }

    fn update(&self, _sim: &mut Simulation, _x: usize, _y: usize) {}
}
