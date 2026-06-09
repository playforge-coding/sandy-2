//! Ant — a tiny creature that crawls along the terrain.
//!
//! It paces back and forth over whatever solid ground it finds, climbing small
//! steps, turning at walls and ledges, and falling if the ground gives way. As
//! it goes hungry it browses the foliage — heading for the nearest leaves and
//! eating them — and starves if it can find none. It drowns if it wanders into
//! water (or burns in lava). See [`behaviors::graze`].

use super::behaviors;
use super::{Entity, EntityInfo, EntityState};
use crate::materials::LEAVES;
use crate::sim::Simulation;

pub struct Ant;

/// A three-pixel speck — a body with a head in front — dark against the terrain.
const SPRITE: &[(i8, i8)] = &[(0, 0), (1, 0), (0, -1)];

/// Ants are grazers: they browse leaves and prey on nothing, sniffing out a patch
/// only within a short range so they graze nearby rather than roam.
const DIET: behaviors::Diet = behaviors::Diet {
    plants: &[LEAVES],
    prey: &[],
    plant_sense: 16,
    prey_sense: 0.0,
};

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
        behaviors::graze(sim, me, &DIET);
    }
}
