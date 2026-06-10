//! Shared movement logic that creatures reuse — the entity cousin of
//! [`crate::behaviors`].
//!
//! A creature's `update` should be tiny: it just delegates to one of these. That
//! way every walker shares one well-behaved "amble along the ground" routine and
//! every flier one "wheel through the sky" routine, and a new creature gets that
//! motion for free by calling the same helper.
//!
//! These sense the world through the same `pub(crate)` grid helpers the material
//! behaviours use (`cell_mat`, `chance`, `rand_f32`), so they never need to know
//! how the grid is stored.

use crate::materials::{MaterialId, ALGAE, CLOUD, EMPTY, FIRE, LAVA, LEAVES, OIL, WATER};
use crate::sim::Simulation;

use super::{EntityKindId, EntityState};

/// Liquids a land creature drowns or burns in if it ends up standing in one.
fn is_liquid(m: MaterialId) -> bool {
    matches!(m, WATER | LAVA | OIL)
}

/// Molten lava and open flame: the hazards that kill *any* creature outright,
/// whatever element it lives in. Water and oil only drown a land-dweller (a fish
/// is at home in water), so they're handled separately by the walker — but a bird
/// in the flames or a fish flung into lava dies just the same.
fn lethal(m: MaterialId) -> bool {
    matches!(m, LAVA | FIRE)
}

/// Reap the creature if it's sitting in lava or fire. Returns `true` (so the
/// caller bails out of the rest of its turn) when it died. Shared by every
/// forager, so birds, fish, and ants all perish in the heat alike.
fn burned(sim: &Simulation, me: &mut EntityState) -> bool {
    let (x, y) = (me.x.round() as i32, me.y.round() as i32);
    if matches!(sim.cell_mat(x, y), Some(m) if lethal(m)) {
        me.alive = false;
        return true;
    }
    false
}

/// Whether a cell is solid footing or a wall to a creature: any filled,
/// non-liquid cell. The world's sides and floor count as solid (so a walker
/// can't trundle off the edge of the world); the open sky *above* the world does
/// not, so a flier near the top isn't fenced in by phantom ceiling.
fn solid(sim: &Simulation, x: i32, y: i32) -> bool {
    match sim.cell_mat(x, y) {
        Some(m) => m != EMPTY && !is_liquid(m),
        // Off-grid: sides and the floor block (`y >= 0`); above the top is sky.
        None => y >= 0,
    }
}

/// Whether a cell blocks a flier's path. Like [`solid`], but soft, wispy cells —
/// a tree's leafy canopy and clouds — don't count, so a bird or breaching fish
/// wings straight through them rather than bouncing off. (Walkers still treat
/// leaves as solid, so an ant perches on and grazes the foliage as before.)
fn obstacle(sim: &Simulation, x: i32, y: i32) -> bool {
    solid(sim, x, y) && !matches!(sim.cell_mat(x, y), Some(LEAVES) | Some(CLOUD))
}

/// Ticks between an ambling creature's steps. Higher = a slower walk; moving
/// every tick would have an ant sprint across the world.
const WALK_STEP_TICKS: u32 = 3;

/// Walk along solid surfaces: fall under gravity until something is underfoot,
/// then amble in the facing direction — climbing a one-cell step, easing down a
/// one-cell drop, and turning around at walls and ledges. Drowns the creature
/// the instant it's standing in a liquid. Shared by ants and any future
/// ground-crawler (beetles, worms, …).
pub fn walk(sim: &mut Simulation, me: &mut EntityState) {
    me.timer = me.timer.wrapping_add(1);
    let x = me.x.round() as i32;
    let y = me.y.round() as i32;

    // Standing in liquid: drown or burn.
    if let Some(m) = sim.cell_mat(x, y) {
        if is_liquid(m) {
            me.alive = false;
            return;
        }
    }

    // Gravity: nothing solid directly underfoot → fall one cell this tick.
    if !solid(sim, x, y + 1) {
        me.y += 1.0;
        return;
    }

    // Amble at a walking pace rather than a cell every single tick.
    if me.timer % WALK_STEP_TICKS != 0 {
        return;
    }

    let ahead = x + me.dir as i32;
    if solid(sim, ahead, y) {
        // Wall ahead: climb a single-cell step if there's headroom, else turn.
        if !solid(sim, ahead, y - 1) {
            me.x = ahead as f32;
            me.y = (y - 1) as f32;
        } else {
            me.dir = -me.dir;
        }
    } else if solid(sim, ahead, y + 1) {
        // Flat ground continues ahead: step onto it.
        me.x = ahead as f32;
    } else if solid(sim, ahead, y + 2) {
        // A one-cell drop with ground below it: follow the slope down.
        me.x = ahead as f32;
        me.y = (y + 1) as f32;
    } else {
        // A sheer ledge with nothing to step onto: turn back rather than march
        // off into the air.
        me.dir = -me.dir;
    }
}

