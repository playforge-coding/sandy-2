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

use crate::materials::{EMPTY, LEAVES, SOIL, STONE, WATER, WOOD};
use crate::sim::Simulation;

/// The seed the world opens with before the user picks their own.
pub const DEFAULT_SEED: i32 = 1337;

/// How many cells of the ground's top layer are soil; everything below is stone.
const SOIL_DEPTH: usize = 7;

/// Roughly one in this many land columns sprouts a tree.
const TREE_RARITY: u32 = 11;

/// Build and paint a complete world for `seed`, replacing whatever was there.
pub fn generate(sim: &mut Simulation, seed: i32) {
    sim.clear();

    let (w, h) = (sim.width, sim.height);
    let surface = heightmap(seed, w, h);
    // Sea level sits a little below the middle of the screen, so lower terrain
    // drowns and higher terrain stays dry.
    let sea_level = (h as f32 * 0.62) as usize;

    // ---- Terrain + water columns ----
    for x in 0..w {
        let top = surface[x];
        for y in 0..h {
            if y >= top {
                // Below the surface: a soil cap over a stone base.
                let mat = if y < top + SOIL_DEPTH { SOIL } else { STONE };
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
    for x in 4..w.saturating_sub(4) {
        let top = surface[x];
        let on_dry_land = top < sea_level;
        if !on_dry_land || hash(seed, x as i64, 0) % TREE_RARITY != 0 {
            continue;
        }
        plant_tree(sim, seed, x, top);
    }
}

/// Surface height (the topmost ground cell's `y`) for every column, from a
/// fractal OpenSimplex2 field. Higher noise → higher ground → smaller `y`.
fn heightmap(seed: i32, w: usize, h: usize) -> Vec<usize> {
    let mut noise = FastNoiseLite::with_seed(seed);
    noise.set_noise_type(Some(NoiseType::OpenSimplex2));
    noise.set_fractal_type(Some(FractalType::FBm));
    noise.set_fractal_octaves(Some(4));
    noise.set_frequency(Some(0.008));

    // Centre the terrain band around mid-screen with hills/valleys either side,
    // clamped to leave sky above and a floor below.
    let base = h as f32 * 0.5;
    let amplitude = h as f32 * 0.3;
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
    let mut h = (seed as u32 as u64)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (x as u64).wrapping_mul(0xD1B5_4A32_D192_ED03)
        ^ stream.wrapping_mul(0xCA5A_826E_5C9C_7A1F);
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    h as u32
}
