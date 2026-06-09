//! Meteor — a summoned projectile that streaks in and detonates on impact.
//!
//! Unlike every other material, a meteor isn't painted: the Meteor tool launches
//! one from a top corner aimed at the clicked cell (see
//! [`crate::sim::Simulation::spawn_meteor`]). It rides the shared
//! [`behaviors::ballistic`] motion — coasting on its own velocity, ignoring the
//! wind, and arcing down under gravity — and the instant that flight meets the
//! ground (or anything else solid) it bursts into a molten [`LAVA`] core wrapped
//! in a ball of outward-flung [`FIRE`].

use super::{Material, MaterialInfo, EMPTY, FIRE, LAVA};
use crate::behaviors;
use crate::sim::{Simulation, VEL_UNIT};

pub struct Meteor;

/// Sub-units added to the meteor's downward velocity each tick, curving its
/// aimed shot into a falling arc.
const GRAVITY: i32 = 3;

/// How far the blast reaches, in cells.
const BLAST_RADIUS: i32 = 9;

/// Inner radius of the blast that floods with molten lava; beyond it is fire.
const LAVA_CORE: i32 = 4;

/// 1-in-N chance per tick to shed a flaming ember into the cell just vacated,
/// giving the meteor a glowing tail.
const TRAIL: u32 = 2;

impl Material for Meteor {
    fn info(&self) -> MaterialInfo {
        MaterialInfo {
            name: "Meteor",
            color: [255, 170, 60, 255],
            jitter: 40,
            // Heavier than anything else, but it never actually sinks through
            // material — `ballistic` explodes it on contact instead.
            density: 250,
            movable: true,
            glow: true,
        }
    }

    fn update(&self, sim: &mut Simulation, x: usize, y: usize) {
        match behaviors::ballistic(sim, x, y, GRAVITY) {
            // Still in flight: leave the odd ember behind for a fiery streak.
            Ok((nx, ny)) => {
                if (nx, ny) != (x, y) && sim.mat_at(x, y) == EMPTY && sim.chance(TRAIL) {
                    sim.set(x, y, FIRE);
                }
            }
            // Struck the ground (or a wall): detonate.
            Err((ix, iy)) => explode(sim, ix, iy),
        }
    }

    fn pickable(&self) -> bool {
        false // summoned with the Meteor tool, never hand-painted
    }
}

/// Detonate at `(cx, cy)`: a molten lava core wrapped in a fireball that bursts
/// outward. The meteor cell itself is consumed in the blast.
fn explode(sim: &mut Simulation, cx: usize, cy: usize) {
    let r2 = BLAST_RADIUS * BLAST_RADIUS;
    let core2 = LAVA_CORE * LAVA_CORE;
    // Outward kick on the flames so the fireball visibly blasts apart before the
    // flames' own buoyancy takes over (their velocity bleeds off via `drift`).
    let burst = 2 * VEL_UNIT;
    for dy in -BLAST_RADIUS..=BLAST_RADIUS {
        for dx in -BLAST_RADIUS..=BLAST_RADIUS {
            let d2 = dx * dx + dy * dy;
            if d2 > r2 {
                continue;
            }
            let x = cx as i32 + dx;
            let y = cy as i32 + dy;
            if x < 0 || y < 0 || x as usize >= sim.width || y as usize >= sim.height {
                continue;
            }
            let (x, y) = (x as usize, y as usize);
            if d2 <= core2 {
                // Molten heart: lava floods the crater, whatever was there.
                sim.set(x, y, LAVA);
            } else if sim.mat_at(x, y) == EMPTY {
                // Fireball: flame fills the surrounding air and is flung outward.
                sim.set(x, y, FIRE);
                let len = (d2 as f32).sqrt().max(1.0);
                let vx = (dx as f32 / len * burst as f32) as i32;
                let vy = (dy as f32 / len * burst as f32) as i32;
                sim.set_velocity(x, y, vx, vy);
            }
        }
    }
}