/// Cruising speed of a flier, in cells/tick.
const FLY_SPEED: f32 = 1.1;
/// Start climbing when terrain is within this many cells below.
const GROUND_CLEARANCE: i32 = 8;
/// Keep this many cells clear of the top of the world.
const SKY_MARGIN: f32 = 6.0;
/// Per-tick damping on vertical speed, so a flier levels out instead of
/// careening up or down forever.
const VY_DAMPING: f32 = 0.92;

/// Wheel through open air: cruise horizontally in the facing direction, wander
/// gently up and down, steer away from terrain below and the ceiling above, and
/// turn at side walls or anything solid dead ahead. Shared by birds and any
/// future flier (bats, butterflies, …).
pub fn fly(sim: &mut Simulation, me: &mut EntityState) {
    me.timer = me.timer.wrapping_add(1);

    // Pick up the initial heading from the facing direction.
    if me.vx == 0.0 {
        me.vx = me.dir as f32 * FLY_SPEED;
    }

    let x = me.x.round() as i32;
    let y = me.y.round() as i32;

    // A little vertical wandering, damped so it bobs rather than drifts away.
    if sim.chance(8) {
        me.vy += sim.rand_f32() - 0.5;
    }
    me.vy *= VY_DAMPING;

    // Steer clear of the ground below and the ceiling above (winging through
    // leaves, not over them).
    if (1..=GROUND_CLEARANCE).any(|d| obstacle(sim, x, y + d)) {
        me.vy -= 0.5; // climb away from the terrain
    }
    if me.y < SKY_MARGIN {
        me.vy += 0.4; // dip away from the top of the world
    }
    me.vy = me.vy.clamp(-1.4, 1.4);

    // Horizontal: turn at a side wall or any terrain straight ahead.
    let nx = me.x + me.vx;
    let ahead = nx.round() as i32;
    if nx < 1.0 || nx >= (sim.width - 1) as f32 || obstacle(sim, ahead, y) {
        me.vx = -me.vx;
        me.dir = -me.dir;
    } else {
        me.x = nx;
    }

    me.y = (me.y + me.vy).clamp(1.0, (sim.height - 2) as f32);
}

// ───────────────────────────── foraging ──────────────────────────────
//
// Two primitives — [`search`] (find food and point at it) and [`eat`] (consume
// food within reach) — plus the hunger bookkeeping that drives them, and two
// ready-made foragers that fold them into the movement above: [`graze`] for a
// ground-crawler that browses plants, [`hunt`] for a flier that chases prey.

/// What a creature feeds on, consulted by the foraging behaviours: which terrain
/// cells it can browse (an ant on [`LEAVES`](crate::materials::LEAVES)) and which
/// creature kinds it can hunt (a bird on [`ANT`](crate::entities::ANT)). Either
/// list may be empty — a pure grazer has no `prey`, a pure predator no `plants`.
///
/// The `prey` list is **priority-ordered**: a hunter goes for the first kind it
/// can find, only falling back to later kinds when none is in range. A bird that
/// lists `[FISH, ANT]` thus always stoops on a fish it can see in preference to
/// any ant. Prey is in turn preferred over plants (a creature is the bigger meal).
///
/// A creature must **never** list its own kind in `prey`: a hunter senses prey
/// through the live entity list, where it still appears as a stale copy of itself
/// while it thinks (see [`crate::sim::Simulation::step_entities`]), so it would
/// hunt itself. Eating a different kind is always safe.
pub struct Diet {
    /// Terrain materials this creature grazes, cleared to air when eaten.
    pub plants: &'static [MaterialId],
    /// Creature kinds this creature preys on, in priority order, reaped when eaten.
    pub prey: &'static [EntityKindId],
    /// How far this creature can smell out a plant patch, in cells. Short keeps a
    /// grazer browsing what's around it rather than marching across the world.
    pub plant_sense: i32,
    /// How far this creature can *see* prey, in cells. A high-flying hunter sees a
    /// long way (and stoops from afar); an ambusher only senses prey close by.
    pub prey_sense: f32,
}

