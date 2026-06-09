//! Shared movement logic that creatures reuse — the entity cousin of
//! [`crate::behaviors`].
//!
//! A creature's `update` should be tiny: it just delegates to one of these. That
//! way every walker shares one well-behaved "amble along the ground" routine and
//! every flier one "wheel through the sky" routine, and a new creature gets that
//! motion for free by calling the same helper.
//!
//! These sense the world through the same `pub(crate)` grid helpers the material
//! behaviours use (`cell_mat`, `chance`, `rand_f32`), so they never need to know
//! how the grid is stored.

use crate::materials::{MaterialId, EMPTY, LAVA, OIL, WATER};
use crate::sim::Simulation;

use super::EntityState;

/// Liquids a land creature drowns or burns in if it ends up standing in one.
fn is_liquid(m: MaterialId) -> bool {
    matches!(m, WATER | LAVA | OIL)
}

/// Whether a cell is solid footing or a wall to a creature: any filled,
/// non-liquid cell. The world's sides and floor count as solid (so a walker
/// can't trundle off the edge of the world); the open sky *above* the world does
/// not, so a flier near the top isn't fenced in by phantom ceiling.
fn solid(sim: &Simulation, x: i32, y: i32) -> bool {
    match sim.cell_mat(x, y) {
        Some(m) => m != EMPTY && !is_liquid(m),
        // Off-grid: sides and the floor block (`y >= 0`); above the top is sky.
        None => y >= 0,
    }
}

/// Ticks between an ambling creature's steps. Higher = a slower walk; moving
/// every tick would have an ant sprint across the world.
const WALK_STEP_TICKS: u32 = 3;

/// Walk along solid surfaces: fall under gravity until something is underfoot,
/// then amble in the facing direction — climbing a one-cell step, easing down a
/// one-cell drop, and turning around at walls and ledges. Drowns the creature
/// the instant it's standing in a liquid. Shared by ants and any future
/// ground-crawler (beetles, worms, …).
pub fn walk(sim: &mut Simulation, me: &mut EntityState) {
    me.timer = me.timer.wrapping_add(1);
    let x = me.x.round() as i32;
    let y = me.y.round() as i32;

    // Standing in liquid: drown or burn.
    if let Some(m) = sim.cell_mat(x, y) {
        if is_liquid(m) {
            me.alive = false;
            return;
        }
    }

    // Gravity: nothing solid directly underfoot → fall one cell this tick.
    if !solid(sim, x, y + 1) {
        me.y += 1.0;
        return;
    }

    // Amble at a walking pace rather than a cell every single tick.
    if me.timer % WALK_STEP_TICKS != 0 {
        return;
    }

    let ahead = x + me.dir as i32;
    if solid(sim, ahead, y) {
        // Wall ahead: climb a single-cell step if there's headroom, else turn.
        if !solid(sim, ahead, y - 1) {
            me.x = ahead as f32;
            me.y = (y - 1) as f32;
        } else {
            me.dir = -me.dir;
        }
    } else if solid(sim, ahead, y + 1) {
        // Flat ground continues ahead: step onto it.
        me.x = ahead as f32;
    } else if solid(sim, ahead, y + 2) {
        // A one-cell drop with ground below it: follow the slope down.
        me.x = ahead as f32;
        me.y = (y + 1) as f32;
    } else {
        // A sheer ledge with nothing to step onto: turn back rather than march
        // off into the air.
        me.dir = -me.dir;
    }
}

/// Cruising speed of a flier, in cells/tick.
const FLY_SPEED: f32 = 1.1;
/// Start climbing when terrain is within this many cells below.
const GROUND_CLEARANCE: i32 = 8;
/// Keep this many cells clear of the top of the world.
const SKY_MARGIN: f32 = 6.0;
/// Per-tick damping on vertical speed, so a flier levels out instead of
/// careening up or down forever.
const VY_DAMPING: f32 = 0.92;

/// Wheel through open air: cruise horizontally in the facing direction, wander
/// gently up and down, steer away from terrain below and the ceiling above, and
/// turn at side walls or anything solid dead ahead. Shared by birds and any
/// future flier (bats, butterflies, …).
pub fn fly(sim: &mut Simulation, me: &mut EntityState) {
    me.timer = me.timer.wrapping_add(1);

    // Pick up the initial heading from the facing direction.
    if me.vx == 0.0 {
        me.vx = me.dir as f32 * FLY_SPEED;
    }

    let x = me.x.round() as i32;
    let y = me.y.round() as i32;

    // A little vertical wandering, damped so it bobs rather than drifts away.
    if sim.chance(8) {
        me.vy += sim.rand_f32() - 0.5;
    }
    me.vy *= VY_DAMPING;

    // Steer clear of the ground below and the ceiling above.
    if (1..=GROUND_CLEARANCE).any(|d| solid(sim, x, y + d)) {
        me.vy -= 0.5; // climb away from the terrain
    }
    if me.y < SKY_MARGIN {
        me.vy += 0.4; // dip away from the top of the world
    }
    me.vy = me.vy.clamp(-1.4, 1.4);

    // Horizontal: turn at a side wall or any terrain straight ahead.
    let nx = me.x + me.vx;
    let ahead = nx.round() as i32;
    if nx < 1.0 || nx >= (sim.width - 1) as f32 || solid(sim, ahead, y) {
        me.vx = -me.vx;
        me.dir = -me.dir;
    } else {
        me.x = nx;
    }

    me.y = (me.y + me.vy).clamp(1.0, (sim.height - 2) as f32);
}
