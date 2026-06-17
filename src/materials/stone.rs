//! Stone — an immovable solid. Never moves on its own; blocks falling powders.
//!
//! Motion (or lack of it) is delegated to [`behaviors::solid`]. Copy this file
//! to add another static material (wall, bedrock, …).

use super::{Material, MaterialInfo, FIRE, LAVA};
use crate::behaviors;
use crate::sim::Simulation;

pub struct Stone;

/// Local temperature at or above which stone in contact with molten lava (or
/// flame) can melt. Pitched high enough that only a sustained, sizeable heat
/// source — a lava *lake*, not a stray droplet — ever drives a tile here, so rock
/// doesn't soften from a passing spark.
const MELT_TEMP: f32 = 600.0;

/// Stone only melts in a genuinely scorching *climate*: the world's ambient
/// temperature must be at least this. So a desert's lava lakes eat into the rock
/// around them, while the same lava in a temperate forest or a cold sea just sits
/// in its bed — the "desert + lava = hot enough to melt" rule.
const MELT_MIN_WORLD_TEMP: f32 = 35.0;

/// Once eligible, a stone cell melts with probability `1/this` per tick — slow,
/// so the rock gives way as a creeping front rather than slumping all at once.
const MELT_CHANCE: u32 = 200;

impl Material for Stone {
    fn info(&self) -> MaterialInfo {
        MaterialInfo {
            name: "Stone",
            color: [128, 128, 134, 255],
            jitter: 18,
            density: 255,
            movable: false,
            glow: false,
            source_temp: None,
        }
    }

    fn update(&self, sim: &mut Simulation, x: usize, y: usize) {
        // Melt back into lava in a scorching climate, where it's hot enough
        // locally *and* touching something molten or burning — a desert lava lake
        // slowly consumes the rock around it. The contact check keeps melting
        // pinned to the lava front rather than softening a whole baking hillside.
        if sim.world_temp() >= MELT_MIN_WORLD_TEMP
            && sim.temp_at(x, y) >= MELT_TEMP
            && (sim.neighbor(x, y, LAVA).is_some() || sim.neighbor(x, y, FIRE).is_some())
            && sim.chance(MELT_CHANCE)
        {
            sim.set(x, y, LAVA);
            return;
        }
        behaviors::solid(sim, x, y);
    }
}
