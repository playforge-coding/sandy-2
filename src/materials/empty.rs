//! Empty — air / nothing.
//!
//! It has no behaviour and is never updated directly (the simulation skips
//! empty cells), but it lives in the registry as id 0 so colour lookups and
//! density comparisons work uniformly for every cell.

use super::{Material, MaterialInfo};
use crate::sim::Simulation;

pub struct Empty;

impl Material for Empty {
    fn info(&self) -> MaterialInfo {
        MaterialInfo {
            name: "Empty",
            color: [0, 0, 0, 255],
            jitter: 0,
            density: 0,
            movable: false,
        }
    }

    fn update(&self, _sim: &mut Simulation, _x: usize, _y: usize) {}
}
