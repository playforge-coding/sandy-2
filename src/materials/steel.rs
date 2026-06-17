//! Steel — the second-strongest metal. Falls as a rigid block and resists
//! melting all but completely; it just hasn't tungsten's stone-denting heft.
//!
//! Motion is the shared [`behaviors::falling_metal`] (straight-down gravity that
//! crushes any creature it drops on). The only thing it adds over the bare metal
//! behaviour is a stubborn, very-rarely-met melting point.

use super::{Material, MaterialInfo, LAVA};
use crate::behaviors;
use crate::sim::Simulation;

pub struct Steel;

/// Sub-units added to the block's downward velocity each tick it falls — shared
/// with the other metals so they all drop at the same rate.
const GRAVITY: i32 = 8;

/// Local temperature at which steel begins to soften — reached only deep inside a
/// real lava lake, not at its edge, so steel "cannot be melted easily".
const MELT_TEMP: f32 = 1000.0;

/// Once that hot and touching lava, a steel cell melts with probability only
/// `1/this` per tick: it gives way far more grudgingly than iron, but not with
/// tungsten's near-total obstinacy.
const MELT_CHANCE: u32 = 2000;

impl Material for Steel {
    fn info(&self) -> MaterialInfo {
        MaterialInfo {
            name: "Steel",
            color: [150, 158, 170, 255],
            jitter: 12,
            density: 255,
            movable: false,
            glow: false,
            source_temp: None,
        }
    }

    fn update(&self, sim: &mut Simulation, x: usize, y: usize) {
        // Melts only in the heart of a lava lake, and even then seldom.
        if sim.temp_at(x, y) >= MELT_TEMP
            && sim.neighbor(x, y, LAVA).is_some()
            && sim.chance(MELT_CHANCE)
        {
            sim.set(x, y, LAVA);
            return;
        }
        // Falls as a rigid block, crushing creatures beneath it. No stone-denting.
        behaviors::falling_metal(sim, x, y, GRAVITY);
    }
}
