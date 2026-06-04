//! The material system.
//!
//! Each material lives in **its own file** (`sand.rs`, `stone.rs`, …) and
//! implements the [`Material`] trait: it declares its static properties via
//! [`MaterialInfo`] and delegates its per-tick motion to a shared helper in
//! [`crate::behaviors`]. The [`REGISTRY`] below maps ids to materials.
//!
//! # Adding a new material
//!
//! 1. Create `src/materials/<name>.rs` (copy `sand.rs` or `stone.rs`).
//! 2. `mod <name>;` it below.
//! 3. Add `&<name>::<Type>` to [`REGISTRY`].
//!
//! If it moves like something that already exists, point its `update` at the
//! matching `behaviors::*` helper and you're done. If it needs genuinely new
//! motion, add a helper in `behaviors.rs` and call that instead.

mod empty;
mod lava;
mod sand;
mod stone;
mod water;

use crate::sim::Simulation;

/// A material identifier. `0` is always [`EMPTY`]; every other value indexes
/// into [`REGISTRY`].
pub type MaterialId = u8;

/// The empty cell (air / nothing). Always id `0`.
pub const EMPTY: MaterialId = 0;
/// Named ids for materials that other materials react with. These must match
/// the positions in [`REGISTRY`].
pub const STONE: MaterialId = 2;
pub const WATER: MaterialId = 3;
pub const LAVA: MaterialId = 4;

/// A material's static, render- and physics-relevant properties.
#[derive(Clone, Copy)]
pub struct MaterialInfo {
    /// Human-readable name, shown in the material picker.
    pub name: &'static str,
    /// Base colour, RGBA, 0–255.
    pub color: [u8; 4],
    /// Per-cell brightness jitter (0 = flat). Gives powders a grainy look.
    pub jitter: u8,
    /// Used for sinking: a denser *movable* material displaces a lighter one.
    pub density: u8,
    /// Whether other materials can displace this one (false for solids/air).
    pub movable: bool,
}

impl MaterialInfo {
    /// The material's representative (average) colour, for UI swatches. Per-cell
    /// jitter is symmetric about the base colour, so over many cells it averages
    /// out and the base `color` *is* the mean — no need to sample variants.
    pub fn average_color(&self) -> [u8; 4] {
        #[warn(dead_code)]
        self.color
    }

    /// Resolve this material's colour for a cell, applying per-cell jitter
    /// using the cell's stored random `variant`.
    pub fn shade(&self, variant: u8) -> [u8; 4] {
        if self.jitter == 0 {
            return self.color;
        }
        let offset = ((variant as i32) - 128) * (self.jitter as i32) / 128;
        let ch = |v: u8| (v as i32 + offset).clamp(0, 255) as u8;
        [
            ch(self.color[0]),
            ch(self.color[1]),
            ch(self.color[2]),
            self.color[3],
        ]
    }
}

/// One material kind. Implemented once per material file. Implementors are
/// zero-sized and stored as `'static` trait objects in [`REGISTRY`].
pub trait Material: Sync {
    /// Static properties (name, colour, density, …).
    fn info(&self) -> MaterialInfo;

    /// Advance a single cell of this material by one tick. Should delegate to
    /// a shared helper in [`crate::behaviors`].
    fn update(&self, sim: &mut Simulation, x: usize, y: usize);
}

/// ===================== ADD NEW MATERIALS HERE =====================
/// The position in this array is the material's id, so keep `Empty` first and
/// don't reorder existing entries (key bindings / saved scenes use ids).
pub static REGISTRY: &[&dyn Material] = &[
    &empty::Empty, // id 0
    &sand::Sand,   // id 1
    &stone::Stone, // id 2
    &water::Water, // id 3
    &lava::Lava,   // id 4
];

/// Look up a material by id. `'static` — never borrows the simulation, which
/// is what lets `Simulation::step` call `get(id).update(self, …)` freely.
#[inline]
pub fn get(id: MaterialId) -> &'static dyn Material {
    REGISTRY[id as usize]
}

/// Number of registered materials (including Empty). Used by the material
/// picker to lay out one row per material.
pub fn count() -> usize {
    REGISTRY.len()
}
