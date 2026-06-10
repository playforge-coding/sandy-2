//! Seed-based world generation.
//!
//! Paints a fresh landscape into the [`Simulation`] grid from a single integer
//! seed: a noise-driven terrain heightmap (soil over stone), water pooled in
//! everything below sea level, and trees (wood trunks topped with leafy
//! canopies) scattered across the dry land. The same seed always produces the
//! same world.
//!
//! The terrain shape comes from [FastNoise Lite](https://github.com/Auburn/FastNoiseLite)
//! (the Rust port, [`fastnoise_lite`]): one octave-stacked OpenSimplex2 field
//! sampled along x gives the surface height of each column. Tree placement and
//! sizing use a tiny inline hash of `(seed, x)` so they're reproducible too,
//! without disturbing the simulation's own RNG.
//!
//! Generation only *places* cells; from there the normal tick loop takes over —
//! water flows and finds its level, fire (if any) spreads — so the world is
//! alive the moment it's drawn.

use fastnoise_lite::{FastNoiseLite, FractalType, NoiseType};

use crate::entities::{ANT, BIRD, FISH};
use crate::materials::{MaterialId, ALGAE, EMPTY, LEAVES, SAND, SOIL, STONE, WATER, WOOD};
use crate::sim::Simulation;

/// The seed the world opens with before the user picks their own.
pub const DEFAULT_SEED: i32 = 1337;

/// A choice of landscape preset, picked in the UI. Each maps to a [`WorldParams`]
/// that re-shapes the same generator — terrain profile, sea level, the materials
/// the ground is made of, and which flora and creatures get scattered about.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum WorldType {
    /// Rolling hills of soil over stone, pooled water, and trees — the original.
    #[default]
    Forest,
    /// Near-flat grassland: dry soil plains with the odd tree and ant, no water.
    Plains,
    /// A deep sea over a gently rolling sand bed, with algae and fish.
    Ocean,
    /// Arid sand dunes over stone, bone dry, with a few ants.
    Desert,
}

impl WorldType {
    /// Every preset, in the order the UI lists them.
    pub const ALL: [WorldType; 4] = [
        WorldType::Forest,
        WorldType::Plains,
        WorldType::Ocean,
        WorldType::Desert,
    ];

    /// The preset's display name for the picker.
    pub fn name(self) -> &'static str {
        match self {
            WorldType::Forest => "Forest",
            WorldType::Plains => "Plains",
            WorldType::Ocean => "Ocean",
            WorldType::Desert => "Desert",
        }
    }

    /// The generation knobs for this preset.
    fn params(self) -> WorldParams {
        match self {
            // Hilly soil-over-stone with a mid-screen waterline and full wildlife.
            WorldType::Forest => WorldParams {
                base: 0.5,
                amplitude: 0.3,
                frequency: 0.008,
                sea_level: 0.62,
                surface: SOIL,
                subsurface: STONE,
                trees: true,
                algae: true,
                ants: true,
                fish: true,
            },
            // Almost flat, dry grassland: soil over stone, no sea, sparse trees/ants.
            WorldType::Plains => WorldParams {
                base: 0.62,
                amplitude: 0.03,
                frequency: 0.012,
                sea_level: 1.0, // >= 1.0 ⇒ no water at all
                surface: SOIL,
                subsurface: STONE,
                trees: true,
                algae: false,
                ants: true,
                fish: false,
            },
            // A deep sea: water high up over a low, gently rolling sand bed.
            WorldType::Ocean => WorldParams {
                base: 0.9,
                amplitude: 0.05,
                frequency: 0.012,
                sea_level: 0.12,
                surface: SAND,
                subsurface: SAND,
                trees: false,
                algae: true,
                ants: false,
                fish: true,
            },
            // Arid dunes of sand over stone, no water, a scattering of ants.
            WorldType::Desert => WorldParams {
                base: 0.5,
                amplitude: 0.2,
                frequency: 0.006,
                sea_level: 1.0, // >= 1.0 ⇒ no water at all
                surface: SAND,
                subsurface: STONE,
                trees: false,
                algae: false,
                ants: true,
                fish: false,
            },
        }
    }
}

