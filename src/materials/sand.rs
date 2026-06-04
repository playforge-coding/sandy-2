//! Sand — a classic powder. Falls straight down and tumbles into a pile.
//!
//! All of the actual motion lives in [`behaviors::powder`]; this file just
//! supplies sand's identity. Copy this file to add another powder (dirt, ash,
//! salt, …) — change the `info()` and you're done.

use super::{Material, MaterialInfo};
use crate::behaviors;
use crate::sim::Simulation;

pub struct Sand;

impl Material for Sand {
    fn info(&self) -> MaterialInfo {
        MaterialInfo {
            name: "Sand",
            color: [194, 178, 128, 255],
            jitter: 28,
            density: 150,
            movable: true,
        }
    }

    fn update(&self, sim: &mut Simulation, x: usize, y: usize) {
        behaviors::powder(sim, x, y);
    }
}
