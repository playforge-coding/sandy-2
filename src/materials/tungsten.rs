//! Tungsten — the strongest metal. Falls as a rigid block, all but refuses to
//! melt, and lands hard enough to punch a dent into solid stone.
//!
//! Motion is the shared [`behaviors::falling_metal`] (straight-down gravity that
//! crushes any creature it drops on); the two things that set tungsten apart are
//! its near-immunity to melting and the dent it carves into stone on a heavy
//! landing. Copy this file (dropping the dent) to add another rigid metal.

use super::{Material, MaterialInfo, EMPTY, LAVA, STONE};
use crate::behaviors;
use crate::sim::Simulation;

pub struct Tungsten;

/// Sub-units added to the block's downward velocity each tick it falls — its
/// gravity. Tuned so a tall drop builds up a sizeable impact (which the dent
/// below spends) within the `i8` the cell stores velocity in.
const GRAVITY: i32 = 8;

/// Local temperature at which tungsten will even begin to soften. Pitched above
/// a lava lake's reach (lava sits near 1100; see [`crate::materials::lava`]) so
/// only the very hottest thing in the world — a fresh meteor's molten core —
/// stands a chance of melting it: tungsten "cannot be melted easily".
const MELT_TEMP: f32 = 1250.0;

/// Even once that scorching, a tungsten cell melts with probability only `1/this`
/// per tick — so it lingers as solid metal through all but a sustained inferno.
const MELT_CHANCE: u32 = 5000;

/// Minimum landing speed (in velocity sub-units) that punches into stone. Below
/// this the block just settles; a fall has to gather real pace to bite.
const DENT_MIN: i32 = 24;

/// Impact speed spent carving one cell of stone. Larger than [`GRAVITY`], so each
/// cell punched costs more than the next fall regains — the block sinks a little
/// deeper while it still has the speed for it, then comes to rest. The dent's
/// depth thus scales with how far the block fell.
const DENT_COST: i32 = 40;

impl Material for Tungsten {
    fn info(&self) -> MaterialInfo {
        MaterialInfo {
            name: "Tungsten",
            color: [82, 86, 96, 255],
            jitter: 10,
            density: 255,
            movable: false,
            glow: false,
            source_temp: None,
        }
    }

    fn update(&self, sim: &mut Simulation, x: usize, y: usize) {
        // Only the fiercest heat — touching molten lava in a searing local
        // hot-spot — ever melts it, and even then rarely.
        if sim.temp_at(x, y) >= MELT_TEMP
            && sim.neighbor(x, y, LAVA).is_some()
            && sim.chance(MELT_CHANCE)
        {
            sim.set(x, y, LAVA);
            return;
        }

        // Fall as a rigid block; on a hard landing on stone, punch a dent.
        if let Some(impact) = behaviors::falling_metal(sim, x, y, GRAVITY) {
            if impact >= DENT_MIN && y + 1 < sim.height && sim.mat_at(x, y + 1) == STONE {
                // Carve the stone directly below into a hollow and keep what's
                // left of the impact, so next tick the block drops into the dent
                // and may bite again — a deeper fall leaves a deeper dent.
                sim.set(x, y + 1, EMPTY);
                sim.set_velocity(x, y, 0, impact - DENT_COST);
            }
        }
    }
}
