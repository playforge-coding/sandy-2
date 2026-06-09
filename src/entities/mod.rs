//! The entity system — creatures that live *on* the grid rather than *in* it.
//!
//! Where a [`crate::materials`] cell is a fixed point in the grid that only ever
//! becomes some other material, an **entity** is a discrete, mobile agent with
//! its own position and a scrap of state ([`EntityState`]): an ant ambling along
//! the ground, a bird wheeling through the sky. The grid is its world — it senses
//! the cells around it and walks or flies over them — but it isn't stored in the
//! grid, so two ants can pass the same cell and a bird can glide over open air
//! that holds nothing.
//!
//! The shape of this module deliberately echoes `materials`: each kind lives in
//! **its own file** (`ant.rs`, `bird.rs`, …) and implements the [`Entity`] trait,
//! declaring its look via [`EntityInfo`] and delegating its per-tick motion to a
//! shared helper in [`behaviors`]. The [`REGISTRY`] maps a [`EntityKindId`] to the
//! one zero-sized value that describes that kind's behaviour; the *instances* (an
//! ant here, a bird there) live in the [`crate::sim::Simulation`] as a list of
//! [`EntityState`].
//!
//! # Adding a new creature
//!
//! 1. Create `src/entities/<name>.rs` (copy `ant.rs`).
//! 2. `mod <name>;` it below.
//! 3. Add it to [`builtins`].
//!
//! If it moves like something that already exists, point its `update` at the
//! matching `behaviors::*` helper. If it needs genuinely new motion, add a helper
//! in `behaviors.rs` and call that.

mod ant;
mod behaviors;
mod bird;
mod fish;

use std::cell::RefCell;

use crate::sim::Simulation;

/// An entity-kind identifier — which *sort* of creature this is (ant, bird, …),
/// indexing into [`REGISTRY`]. Distinct from a [`crate::materials::MaterialId`]:
/// it names a behaviour, not a cell.
pub type EntityKindId = u8;

/// Named kinds, for code that spawns a specific creature (the placement tool, the
/// world generator). Must match the positions in [`builtins`].
pub const ANT: EntityKindId = 0;
pub const BIRD: EntityKindId = 1;
pub const FISH: EntityKindId = 2;

/// A creature kind's static, render-relevant properties — the entity cousin of
/// [`crate::materials::MaterialInfo`].
#[derive(Clone, Copy)]
pub struct EntityInfo {
    /// Human-readable name, shown in the creature picker.
    pub name: &'static str,
    /// Body colour, RGBA, 0–255. (As with materials, the stored alpha doubles as
    /// the renderer's glow flag — see [`crate::sim::Simulation::render_into`].)
    pub color: [u8; 4],
    /// Whether the creature emits light (a firefly might); flagged for the bloom
    /// pass, exactly like a glowing material.
    pub glow: bool,
    /// The creature's body as a little set of pixel offsets from its position,
    /// stamped over the grid each frame. A handful of cells is plenty at this
    /// resolution: an ant is a speck, a bird a small silhouette.
    pub sprite: &'static [(i8, i8)],
}

/// The mutable, per-*instance* state of one live creature — the part that moves.
/// `Copy` (everything in it is), so [`crate::sim::Simulation::step`] can lift a
/// creature out of the live list, let it think against `&mut Simulation`, and
/// write the result back without any borrow tangle (see `step_entities`).
#[derive(Clone, Copy)]
pub struct EntityState {
    /// Which kind this is — indexes [`REGISTRY`] for behaviour and look.
    pub kind: EntityKindId,
    /// Position in grid cells, sub-cell precise so motion is smooth even though
    /// it's drawn at the nearest cell.
    pub x: f32,
    pub y: f32,
    /// Velocity in cells/tick. Walkers (ants) barely use it; fliers (birds)
    /// integrate it every tick.
    pub vx: f32,
    pub vy: f32,
    /// Facing: `-1` left, `+1` right. The heading a walker paces along and a
    /// flier cruises in.
    pub dir: i8,
    /// A free-running per-creature clock, incremented each tick. Behaviours use
    /// it to pace themselves (an ant steps every few ticks, not every one).
    pub timer: u32,
    /// How peckish the creature is, climbing each tick it goes unfed. A forager
    /// (see [`behaviors::graze`]/[`behaviors::hunt`]) starts looking for food once
    /// this passes a threshold, resets it to zero when it eats, and is reaped if
    /// it climbs all the way to starvation. Creatures with no appetite simply
    /// never consult it.
    pub hunger: u16,
    /// Ticks a water creature has spent out of water — climbing while it's in the
    /// air (a fish that beached itself, or is mid-leap) and reset the moment it's
    /// back under. A swimmer suffocates if it stays out too long; land and air
    /// creatures never touch it. See [`behaviors::swim`].
    pub air: u16,
    /// Cleared by a behaviour to have the creature reaped at the end of the tick
    /// (an ant that wandered into water drowns, or one that starved).
    pub alive: bool,
}

impl EntityState {
    /// A fresh creature of `kind` at `(x, y)` facing `dir`, at rest.
    pub fn new(kind: EntityKindId, x: f32, y: f32, dir: i8) -> Self {
        Self {
            kind,
            x,
            y,
            vx: 0.0,
            vy: 0.0,
            dir,
            timer: 0,
            hunger: 0,
            air: 0,
            alive: true,
        }
    }
}

/// One creature kind. Built-ins are zero-sized; the registry stores them as
/// `&'static dyn Entity` so [`get`] hands one out without borrowing the
/// simulation — which is what lets `Simulation::step` call
/// `get(kind).update(self, …)` with `self` borrowed mutably. The mirror of
/// [`crate::materials::Material`], for agents instead of cells.
pub trait Entity {
    /// Static properties (name, colour, sprite).
    fn info(&self) -> EntityInfo;

    /// Advance one creature by a tick: read the world through `sim`, move and
    /// otherwise mutate `me`. Should delegate to a shared helper in
    /// [`behaviors`].
    fn update(&self, sim: &mut Simulation, me: &mut EntityState);
}

/// ===================== ADD NEW BUILT-IN CREATURES HERE =====================
/// The position here is the kind's id, so don't reorder existing entries.
fn builtins() -> Vec<&'static dyn Entity> {
    static ANT: ant::Ant = ant::Ant; // id 0
    static BIRD: bird::Bird = bird::Bird; // id 1
    static FISH: fish::Fish = fish::Fish; // id 2
    vec![&ANT, &BIRD, &FISH]
}

thread_local! {
    /// The live kind table, indexed by [`EntityKindId`]. Single-threaded, so a
    /// `thread_local` is all the sharing we need — same as the material registry.
    static REGISTRY: RefCell<Vec<&'static dyn Entity>> = RefCell::new(builtins());
}

/// Look up a creature kind by id. The returned reference is `'static` (it points
/// at a leaked/`static` built-in), so the registry borrow ends as this returns
/// and `update` is free to take `&mut Simulation`.
#[inline]
pub fn get(kind: EntityKindId) -> &'static dyn Entity {
    REGISTRY.with(|r| r.borrow()[kind as usize])
}

/// Number of registered creature kinds. Used by the picker to lay out a button
/// per kind.
pub fn count() -> usize {
    REGISTRY.with(|r| r.borrow().len())
}