/// Hunger gained per tick a creature goes unfed.
const HUNGER_PER_TICK: u16 = 1;
/// Once hunger passes this, a forager stops idling and goes looking for food.
const HUNGRY: u16 = 120;
/// Once hunger reaches this, the creature has starved and is reaped.
const STARVING: u16 = 2000;
/// How near food must be for [`eat`] to reach it, in cells (a little over a
/// cell's diagonal, so anything orthogonally or diagonally adjacent counts).
const EAT_REACH: f32 = 2.5;
/// A hunting flier's chase speed — brisker than its idle cruise.
const DIVE_SPEED: f32 = 1.4;
/// Chance (1 in this) that a meal leaves a creature well-fed enough to breed.
/// A creature can't eat again until its hunger rebuilds past [`HUNGRY`], so this
/// paces population growth to roughly one offspring per several meals — kept in
/// check by how much food (and, for predators, prey) the world can supply.
const BREED_CHANCE: u32 = 8;

/// Tick the hunger clock and reap the creature if it has starved. Returns `true`
/// when it died, so the caller can bail out of the rest of its turn.
fn hunger_tick(me: &mut EntityState) -> bool {
    me.hunger = me.hunger.saturating_add(HUNGER_PER_TICK);
    if me.hunger >= STARVING {
        me.alive = false;
        return true;
    }
    false
}

/// Search for food: sense the nearest edible thing within range — a prey
/// creature or a plant cell, whichever is closer — and return the vector from the
/// creature to it, or `None` if nothing edible is near. Pure sensing: it moves
/// nothing itself, leaving the forager to decide how to act on the bearing. The
/// "search for food" primitive, shared by every forager.
pub fn search(sim: &Simulation, me: &EntityState, diet: &Diet) -> Option<(f32, f32)> {
    // Prey first, in priority order: the nearest of the highest-priority kind that
    // has anything in range wins, so a bird eyeing both a fish and an ant goes for
    // the fish (see [`Diet`]).
    let prey = diet.prey.iter().find_map(|&kind| {
        sim.nearest_entity(me.x, me.y, &[kind], diet.prey_sense)
            .map(|(_, dx, dy)| (dx, dy))
    });
    if prey.is_some() {
        return prey;
    }
    // No prey about: head for the nearest plant patch instead.
    if diet.plants.is_empty() {
        return None;
    }
    sim.nearest_cell(
        me.x.round() as i32,
        me.y.round() as i32,
        diet.plants,
        diet.plant_sense,
    )
    .map(|(cx, cy)| (cx as f32 - me.x, cy as f32 - me.y))
}

/// Eat any food within [`EAT_REACH`]: a prey creature is reaped, or an adjacent
/// plant cell (or the one underfoot) is cleared to air. Resets hunger and returns
/// `true` if it ate. The entity cousin of a material reaction — it consumes a
/// neighbour in place rather than moving. Prey is preferred over plants when both
/// are in reach (a creature is the bigger meal).
pub fn eat(sim: &mut Simulation, me: &mut EntityState, diet: &Diet) -> bool {
    // Prey in priority order — snatch the highest-priority kind within reach.
    for &kind in diet.prey {
        if let Some((i, _, _)) = sim.nearest_entity(me.x, me.y, &[kind], EAT_REACH) {
            sim.reap_entity(i);
            me.hunger = 0;
            return true;
        }
    }
    if !diet.plants.is_empty() {
        let (x, y) = (me.x.round() as i32, me.y.round() as i32);
        // The cell underfoot first, then the four orthogonal neighbours.
        const SPOTS: [(i32, i32); 5] = [(0, 0), (0, -1), (0, 1), (-1, 0), (1, 0)];
        for (dx, dy) in SPOTS {
            let (cx, cy) = (x + dx, y + dy);
            if cx < 0 || cy < 0 || cx as usize >= sim.width || cy as usize >= sim.height {
                continue;
            }
            let m = sim.mat_at(cx as usize, cy as usize);
            if diet.plants.contains(&m) {
                // Aquatic plants grow *in* water, so grazing one leaves the water
                // it stood in behind — not an air pocket the grazer would then
                // "drown" out of. Land plants just leave bare air.
                let left = if m == ALGAE { WATER } else { EMPTY };
                sim.set(cx as usize, cy as usize, left);
                me.hunger = 0;
                return true;
            }
        }
    }
    false
}

