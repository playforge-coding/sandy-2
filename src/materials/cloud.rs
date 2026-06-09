//! Cloud — a drifting puff that rains.
//!
//! A cloud doesn't fall, but it isn't pinned either: it rides the wind (see
//! [`behaviors::drift`]) sideways and slowly bobs upward, the way a real cloud
//! loiters across the sky. Every so often a cell sheds a drop of [`RAIN`] into
//! the open air directly beneath it; the rain then falls on its own and wets
//! whatever soil it lands on — see `rain.rs`. Blown to the edge of the world a
//! cloud drifts off and is gone, a cell at a time.
//!
//! Movement goes through `try_move`, so a cloud only ever drifts into open air
//! and the bottom-to-top scan's `moved` stamp keeps a rising cell from being
//! processed twice in a tick (the same guard fire relies on).

use super::{Material, MaterialInfo, EMPTY, RAIN};
use crate::behaviors;
use crate::sim::Simulation;

pub struct Cloud;

/// Each tick, a cloud cell drips a raindrop with probability `1/this`. Tuned so
/// a painted cloud produces a steady drizzle rather than a solid sheet.
const DRIP_RARITY: u32 = 40;

/// Chance per tick (`1/this`) that a cell bobs upward one cell. Keeps clouds
/// loitering high as they ride the mostly-horizontal wind.
const RISE_RARITY: u32 = 60;

impl Material for Cloud {
    fn info(&self) -> MaterialInfo {
        MaterialInfo {
            name: "Cloud",
            color: [228, 230, 238, 255],
            jitter: 14,
            density: 255,
            // Not movable: rain and other particles fall *past* a cloud rather
            // than shoving it around. It still moves itself (see `update`).
            movable: false,
            glow: false,
        }
    }

    fn update(&self, sim: &mut Simulation, x: usize, y: usize) {
        // Drip into the open air just below. Only into empty space, so a cloud
        // resting on the ground (or stacked on its own rain) doesn't spawn
        // drops inside solid cells.
        if y + 1 < sim.height && sim.mat_at(x, y + 1) == EMPTY && sim.chance(DRIP_RARITY) {
            sim.set(x, y + 1, RAIN);
        }

        // Ride the wind. `escape: true` lets a cloud blown off the side drift
        // away for good rather than piling against the wall.
        let Some((x, y)) = behaviors::drift(sim, x, y, true) else {
            return;
        };

        // Bob gently upward.
        if y > 0 && sim.chance(RISE_RARITY) {
            sim.try_move(x, y, x, y - 1);
        }
    }
}
