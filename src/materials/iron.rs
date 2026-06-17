//! Iron — a strong metal, but the one lava can melt. Falls as a rigid block like
//! the others; it parts with its shape only when molten rock gets at it.
//!
//! Motion is the shared [`behaviors::falling_metal`] (straight-down gravity that
//! crushes any creature it drops on). Where tungsten and steel shrug off all but
//! the fiercest heat, iron melts readily wherever lava touches it.

use super::{Material, MaterialInfo, LAVA};
use crate::behaviors;
use crate::sim::Simulation;

pub struct Iron;

/// Sub-units added to the block's downward velocity each tick it falls — shared
/// with the other metals so they all drop at the same rate.
const GRAVITY: i32 = 8;

/// Local temperature at which iron melts. Modest — well within the heat a pool of
/// lava throws into the cells it touches — so lava lapping at iron eats through
/// it, while iron well away from any lava stays solid in any climate.
const MELT_TEMP: f32 = 500.0;

/// Once hot enough and touching lava, an iron cell melts with probability `1/this`
/// per tick — quick enough that a lava flow visibly consumes iron, the soft metal
/// of the three.
const MELT_CHANCE: u32 = 80;

impl Material for Iron {
    fn info(&self) -> MaterialInfo {
        MaterialInfo {
            name: "Iron",
            color: [124, 116, 110, 255],
            jitter: 14,
            density: 255,
            movable: false,
            glow: false,
            source_temp: None,
        }
    }

    fn update(&self, sim: &mut Simulation, x: usize, y: usize) {
        // Melted by lava: where molten rock touches it and the cell runs hot, the
        // iron turns molten too.
        if sim.temp_at(x, y) >= MELT_TEMP
            && sim.neighbor(x, y, LAVA).is_some()
            && sim.chance(MELT_CHANCE)
        {
            sim.set(x, y, LAVA);
            return;
        }
        // Falls as a rigid block, crushing creatures beneath it.
        behaviors::falling_metal(sim, x, y, GRAVITY);
    }
}
