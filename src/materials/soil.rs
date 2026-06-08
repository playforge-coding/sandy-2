//! Soil — packed earth. The bulk of the generated ground's top layer.
//!
//! Unlike sand, soil is a *solid*: it holds the terrain's shape so the
//! noise-generated hills and valleys (see [`crate::worldgen`]) don't avalanche
//! flat the instant the simulation starts. Motion is delegated to
//! [`behaviors::solid`], exactly like stone — the only difference is its earthy
//! colour.

use super::{Material, MaterialInfo};
use crate::behaviors;
use crate::sim::Simulation;

pub struct Soil;

impl Material for Soil {
    fn info(&self) -> MaterialInfo {
        MaterialInfo {
            name: "Soil",
            color: [104, 72, 44, 255],
            jitter: 22,
            density: 255,
            movable: false,
            glow: false,
        }
    }

    fn update(&self, sim: &mut Simulation, x: usize, y: usize) {
        behaviors::solid(sim, x, y);
    }
}
