//! Ant — a tiny creature that crawls along the terrain.
//!
//! It paces back and forth over whatever solid ground it finds, climbing small
//! steps, turning at walls and ledges, and falling if the ground gives way. It
//! drowns if it wanders into water (or burns in lava) — see
//! [`behaviors::walk`].

use super::behaviors;
use super::{Entity, EntityInfo, EntityState};
use crate::sim::Simulation;

pub struct Ant;

/// A three-pixel speck — a body with a head in front — dark against the terrain.
const SPRITE: &[(i8, i8)] = &[(0, 0), (1, 0), (0, -1)];

impl Entity for Ant {
    fn info(&self) -> EntityInfo {
        EntityInfo {
            name: "Ant",
            color: [40, 26, 18, 255],
            glow: false,
            sprite: SPRITE,
        }
    }

    fn update(&self, sim: &mut Simulation, me: &mut EntityState) {
        behaviors::walk(sim, me);
    }
}