/// The tunable knobs a [`WorldType`] feeds the generator. Heights and the sea
/// level are fractions of the world's pixel height (0 = top, 1 = bottom).
struct WorldParams {
    /// Terrain band centre.
    base: f32,
    /// How far hills rise and valleys fall around `base`.
    amplitude: f32,
    /// Noise frequency — smaller is smoother, broader features.
    frequency: f32,
    /// Waterline; open air at or below it floods. `>= 1.0` leaves the world dry.
    sea_level: f32,
    /// The material of the ground's top [`SOIL_DEPTH`] cells.
    surface: MaterialId,
    /// The material of everything below the surface layer.
    subsurface: MaterialId,
    /// Scatter trees across the dry land?
    trees: bool,
    /// Tuft algae on the bed of deep pools?
    algae: bool,
    /// Drop ants on the dry ground?
    ants: bool,
    /// Seed fish in the deeper water?
    fish: bool,
}

/// How many cells of the ground's top layer are soil; everything below is stone.
const SOIL_DEPTH: usize = 7;

/// Roughly one in this many land columns sprouts a tree.
const TREE_RARITY: u32 = 11;

/// Roughly one in this many dry-land columns starts with an ant on it.
const ANT_RARITY: u32 = 40;

/// How many birds wheel over a freshly-generated world.
const BIRD_COUNT: u64 = 5;

/// Roughly one in this many submerged columns sprouts a tuft of algae on its bed.
const ALGAE_RARITY: u32 = 5;

/// Roughly one in this many submerged columns starts with a fish in it.
const FISH_RARITY: u32 = 22;

/// A column needs at least this much water depth to seed a fish, so they start
/// with room to swim rather than wedged into a shallow puddle.
const FISH_MIN_DEPTH: usize = 8;

/// Build and paint the default [`WorldType::Forest`] world for `seed`.
///
/// A thin convenience over [`generate_world`] for callers (the initial world,
/// the keyboard shortcuts) that just want the classic landscape.
pub fn generate(sim: &mut Simulation, seed: i32) {
    generate_world(sim, seed, WorldType::default());
}

/// Build and paint a complete `world` for `seed`, replacing whatever was there.
pub fn generate_world(sim: &mut Simulation, seed: i32, world: WorldType) {
    sim.clear();

    let p = world.params();
    let (w, h) = (sim.width, sim.height);
    let surface = heightmap(seed, w, h, &p);
    // The waterline, in cells. A fraction `>= 1.0` lands at (or past) the floor,
    // so no open-air cell is ever below it and the world stays bone dry.
    let sea_level = (h as f32 * p.sea_level) as usize;

    // ---- Terrain + water columns ----
    for x in 0..w {
        let top = surface[x];
        for y in 0..h {
            if y >= top {
                // Below the surface: a surface cap over a deeper base layer.
                let mat = if y < top + SOIL_DEPTH {
                    p.surface
                } else {
                    p.subsurface
                };
                sim.set(x, y, mat);
            } else if y >= sea_level {
                // Open air below sea level fills with water.
                sim.set(x, y, WATER);
            }
            // else: open sky, left empty.
        }
    }

    // ---- Trees ----
    // Only on dry land (surface above the waterline) and clear of the edges so a
    // canopy can't spill off the world.
    if p.trees {
        for x in 4..w.saturating_sub(4) {
            let top = surface[x];
            let on_dry_land = top < sea_level;
            if !on_dry_land || hash(seed, x as i64, 0) % TREE_RARITY != 0 {
                continue;
            }
            plant_tree(sim, seed, x, top);
        }
    }

    // ---- Algae ----
    // A few tufts on the bed of any deep-enough pool, in the water just above the
    // seabed, so newly-spawned fish have something to graze. It creeps from there.
    if p.algae {
        for x in 0..w {
            let top = surface[x];
            let submerged = top > sea_level;
            if !submerged || hash(seed, x as i64, 5) % ALGAE_RARITY != 0 {
                continue;
            }
            // The water cell sitting just above the seabed.
            let bed = top - 1;
            if bed >= sea_level {
                sim.set(x, bed, ALGAE);
            }
        }
    }

    // ---- Creatures ----
    // A scattering of ants ambling on the dry ground and a few birds aloft, so a
    // freshly-generated world already has some life in it. (Placement is seeded
    // by the same reproducible hash; the tick loop takes their motion from there.)
    if p.ants {
        for x in 4..w.saturating_sub(4) {
            let top = surface[x];
            if top >= sea_level || hash(seed, x as i64, 3) % ANT_RARITY != 0 {
                continue;
            }
            // Just above the surface cell, so the ant starts standing on the ground.
            sim.spawn_entity(ANT, x as i32, top as i32 - 1);
        }
    }
    for b in 0..BIRD_COUNT {
        // Spread the birds evenly across the width, scattered a little in height.
        let bx = (w as u64 * (2 * b + 1) / (2 * BIRD_COUNT)) as i32;
        let mut by = (h as f32 * 0.18) as i32 + (hash(seed, b as i64, 4) % 20) as i32;
        // Keep them in the open sky above any sea (the ocean's waterline sits high),
        // so they start wheeling over the water rather than submerged in it.
        by = by.min(sea_level as i32 - 2).max(2);
        sim.spawn_entity(BIRD, bx, by);
    }
    // Fish in the deeper pools, set partway down the water column so they begin
    // comfortably submerged.
    if p.fish {
        for x in 0..w {
            let top = surface[x];
            let deep_enough = top > sea_level + FISH_MIN_DEPTH;
            if !deep_enough || hash(seed, x as i64, 6) % FISH_RARITY != 0 {
                continue;
            }
            let fy = (sea_level + top) / 2;
            sim.spawn_entity(FISH, x as i32, fy as i32);
        }
    }
}

