//! Shared movement logic that materials reuse.
//!
//! A material's `update` method should be tiny — it just delegates to one of
//! these functions. That way every powder shares a single, well-tested
//! falling/piling implementation, and a new material gets that behaviour "for
//! free" by calling the same helper.
//!
//! These operate on the grid through `Simulation`'s `pub(crate)` helpers
//! (`try_move`, `rand_bool`, …) and never touch material-specific data, so any
//! material can call any of them.

use crate::materials::{MaterialId, EMPTY, FIRE, LAVA};
use crate::sim::Simulation;

/// Catch fire from an adjacent flame or lava, turning this cell into [`FIRE`].
///
/// A combustible solid (wood, leaves) calls this before its normal "do nothing"
/// motion. The ignition is stochastic — with probability `1/rarity` per tick
/// while a flame or lava touches it — so a tree burns down as a creeping front
/// rather than vanishing in a single tick. A smaller `rarity` catches faster
/// (leaves), a larger one resists longer (wood). Returns `true` if it ignited.
pub fn flammable(sim: &mut Simulation, x: usize, y: usize, rarity: u32) -> bool {
    let touched_by_heat =
        sim.neighbor(x, y, FIRE).is_some() || sim.neighbor(x, y, LAVA).is_some();
    if touched_by_heat && sim.chance(rarity) {
        sim.set(x, y, FIRE);
        true
    } else {
        false
    }
}

/// React on contact: if any neighbour of `(x,y)` is `trigger`, turn both this
/// cell and that neighbour into `product` and report that a reaction happened.
///
/// This is the shared "is something touching me?" primitive — a material's
/// `update` calls it before its normal motion, and any pair of materials can
/// reuse it (water+lava→stone today; acid+metal, water+fire, … tomorrow) just
/// by passing different ids. Returns `true` if it reacted, so the caller can
/// skip moving a cell that no longer exists.
pub fn react_on_contact(
    sim: &mut Simulation,
    x: usize,
    y: usize,
    trigger: MaterialId,
    product: MaterialId,
) -> bool {
    if let Some((nx, ny)) = sim.neighbor(x, y, trigger) {
        sim.set(x, y, product);
        sim.set(nx, ny, product);
        true
    } else {
        false
    }
}

/// One-sided reaction: if any neighbour of `(x,y)` is `trigger`, turn *only*
/// this cell into `product` (the neighbour is left untouched) and report it.
///
/// Where [`react_on_contact`] consumes both cells, here the trigger is a
/// catalyst that survives — exactly what a spreading effect wants: oil next to
/// fire or lava ignites, but the flame or lava that lit it stays put and can go
/// on to light the next cell. Returns `true` if it transformed.
pub fn transform_on_contact(
    sim: &mut Simulation,
    x: usize,
    y: usize,
    trigger: MaterialId,
    product: MaterialId,
) -> bool {
    if sim.neighbor(x, y, trigger).is_some() {
        sim.set(x, y, product);
        true
    } else {
        false
    }
}

/// Occasionally shed a `product` particle into an adjacent empty cell — a
/// source that gives something off (lava spitting fire, water steaming). With
/// probability `1/rarity` per tick it fills the first empty orthogonal
/// neighbour (favouring the one above, since most emissions rise) and returns
/// `true`. Does nothing — and returns `false` — when it doesn't fire or when the
/// cell is fully boxed in.
pub fn emit(sim: &mut Simulation, x: usize, y: usize, product: MaterialId, rarity: u32) -> bool {
    if !sim.chance(rarity) {
        return false;
    }
    if let Some((nx, ny)) = sim.neighbor(x, y, EMPTY) {
        sim.set(nx, ny, product);
        true
    } else {
        false
    }
}

/// Immovable: never moves. Used by stone, walls, bedrock.
pub fn solid(_sim: &mut Simulation, _x: usize, _y: usize) {}

/// Powder: fall straight down; if blocked, tumble diagonally so the material
/// settles into a pile at its angle of repose. Used by sand, and any future
/// dirt/ash/salt/gunpowder.
pub fn powder(sim: &mut Simulation, x: usize, y: usize) {
    // Resting on the floor.
    if y + 1 >= sim.height {
        return;
    }
    // Straight down.
    if sim.try_move(x, y, x, y + 1) {
        return;
    }
    // Down-diagonal. Randomise which side we try first to avoid a drift bias.
    let (first, second): (i32, i32) = if sim.rand_bool() { (-1, 1) } else { (1, -1) };
    for dx in [first, second] {
        let nx = x as i32 + dx;
        if nx >= 0 && (nx as usize) < sim.width && sim.try_move(x, y, nx as usize, y + 1) {
            return;
        }
    }
}

/// Liquid: fall straight down, tumble diagonally like a powder, then spread
/// sideways to seek its own level. Shared by every liquid (water, lava, …); the
/// only thing that differs per-liquid is `speed`.
///
/// `speed` is how many cells the liquid may flow horizontally in a single tick
/// — its viscosity knob. A runny liquid like water uses a high value so it fans
/// out and levels off almost instantly; a sluggish one like lava uses a low
/// value so it barely creeps sideways and piles into blobs.
pub fn liquid(sim: &mut Simulation, x: usize, y: usize, speed: usize) {
    // Straight down (unless resting on the floor).
    if y + 1 < sim.height {
        if sim.try_move(x, y, x, y + 1) {
            return;
        }
        // Down-diagonal. Randomise which side we try first to avoid drift bias.
        let (first, second): (i32, i32) = if sim.rand_bool() { (-1, 1) } else { (1, -1) };
        for dx in [first, second] {
            let nx = x as i32 + dx;
            if nx >= 0 && (nx as usize) < sim.width && sim.try_move(x, y, nx as usize, y + 1) {
                return;
            }
        }
    }
    // Can't fall: flow sideways up to `speed` cells in one randomly-chosen
    // direction, stopping at the first cell it can't enter.
    let dir: i32 = if sim.rand_bool() { -1 } else { 1 };
    let mut cx = x;
    for _ in 0..speed {
        let nx = cx as i32 + dir;
        if nx < 0 || nx as usize >= sim.width || !sim.try_move(cx, y, nx as usize, y) {
            break;
        }
        cx = nx as usize;
    }
}

/// Gas: the inverse of [`liquid`] — rises, tumbles up-diagonally when blocked,
/// then drifts sideways along a ceiling. Shared by fire, smoke, steam, …; as
/// with `liquid`, `speed` is the horizontal flow rate (how far it may drift in
/// one tick). Movement still goes through `try_move`, so a light gas only rises
/// through cells it can displace (empty air, by default).
pub fn gas(sim: &mut Simulation, x: usize, y: usize, speed: usize) {
    // Straight up (unless against the ceiling).
    if y > 0 {
        if sim.try_move(x, y, x, y - 1) {
            return;
        }
        // Up-diagonal. Randomise which side we try first to avoid drift bias.
        let (first, second): (i32, i32) = if sim.rand_bool() { (-1, 1) } else { (1, -1) };
        for dx in [first, second] {
            let nx = x as i32 + dx;
            if nx >= 0 && (nx as usize) < sim.width && sim.try_move(x, y, nx as usize, y - 1) {
                return;
            }
        }
    }
    // Can't rise: drift sideways up to `speed` cells in one random direction.
    let dir: i32 = if sim.rand_bool() { -1 } else { 1 };
    let mut cx = x;
    for _ in 0..speed {
        let nx = cx as i32 + dir;
        if nx < 0 || nx as usize >= sim.width || !sim.try_move(cx, y, nx as usize, y) {
            break;
        }
        cx = nx as usize;
    }
}
