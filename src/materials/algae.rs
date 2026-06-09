//! Algae — an aquatic plant that creeps slowly through still water.
//!
//! Where [`leaves`](super::leaves) are the land's food (for ants), algae is the
//! water's (for fish). It's a static, immovable green — it never falls or flows —
//! but it isn't quite inert: a cell touching open water now and then puts out a
//! tendril into one of those neighbouring [`WATER`] cells, so a patch spreads
//! gradually across a pool. Growth is slow and only ever into water it's already
//! beside, so it creeps rather than blooms; fish grazing it (and the pool's size)
//! keep it in check. Stranded out of water — a pool drained away from under it —
//! it has no water to drink from and slowly withers back to nothing.

use super::{Material, MaterialInfo, EMPTY, WATER};
use crate::behaviors;
use crate::sim::Simulation;

pub struct Algae;

/// Chance per tick (1 in this) that a water-fed cell creeps into a neighbour.
/// High, so a patch spreads as a slow creep rather than a sudden bloom.
const GROW_RARITY: u32 = 220;
/// Chance per tick (1 in this) that a cell cut off from all water withers away.
const WITHER_RARITY: u32 = 160;

impl Material for Algae {
    fn info(&self) -> MaterialInfo {
        MaterialInfo {
            name: "Algae",
            color: [42, 138, 92, 255],
            jitter: 34,
            density: 255,
            movable: false,
            glow: false,
        }
    }

    fn update(&self, sim: &mut Simulation, x: usize, y: usize) {
        // Algae lives on the water around it. Beside open water it slowly spreads;
        // cut off from water entirely it withers back to air.
        match sim.neighbor(x, y, WATER) {
            Some((wx, wy)) => {
                if sim.chance(GROW_RARITY) {
                    sim.set(wx, wy, super::ALGAE);
                }
            }
            None => {
                if sim.chance(WITHER_RARITY) {
                    sim.set(x, y, EMPTY);
                }
            }
        }
        // No motion of its own otherwise — it stays rooted where it grew.
        behaviors::solid(sim, x, y);
    }
}
