//! Wet soil — soil that rain has dampened.
//!
//! Created when a raindrop lands on [`SOIL`] (see `rain.rs`). It holds its shape
//! exactly like dry soil — motion is [`behaviors::solid`] — but it carries one
//! extra property: a [`crate::materials::seeds`] grain resting on it will sprout
//! into a tree. The dampness isn't permanent; each tick it has a small chance to
//! dry back out into ordinary [`SOIL`], so a patch must be kept rained-on to
//! stay plantable.

use super::{Material, MaterialInfo, SOIL};
use crate::behaviors;
use crate::sim::Simulation;

pub struct WetSoil;

/// Each tick a damp cell dries with probability `1/this`, reverting to soil.
/// Large enough that a soaked patch stays plantable for a good while.
const DRY_OUT: u32 = 900;

impl Material for WetSoil {
    fn info(&self) -> MaterialInfo {
        MaterialInfo {
            name: "Wet Soil",
            // Darker and cooler than dry soil's [104, 72, 44], the way damp
            // earth looks.
            color: [68, 46, 30, 255],
            jitter: 18,
            density: 255,
            movable: false,
            glow: false,
        }
    }

    // Just use the rain to grow it
    fn pickable(&self) -> bool {
        false
    }

    fn update(&self, sim: &mut Simulation, x: usize, y: usize) {
        if sim.chance(DRY_OUT) {
            sim.set(x, y, SOIL);
            return;
        }
        behaviors::solid(sim, x, y);
    }
}
