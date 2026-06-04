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

use crate::materials::MaterialId;
use crate::sim::Simulation;

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

// Future shared behaviours slot in here and get called from a material's
// `update`, e.g.:
//
// /// Gas: the inverse of powder — rise and spread.
// pub fn gas(sim: &mut Simulation, x: usize, y: usize) { ... }