/// Breed off the back of a meal: with [`BREED_CHANCE`] odds, drop a fresh
/// offspring of the creature's own kind beside it. Called the moment it eats, so
/// a creature only multiplies while it's finding food — well-fed populations grow
/// and starving ones don't. The newborn starts at rest and full (see
/// [`EntityState::new`](super::EntityState::new)); the simulation caps the total,
/// so this can't run away.
pub fn breed(sim: &mut Simulation, me: &EntityState) {
    if !sim.chance(BREED_CHANCE) {
        return;
    }
    // Lay the young one a step to the side so parent and child don't set off
    // perfectly superimposed; off-grid or over-capacity spawns are no-ops.
    let (x, y) = (me.x.round() as i32 + me.dir as i32, me.y.round() as i32);
    sim.spawn_entity(me.kind, x, y);
}

/// A grazing ground-crawler: amble along the terrain ([`walk`]) until hunger
/// bites, then browse — eat any plant within reach, else turn toward the nearest
/// patch sensed and walk to it. Starves if it goes too long unfed. Shared by ants
/// and any future browser.
pub fn graze(sim: &mut Simulation, me: &mut EntityState, diet: &Diet) {
    if burned(sim, me) || hunger_tick(me) {
        return;
    }
    if me.hunger >= HUNGRY {
        // Already next to food? Eat (and maybe breed) and be done for the tick.
        if eat(sim, me, diet) {
            breed(sim, me);
            return;
        }
        // Otherwise head toward whatever was sensed; `walk` does the stepping.
        if let Some((dx, _)) = search(sim, me, diet) {
            if dx.abs() > 0.5 {
                me.dir = if dx > 0.0 { 1 } else { -1 };
            }
        }
    }
    walk(sim, me);
}

/// A hunting flier: wheel through the sky ([`fly`]) until hunger bites, then hunt
/// — eat any prey within reach, else dive toward the nearest creature sensed.
/// Falls back to ordinary cruising when there's nothing to chase, and starves if
/// it goes too long unfed. Shared by birds and any future aerial predator.
pub fn hunt(sim: &mut Simulation, me: &mut EntityState, diet: &Diet) {
    if burned(sim, me) || hunger_tick(me) {
        return;
    }
    if me.hunger >= HUNGRY {
        if eat(sim, me, diet) {
            breed(sim, me);
            return;
        }
        if let Some((dx, dy)) = search(sim, me, diet) {
            dive(sim, me, dx, dy);
            return;
        }
    }
    fly(sim, me);
}

/// Bank a flier toward prey at offset `(dx, dy)`: a focused, faster version of
/// [`fly`] used mid-chase. Aims straight at the prey but refuses to plough into
/// the ground or a wall — it climbs over terrain close below and reverts to
/// ordinary flight if something solid blocks the way ahead.
fn dive(sim: &mut Simulation, me: &mut EntityState, dx: f32, dy: f32) {
    me.timer = me.timer.wrapping_add(1);

    if dx.abs() > 0.5 {
        me.dir = if dx > 0.0 { 1 } else { -1 };
    }
    me.vx = me.dir as f32 * DIVE_SPEED;

    let (x, y) = (me.x.round() as i32, me.y.round() as i32);

    // Aim vertically at the prey, but pull up if terrain is close below.
    me.vy = dy.clamp(-DIVE_SPEED, DIVE_SPEED);
    if (1..=2).any(|d| obstacle(sim, x, y + d)) {
        me.vy = -DIVE_SPEED;
    }

    // Don't plough through a wall: revert to ordinary flight if blocked ahead
    // (leaves don't count — it dives clean through the canopy after prey).
    let nx = me.x + me.vx;
    let ahead = nx.round() as i32;
    if nx < 1.0 || nx >= (sim.width - 1) as f32 || obstacle(sim, ahead, y) {
        fly(sim, me);
        return;
    }
    me.x = nx;
    me.y = (me.y + me.vy).clamp(1.0, (sim.height - 2) as f32);
}

