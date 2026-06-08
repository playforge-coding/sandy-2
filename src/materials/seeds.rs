//! Seeds — a powder that grows into a tree, but only in damp ground.
//!
//! A seed tumbles and settles like sand ([`behaviors::powder`]). What makes it
//! special is what happens once it comes to rest: if the cell it's sitting on is
//! [`WET_SOIL`], it germinates — after a short random delay it replaces itself
//! with a wood trunk topped by a leafy canopy. On dry [`SOIL`] (or anything
//! else) it simply waits, so a gardener has to rain on the ground first. This is
//! the payoff end of the cloud → rain → wet-soil chain.

use super::{Material, MaterialInfo, EMPTY, LEAVES, WET_SOIL, WOOD};
use crate::behaviors;
use crate::sim::Simulation;

pub struct Seeds;

/// Once a seed rests on wet soil it sprouts with probability `1/this` per tick,
/// so germination feels like it takes a moment rather than happening instantly.
const GERMINATE_RARITY: u32 = 90;

impl Material for Seeds {
    fn info(&self) -> MaterialInfo {
        MaterialInfo {
            name: "Seeds",
            color: [150, 132, 64, 255],
            jitter: 26,
            // Light — a powder that piles up, lighter than sand so it rides on
            // top rather than burrowing through it.
            density: 120,
            movable: true,
            glow: false,
        }
    }

    fn update(&self, sim: &mut Simulation, x: usize, y: usize) {
        // Resting on damp ground? Take root rather than tumbling further.
        if y + 1 < sim.height && sim.mat_at(x, y + 1) == WET_SOIL {
            if sim.chance(GERMINATE_RARITY) {
                grow_tree(sim, x, y);
            }
            // Whether or not it sprouted this tick, a seed on wet soil stays put.
            return;
        }
        // Otherwise it's just a powder, falling and piling.
        behaviors::powder(sim, x, y);
    }
}

/// Sprout a tree upward from the seed at `(x, y)`: a vertical wood trunk capped
/// with a blob of leaves, sized from the simulation RNG so no two are quite the
/// same. Mirrors `worldgen::plant_tree`, but self-contained and driven by the
/// live RNG since it happens mid-simulation. The seed cell becomes the base of
/// the trunk.
fn grow_tree(sim: &mut Simulation, x: usize, y: usize) {
    let trunk_h = 5 + (sim.rand_u32() % 6) as usize; // 5..=10
    let radius = 3 + (sim.rand_u32() % 3) as i32; // 3..=5

    // Need clear sky for the trunk and the canopy above it; if the seed is too
    // close to the ceiling, just let it lie there and try again later.
    if y < trunk_h + radius as usize + 1 {
        return;
    }

    // Trunk: from the seed cell upward.
    let mut ty = y;
    for _ in 0..trunk_h {
        sim.set(x, ty, WOOD);
        ty -= 1;
    }

    // Canopy: a filled disk of leaves at the trunk's top, painted only over
    // empty sky so it doesn't punch through neighbouring trees or terrain.
    let (cx, cy) = (x as i32, ty as i32);
    let r2 = radius * radius;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx * dx + dy * dy > r2 {
                continue;
            }
            let (lx, ly) = (cx + dx, cy + dy);
            if lx < 0 || ly < 0 || lx as usize >= sim.width || ly as usize >= sim.height {
                continue;
            }
            if sim.mat_at(lx as usize, ly as usize) == EMPTY {
                sim.set(lx as usize, ly as usize, LEAVES);
            }
        }
    }
}
