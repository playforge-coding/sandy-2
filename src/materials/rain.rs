//! Rain — falling drops shed by clouds.
//!
//! Rain is deliberately *not* water: it never pools, it's absent from the
//! material picker (see [`Material::pickable`]), and it lives only until it hits
//! something. It rides the wind first (see [`behaviors::drift`]) so a gust slants
//! the downpour, then falls: straight down while the air below is open, slipping
//! diagonally past obstacles so it can thread through gaps. The moment it lands
//! on solid ground (or any liquid) it vanishes — and if that ground was [`SOIL`]
//! it leaves it as [`WET_SOIL`]. Stacked rain just waits for the column below to
//! drain rather than evaporating into the drop beneath it.

use super::{Material, MaterialInfo, EMPTY, RAIN, SOIL, WET_SOIL};
use crate::behaviors;
use crate::sim::Simulation;

pub struct Rain;

impl Material for Rain {
    fn info(&self) -> MaterialInfo {
        MaterialInfo {
            name: "Rain",
            color: [120, 162, 232, 255],
            jitter: 20,
            // Light and movable, but it never actually displaces anything — it
            // only falls into empty cells, then disappears on contact.
            density: 40,
            movable: true,
            glow: false,
        }
    }

    /// Rain is spawned by clouds, never painted, so keep it out of the palette.
    fn pickable(&self) -> bool {
        false
    }

    fn update(&self, sim: &mut Simulation, x: usize, y: usize) {
        // Reached the bottom of the world: nothing to wet, just vanish.
        if y + 1 >= sim.height {
            sim.set(x, y, EMPTY);
            return;
        }

        // Slant with the wind (and coast on its momentum) before falling. Rain
        // stays in the world at the edges — only its sideways momentum is lost
        // against a wall — so `escape: false`.
        let Some((x, y)) = behaviors::drift(sim, x, y, false) else {
            return;
        };
        // Drifting may have carried it to the floor; vanish there as above.
        if y + 1 >= sim.height {
            sim.set(x, y, EMPTY);
            return;
        }

        // Fall straight down through open air.
        if sim.mat_at(x, y + 1) == EMPTY {
            sim.try_move(x, y, x, y + 1);
            return;
        }

        // Blocked below: try to slip past on a down-diagonal so a drop can find
        // its way through canopy gaps and uneven terrain.
        let (first, second): (i32, i32) = if sim.rand_bool() { (-1, 1) } else { (1, -1) };
        for dx in [first, second] {
            let nx = x as i32 + dx;
            if nx >= 0 && (nx as usize) < sim.width && sim.mat_at(nx as usize, y + 1) == EMPTY {
                sim.try_move(x, y, nx as usize, y + 1);
                return;
            }
        }

        // Truly resting on something. Sitting on more rain? Just wait for the
        // column below to drain rather than vanishing into it.
        let below = sim.mat_at(x, y + 1);
        if below == RAIN {
            return;
        }
        // Hit the ground (soil, stone, a tree, a pond surface, …): dampen soil
        // and disappear.
        if below == SOIL {
            sim.set(x, y + 1, WET_SOIL);
        }
        sim.set(x, y, EMPTY);
    }
}