/// Surface height (the topmost ground cell's `y`) for every column, from a
/// fractal OpenSimplex2 field shaped by the preset's [`WorldParams`]. Higher
/// noise → higher ground → smaller `y`.
fn heightmap(seed: i32, w: usize, h: usize, p: &WorldParams) -> Vec<usize> {
    let mut noise = FastNoiseLite::with_seed(seed);
    noise.set_noise_type(Some(NoiseType::OpenSimplex2));
    noise.set_fractal_type(Some(FractalType::FBm));
    noise.set_fractal_octaves(Some(4));
    noise.set_frequency(Some(p.frequency));

    // Centre the terrain band per the preset, with hills/valleys either side,
    // clamped to leave sky above and a floor below.
    let base = h as f32 * p.base;
    let amplitude = h as f32 * p.amplitude;
    let min_y = 4usize;
    let max_y = h.saturating_sub(SOIL_DEPTH + 2);

    (0..w)
        .map(|x| {
            let n = noise.get_noise_2d(x as f32, 0.0); // ~[-1, 1]
            let y = base - n * amplitude;
            (y as i32).clamp(min_y as i32, max_y as i32) as usize
        })
        .collect()
}

/// Grow one tree: a vertical wood trunk rising from `(x, surface)` capped with a
/// blob of leaves. Sizes come from the seed/x hash, so trees are varied but
/// reproducible. Leaves are only written into empty sky, leaving the trunk —
/// and any neighbouring tree — visible.
fn plant_tree(sim: &mut Simulation, seed: i32, x: usize, surface: usize) {
    let trunk_h = 6 + (hash(seed, x as i64, 1) % 7) as usize; // 6..=12
    let radius = 3 + (hash(seed, x as i64, 2) % 3) as i32; // 3..=5

    // Don't bother if the trunk wouldn't fit under the canopy with sky to spare.
    if surface <= trunk_h + radius as usize + 1 {
        return;
    }

    // Trunk: the cell just above the ground, going up.
    let mut ty = surface - 1;
    for _ in 0..trunk_h {
        sim.set(x, ty, WOOD);
        ty -= 1;
    }

    // Canopy: a filled disk of leaves centred at the trunk's top, painted only
    // over empty cells so the trunk stays solid through its middle.
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

/// A small deterministic hash of `(seed, x, stream)` → `u32`, for reproducible
/// tree placement/sizing independent of the simulation's RNG. `stream` lets one
/// column draw several uncorrelated values (placement, height, radius).
fn hash(seed: i32, x: i64, stream: u64) -> u32 {
    let mut h = (seed as u32 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (x as u64).wrapping_mul(0xD1B5_4A32_D192_ED03)
        ^ stream.wrapping_mul(0xCA5A_826E_5C9C_7A1F);
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    h as u32
}
