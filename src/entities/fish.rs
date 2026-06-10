//! Fish — a creature that swims through water.
//!
//! It cruises about within a pool, drifting up and down but always staying under
//! the surface and off the bottom, and turns back at the water's edge rather than
//! beaching itself. As it grows hungry it browses [`ALGAE`], and — being an
//! opportunist — if an ant strays close along the bank it lines up beneath it and
//! *leaps* clear of the water to snatch it, splashing back down after. Out of
//! water too long it suffocates, and (like everything) it starves if it can find
//! nothing to eat — and, like any creature, dies at once in lava or fire. See
//! [`behaviors::swim`].

use super::behaviors;
use super::{Entity, EntityInfo, EntityState, ANT};
use crate::materials::ALGAE;
use crate::sim::Simulation;

pub struct Fish;

/// A small dart of a body with a tail flicked out behind it.
const SPRITE: &[(i8, i8)] = &[(0, 0), (1, 0), (-1, 0), (-2, -1), (-2, 1)];

/// Fish graze algae and ambush ants that wander near the water. Their eyesight
/// for prey is short — they only lunge at an ant that strays close to the bank,
/// not one off across the map — but they can smell an algae patch a fair way off.
const DIET: behaviors::Diet = behaviors::Diet {
    plants: &[ALGAE],
    prey: &[ANT],
    plant_sense: 22,
    prey_sense: 13.0,
};

impl Entity for Fish {
    fn info(&self) -> EntityInfo {
        EntityInfo {
            name: "Fish",
            color: [196, 132, 70, 255],
            glow: false,
            sprite: SPRITE,
        }
    }

    fn update(&self, sim: &mut Simulation, me: &mut EntityState) {
        behaviors::swim(sim, me, &DIET);
    }
}
