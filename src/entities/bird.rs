//! Bird — a creature that wheels through the open sky.
//!
//! It cruises horizontally, wanders gently up and down, banks away from the
//! terrain below and the top of the world, and turns at the side walls — so it
//! drifts over the landscape rather than ever settling on it. As it grows hungry
//! it turns predator, diving on any ant it spots below and snatching it up, and
//! starves if the hunting is barren. See [`behaviors::hunt`].

use super::behaviors;
use super::{Entity, EntityInfo, EntityState, ANT};
use crate::sim::Simulation;

pub struct Bird;

/// A small gull silhouette: a body with a wing swept up on either side.
const SPRITE: &[(i8, i8)] = &[(0, 0), (-1, -1), (1, -1)];

/// Birds are predators: they hunt ants and graze on nothing.
const DIET: behaviors::Diet = behaviors::Diet {
    plants: &[],
    prey: &[ANT],
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
