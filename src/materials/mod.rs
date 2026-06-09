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
//! 3. Add it to [`builtins`].
//!
//! If it moves like something that already exists, point its `update` at the
//! matching `behaviors::*` helper and you're done. If it needs genuinely new
//! motion, add a helper in `behaviors.rs` and call that instead.
//!
//! # The registry is built at runtime
//!
//! The set of materials is no longer a compile-time constant: the built-ins are
//! seeded first (in a fixed order, so their ids/key-bindings never move), and
//! [`register`] appends more at runtime. That's what lets [`crate::plugin`] load
//! a material from a script the user drops in and have it show up in the picker.
//! Built-ins and plugins are the same `&dyn Material` to everyone else — `sim`,
//! `ui`, and `gpu` only ever go through [`get`]/[`count`] and never learn which
//! is which.

mod cloud;
mod empty;
mod fire;
mod lava;
mod leaves;
mod meteor;
mod oil;
mod rain;
mod sand;
mod seeds;
mod soil;
mod stone;
mod water;
mod wet_soil;
mod wood;

use std::cell::RefCell;

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
pub const OIL: MaterialId = 5;
pub const FIRE: MaterialId = 6;
/// Terrain materials, used by the world generator (`crate::worldgen`).
pub const SOIL: MaterialId = 7;
pub const WOOD: MaterialId = 8;
pub const LEAVES: MaterialId = 9;
/// Weather + growth materials. Clouds rain; rain wets soil; seeds sprout trees
/// from wet soil. See the matching files for the reaction chain.
pub const CLOUD: MaterialId = 10;
pub const RAIN: MaterialId = 11;
pub const WET_SOIL: MaterialId = 12;
pub const SEEDS: MaterialId = 13;
/// Summoned by the Meteor tool; explodes into fire and lava on impact.
pub const METEOR: MaterialId = 14;

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
    /// Whether this material emits light. Glowing cells are flagged for the
    /// renderer's bloom pass (it gives fire and lava their soft halo); see
    /// [`crate::sim::Simulation::render_into`] for how the flag is encoded.
    pub glow: bool,
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

/// One material kind. Built-ins are zero-sized; plugin materials carry a script
/// engine. Either way the registry stores them as `&'static dyn Material` (see
/// [`register`]), so [`get`] can hand one out without borrowing the simulation —
/// which is what lets `Simulation::step` call `get(id).update(self, …)` freely.
///
/// Not `Sync`: a plugin material owns a script interpreter that isn't shareable
/// across threads. The whole simulation runs on one thread, so that's fine and
/// the registry lives in a `thread_local` rather than a `static`.
pub trait Material {
    /// Static properties (name, colour, density, …).
    fn info(&self) -> MaterialInfo;

    /// Advance a single cell of this material by one tick. Should delegate to
    /// a shared helper in [`crate::behaviors`].
    fn update(&self, sim: &mut Simulation, x: usize, y: usize);

    /// Whether this material appears in the on-screen material picker (and is
    /// thus paintable by hand). Almost everything does; the exception is a
    /// material that only ever exists because *another* material spawns it —
    /// rain, which falls from clouds — so it has no business in the palette.
    /// Defaults to `true`, so a normal material needn't think about it.
    fn pickable(&self) -> bool {
        true
    }
}

/// ===================== ADD NEW BUILT-IN MATERIALS HERE =====================
/// The position here is the material's id, so keep `Empty` first and don't
/// reorder existing entries (key bindings / saved scenes use ids). Plugins are
/// appended after these by [`register`].
fn builtins() -> Vec<&'static dyn Material> {
    // ZSTs promoted to `'static`; referencing them gives `&'static dyn Material`.
    static EMPTY: empty::Empty = empty::Empty; // id 0
    static SAND: sand::Sand = sand::Sand; // id 1
    static STONE: stone::Stone = stone::Stone; // id 2
    static WATER: water::Water = water::Water; // id 3
    static LAVA: lava::Lava = lava::Lava; // id 4
    static OIL: oil::Oil = oil::Oil; // id 5
    static FIRE: fire::Fire = fire::Fire; // id 6
    static SOIL: soil::Soil = soil::Soil; // id 7
    static WOOD: wood::Wood = wood::Wood; // id 8
    static LEAVES: leaves::Leaves = leaves::Leaves; // id 9
    static CLOUD: cloud::Cloud = cloud::Cloud; // id 10
    static RAIN: rain::Rain = rain::Rain; // id 11
    static WET_SOIL: wet_soil::WetSoil = wet_soil::WetSoil; // id 12
    static SEEDS: seeds::Seeds = seeds::Seeds; // id 13
    static METEOR: meteor::Meteor = meteor::Meteor; // id 14
    vec![
        &EMPTY, &SAND, &STONE, &WATER, &LAVA, &OIL, &FIRE, &SOIL, &WOOD, &LEAVES, &CLOUD, &RAIN,
        &WET_SOIL, &SEEDS, &METEOR,
    ]
}

thread_local! {
    /// The live material table: built-ins, then any plugins, indexed by id.
    /// Single-threaded, so a `thread_local` is all the sharing we need.
    static REGISTRY: RefCell<Vec<&'static dyn Material>> = RefCell::new(builtins());
}

/// Look up a material by id. The returned reference is `'static` (it points into
/// the registry's leaked entries), so the borrow of the registry ends the moment
/// this returns — `update` can then freely take `&mut Simulation`, and a plugin
/// script running inside `update` can call back into [`get`] without deadlocking.
#[inline]
pub fn get(id: MaterialId) -> &'static dyn Material {
    REGISTRY.with(|r| r.borrow()[id as usize])
}

/// Number of registered materials (including Empty). Used by the material
/// picker to lay out one row per material.
pub fn count() -> usize {
    REGISTRY.with(|r| r.borrow().len())
}

/// Append a material to the registry and return its freshly-assigned id. The
/// material must be `'static`; plugin loaders leak their `ScriptMaterial` (it
/// lives for the rest of the run anyway) to satisfy this. Ids only ever grow, so
/// existing cells/selection stay valid. Returns `None` if the id space (`u8`) is
/// full.
pub fn register(material: &'static dyn Material) -> Option<MaterialId> {
    REGISTRY.with(|r| {
        let mut v = r.borrow_mut();
        if v.len() > MaterialId::MAX as usize {
            return None;
        }
        let id = v.len() as MaterialId;
        v.push(material);
        Some(id)
    })
}

/// Find a material by (case-insensitive) name, e.g. so a plugin can ask for the
/// id of `"Water"` to react with it. Returns the first match.
pub fn id_by_name(name: &str) -> Option<MaterialId> {
    REGISTRY.with(|r| {
        r.borrow()
            .iter()
            .position(|m| m.info().name.eq_ignore_ascii_case(name))
            .map(|i| i as MaterialId)
    })
}