// ───────────────────────────── swimming ──────────────────────────────
//
// A swimmer's world is the water: it must stay submerged to breathe, so its
// motion ([`paddle`]) keeps it under the surface and off the bottom and turns it
// back at the bank. The forager [`swim`] folds in the usual hunger/eat/breed, and
// adds the one trick a fish has that a walker or flier doesn't — when it senses
// prey up out of the water (an ant on the bank) it lines up beneath it and
// [leaps](pursue) clear of the surface to grab it.

/// Cruising speed of a fish, in cells/tick.
const SWIM_SPEED: f32 = 0.9;
/// Gravity on a fish once it's out of the water (mid-leap or beached).
const FISH_GRAVITY: f32 = 0.5;
/// Upward speed of a breaching leap — enough to clear the surface and reach an
/// ant standing on the bank just above it.
const LEAP_SPEED: f32 = 2.2;
/// Horizontal drift a fish manages while airborne (it has little purchase on air).
const AIR_DRIFT: f32 = 0.5;
/// Ticks a fish can survive out of water before it suffocates — comfortably more
/// than a leap takes, so only a properly beached fish dies of it.
const SWIM_SUFFOCATE: u16 = 70;

/// Whether a cell is water a fish can swim through — open water or the algae that
/// grows in it.
fn aquatic(sim: &Simulation, x: i32, y: i32) -> bool {
    matches!(sim.cell_mat(x, y), Some(WATER) | Some(ALGAE))
}

/// Nudge a swimmer's vertical velocity to keep it in the water: don't rise out
/// through the surface, and lift off the bottom when terrain is right below.
fn keep_submerged(sim: &Simulation, me: &mut EntityState, x: i32, y: i32) {
    if !aquatic(sim, x, y - 1) {
        me.vy = me.vy.max(0.0); // open air just above — stay under the surface
    }
    if !aquatic(sim, x, y + 1) {
        me.vy = -(SWIM_SPEED * 0.5); // bottom just below — rise off it
    }
}

/// A fish that isn't in the water: count down to suffocation while it falls back
/// toward the pool under gravity, drifting `drift_dx`-ward (toward the prey it
/// leapt for, if any). Comes to rest flopping if it lands on solid ground.
fn airborne(sim: &mut Simulation, me: &mut EntityState, drift_dx: f32) {
    me.air = me.air.saturating_add(1);
    if me.air >= SWIM_SUFFOCATE {
        me.alive = false;
        return;
    }
    me.timer = me.timer.wrapping_add(1);
    me.vy = (me.vy + FISH_GRAVITY).min(2.5);

    let (x, y) = (me.x.round() as i32, me.y.round() as i32);
    if drift_dx.abs() > 0.5 {
        me.dir = if drift_dx > 0.0 { 1 } else { -1 };
    }
    let nx = me.x + me.dir as f32 * AIR_DRIFT;
    let ahead = nx.round() as i32;
    if nx > 1.0 && nx < (sim.width - 1) as f32 && !obstacle(sim, ahead, y) {
        me.x = nx;
    }
    // Sink unless solid ground catches the flop (soft leaves don't — it drops
    // back through the canopy toward the pool).
    let ny = me.y + me.vy;
    if !obstacle(sim, x, ny.round() as i32) {
        me.y = ny.clamp(1.0, (sim.height - 2) as f32);
    } else {
        me.vy = 0.0;
    }
}

