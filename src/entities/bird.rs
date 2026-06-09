//! Bird — a creature that wheels through the open sky.
//!
//! It cruises horizontally, wanders gently up and down, banks away from the
//! terrain below and the top of the world, and turns at the side walls — so it
//! drifts over the landscape rather than ever settling on it. As it grows hungry
//! it turns predator, diving on prey it spots below — a fish at the water's
//! surface for choice, an ant on the ground otherwise — and snatching it up, and
//! starves if the hunting is barren. See [`behaviors::hunt`].

use super::behaviors;
use super::{Entity, EntityInfo, EntityState, ANT, FISH};
use crate::sim::Simulation;

pub struct Bird;

/// A small gull silhouette: a body with a wing swept up on either side.
const SPRITE: &[(i8, i8)] = &[(0, 0), (-1, -1), (1, -1)];

/// Birds are predators that graze on nothing and hunt with keen, long-range
/// eyesight. Their prey is listed fish-first, so a bird always stoops on a fish it
/// can see over an ant (see [`behaviors::Diet`]).
const DIET: behaviors::Diet = behaviors::Diet {
    plants: &[],
    prey: &[FISH, ANT],
    plant_sense: 0,
    prey_sense: 240.0,
};

impl Entity for Bird {
    fn info(&self) -> EntityInfo {
        EntityInfo {
            name: "Bird",
            color: [40, 42, 54, 255],
            glow: false,
            sprite: SPRITE,
        }
    }

    fn update(&self, sim: &mut Simulation, me: &mut EntityState) {
        behaviors::hunt(sim, me, &DIET);
    }
}
