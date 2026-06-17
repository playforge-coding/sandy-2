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

/// Each tick, a cloud cell drips a raindrop with probability `1/this` at the
/// reference temperature [`TEMP_REF`]. Warmer air drives more evaporation and
/// convection, so the actual rarity scales with the local temperature (see
/// [`drip_rarity`]) — a cloud over a desert or a lava field pours, one drifting
/// through cold sea air barely spits. Tuned so a temperate cloud gives a steady
/// drizzle rather than a solid sheet.
const DRIP_RARITY: u32 = 40;

/// The temperature [`DRIP_RARITY`] is calibrated at — the temperate baseline.
const TEMP_REF: f32 = 20.0;

/// Clamp on the temperature-scaled drip rarity: how fiercely the warmest air can
/// rain, and how sparse the coldest cloud's drizzle gets.
const DRIP_MIN: u32 = 6;
const DRIP_MAX: u32 = 220;

/// The drip rarity for a cloud cell at local temperature `temp`: lower (rains
/// harder) as it warms, higher (rains less) as it cools, clamped either side.
fn drip_rarity(temp: f32) -> u32 {
    let scaled = DRIP_RARITY as f32 * TEMP_REF / temp.max(1.0);
    (scaled as u32).clamp(DRIP_MIN, DRIP_MAX)
}

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
            source_temp: None,
        }
    }

    fn update(&self, sim: &mut Simulation, x: usize, y: usize) {
        // Drip into the open air just below. Only into empty space, so a cloud
        // resting on the ground (or stacked on its own rain) doesn't spawn
        // drops inside solid cells.
        if y + 1 < sim.height && sim.mat_at(x, y + 1) == EMPTY {
            let rarity = drip_rarity(sim.temp_at(x, y));
            if sim.chance(rarity) {
                sim.set(x, y + 1, RAIN);
            }
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

#[cfg(test)]
mod tests {
    use super::{drip_rarity, DRIP_MAX, DRIP_MIN};

    #[test]
    fn warmer_air_rains_harder_than_cold() {
        // Lower rarity = more rain. Warm air should drip more often than cold, and
        // scorching air over a lava field harder still.
        let hot = drip_rarity(45.0); // desert
        let mild = drip_rarity(20.0); // temperate
        let cold = drip_rarity(6.0); // cold sea
        assert!(
            hot < mild && mild < cold,
            "rain should increase with temperature: hot {hot} < mild {mild} < cold {cold}"
        );
    }

    #[test]
    fn drip_rarity_is_clamped_at_both_extremes() {
        // Over molten lava it can't rain infinitely hard; in a deep freeze it
        // can't dry up to never raining at all.
        assert_eq!(drip_rarity(5000.0), DRIP_MIN);
        assert_eq!(drip_rarity(0.1), DRIP_MAX);
    }
}