/// Swim about within the water: cruise in the facing direction, wander gently up
/// and down, hold below the surface and above the bottom, and turn back at the
/// bank. Counts toward suffocation (and falls back toward the pool) whenever it
/// finds itself out of water. Shared by fish and any future swimmer.
pub fn paddle(sim: &mut Simulation, me: &mut EntityState) {
    let (x, y) = (me.x.round() as i32, me.y.round() as i32);
    if !aquatic(sim, x, y) {
        airborne(sim, me, 0.0);
        return;
    }
    me.air = 0;
    me.timer = me.timer.wrapping_add(1);
    if me.vx == 0.0 {
        me.vx = me.dir as f32 * SWIM_SPEED;
    }
    if sim.chance(12) {
        me.vy += sim.rand_f32() - 0.5;
    }
    me.vy = (me.vy * 0.85).clamp(-SWIM_SPEED, SWIM_SPEED);
    keep_submerged(sim, me, x, y);

    let nx = me.x + me.vx;
    let ahead = nx.round() as i32;
    if nx < 1.0 || nx >= (sim.width - 1) as f32 || !aquatic(sim, ahead, y) {
        me.vx = -me.vx;
        me.dir = -me.dir;
    } else {
        me.x = nx;
    }
    me.y = (me.y + me.vy).clamp(1.0, (sim.height - 2) as f32);
}

/// A swimming forager: paddle about until hunger bites, then feed — eat any food
/// within reach (and maybe breed), else make for what it senses. Algae and prey
/// down in the water it simply swims to; an ant up on the bank it stalks to the
/// surface and leaps at. Suffocates if stranded out of water, starves if unfed.
/// Shared by fish and any future swimmer.
pub fn swim(sim: &mut Simulation, me: &mut EntityState, diet: &Diet) {
    if burned(sim, me) || hunger_tick(me) {
        return;
    }
    if me.hunger >= HUNGRY {
        if eat(sim, me, diet) {
            breed(sim, me);
            return;
        }
        if let Some((dx, dy)) = search(sim, me, diet) {
            pursue(sim, me, dx, dy);
            return;
        }
    }
    paddle(sim, me);
}

/// Make for food at offset `(dx, dy)`: swim straight to it when it's in the water,
/// but when it's an ant up above the surface, rise to just under it and — once
/// lined up — breach in a leap to snatch it (gravity, via [`airborne`], splashing
/// the fish back down after). A focused counterpart to [`paddle`] used mid-chase.
fn pursue(sim: &mut Simulation, me: &mut EntityState, dx: f32, dy: f32) {
    let (x, y) = (me.x.round() as i32, me.y.round() as i32);
    // Already out of the water — mid-leap (or chasing across the surface): coast
    // under gravity, steering toward the prey.
    if !aquatic(sim, x, y) {
        airborne(sim, me, dx);
        return;
    }
    me.air = 0;
    me.timer = me.timer.wrapping_add(1);
    if dx.abs() > 0.5 {
        me.dir = if dx > 0.0 { 1 } else { -1 };
    }

    let prey_above = dy < -0.5;
    let lined_up = dx.abs() <= 1.5;
    let at_surface = !aquatic(sim, x, y - 1);

    // Lined up just under the surface with prey above: breach!
    if prey_above && lined_up && at_surface {
        me.vx = 0.0;
        me.vy = -LEAP_SPEED;
        me.y += me.vy; // commit the leap clear of the water this tick
        return;
    }

    // Otherwise swim toward the food, rising toward the surface when the prey is
    // above so the next breach launches from just beneath it.
    me.vx = me.dir as f32 * SWIM_SPEED;
    me.vy = if prey_above {
        -(SWIM_SPEED * 0.6)
    } else {
        dy.clamp(-SWIM_SPEED, SWIM_SPEED)
    };
    keep_submerged(sim, me, x, y);

    // Follow only through water; bounce at the bank rather than beaching itself.
    let nx = me.x + me.vx;
    let ahead = nx.round() as i32;
    if nx > 1.0 && nx < (sim.width - 1) as f32 && aquatic(sim, ahead, y) {
        me.x = nx;
    } else {
        me.vx = -me.vx;
        me.dir = -me.dir;
    }
    me.y = (me.y + me.vy).clamp(1.0, (sim.height - 2) as f32);
}
