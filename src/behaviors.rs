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
use crate::sim::{Simulation, VEL_UNIT};

/// How quickly a wind-borne cell's velocity chases the wind: it closes `1/this`
/// of the gap each tick. A low-pass filter — big enough that a gust takes hold
/// within a few ticks, small enough that the cell keeps coasting for a moment
/// after the gust drops (and, when the wind is calm, bleeds its momentum back to
/// zero, so no separate friction term is needed).
const WIND_RESPONSE: i32 = 3;

/// Catch fire from an adjacent flame or lava, turning this cell into [`FIRE`].
///
/// A combustible solid (wood, leaves) calls this before its normal "do nothing"
/// motion. The ignition is stochastic — with probability `1/rarity` per tick
/// while a flame or lava touches it — so a tree burns down as a creeping front
/// rather than vanishing in a single tick. A smaller `rarity` catches faster
/// (leaves), a larger one resists longer (wood). Returns `true` if it ignited.
pub fn flammable(sim: &mut Simulation, x: usize, y: usize, rarity: u32) -> bool {
    let touched_by_heat = sim.neighbor(x, y, FIRE).is_some() || sim.neighbor(x, y, LAVA).is_some();
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

/// Carry a wind-borne cell along the wind for one tick, then report where it
/// ended up. This is the shared "ride the wind" motion: fire, clouds, and rain
/// call it before their own characteristic motion (rising, bobbing, falling), so
/// a gust leans, drifts, and slants them. A material opts into wind simply by
/// calling this — nothing else does, so dense things like sand stay put.
///
/// The cell's stored velocity eases toward the local wind ([`Simulation::wind_at`])
/// rather than snapping to it, which gives the motion inertia: it ramps up as a
/// gust arrives and coasts on after it passes. The whole-cell part of that
/// velocity (plus a fractional chance of one more cell — see
/// [`Simulation::rand_ratio`]) is then walked out one step at a time, first
/// horizontally then vertically. Each hop goes through [`Simulation::try_move`],
/// so the cell only enters space it may, and on hitting something it sheds the
/// blocked component of its momentum (a soft collision).
///
/// `escape` decides what the world edge does. A cloud blown off the side should
/// drift away for good (`true` → the cell is cleared and `None` returned); rain
/// or a flame should just pile against the wall (`false`). Returns the cell's new
/// position, or `None` if it left the world — in which case the caller must stop
/// touching the cell, as it no longer exists.
pub fn drift(sim: &mut Simulation, x: usize, y: usize, escape: bool) -> Option<(usize, usize)> {
    let (wx, wy) = sim.wind_at(x, y);
    let (vx0, vy0) = sim.velocity(x, y);
    // Ease toward the wind (this also decays momentum to zero when it's calm).
    let vx = vx0 + (wx - vx0) / WIND_RESPONSE;
    let vy = vy0 + (wy - vy0) / WIND_RESPONSE;
    sim.set_velocity(x, y, vx, vy);

    let mut cx = x;
    let mut cy = y;

    // Horizontal transport.
    let hdir = vx.signum();
    for _ in 0..steps_from_velocity(sim, vx) {
        let nx = cx as i32 + hdir;
        if nx < 0 || nx as usize >= sim.width {
            if escape {
                sim.set(cx, cy, EMPTY);
                return None;
            }
            sim.set_velocity(cx, cy, 0, vy); // hit the wall: lose sideways momentum
            break;
        }
        if !sim.try_move(cx, cy, nx as usize, cy) {
            sim.set_velocity(cx, cy, 0, vy); // blocked: shed horizontal momentum
            break;
        }
        cx = nx as usize;
    }

    // Vertical transport, from wherever the horizontal step left the cell.
    let vdir = vy.signum();
    for _ in 0..steps_from_velocity(sim, vy) {
        let ny = cy as i32 + vdir;
        let blocked = ny < 0 || ny as usize >= sim.height || !sim.try_move(cx, cy, cx, ny as usize);
        if blocked {
            let (cvx, _) = sim.velocity(cx, cy);
            sim.set_velocity(cx, cy, cvx, 0); // shed vertical momentum
            break;
        }
        cy = ny as usize;
    }

    Some((cx, cy))
}

/// How many whole cells a velocity component carries the cell this tick: its
/// integer cells-per-tick, plus a `1/VEL_UNIT`-weighted chance of one extra so
/// sub-cell speeds aren't simply rounded away. Always non-negative — direction
/// is the caller's `signum`.
fn steps_from_velocity(sim: &mut Simulation, v: i32) -> i32 {
    let mag = v.abs();
    let frac = (mag % VEL_UNIT) as u32;
    mag / VEL_UNIT + sim.rand_ratio(frac, VEL_UNIT as u32) as i32
}

/// Fly a heavy projectile (a meteor) one tick along its own velocity, under
/// gravity and *ignoring* the wind. Where [`drift`] eases toward the breeze and
/// bleeds its momentum, a projectile keeps every bit of its speed and passes
/// only through open air: the instant its path meets anything solid — or the
/// floor, or a side wall — it stops and reports the cell it struck *from*, so the
/// caller can detonate it. `gravity` is added to the downward velocity each tick,
/// bending an aimed shot into a falling arc.
///
/// The whole-cell velocity (plus the usual fractional chance of one more) is
/// walked out a cell at a time, interleaving the two axes with a small DDA so the
/// flight traces a straight diagonal rather than an L-shaped kink. Returns
/// `Ok(new_pos)` while still in flight, or `Err(impact_pos)` at the cell it hit
/// from — where the projectile still sits, ready to be replaced by the blast.
pub fn ballistic(
    sim: &mut Simulation,
    x: usize,
    y: usize,
    gravity: i32,
) -> Result<(usize, usize), (usize, usize)> {
    let (vx, vy0) = sim.velocity(x, y);
    // Accelerate downward, saturating into the `i8` the cell stores velocity in.
    let vy = (vy0 + gravity).min(i8::MAX as i32);
    sim.set_velocity(x, y, vx, vy);

    let hsteps = steps_from_velocity(sim, vx);
    let vsteps = steps_from_velocity(sim, vy);
    let hdir = vx.signum();
    let vdir = vy.signum();
    let n = hsteps.max(vsteps);

    let mut cx = x as i32;
    let mut cy = y as i32;
    // DDA: spread the smaller axis's steps evenly across the larger one.
    let (mut ax, mut ay) = (0, 0);
    for _ in 0..n {
        ax += hsteps;
        ay += vsteps;
        let mut nx = cx;
        let mut ny = cy;
        if ax >= n {
            ax -= n;
            nx += hdir;
        }
        if ay >= n {
            ay -= n;
            ny += vdir;
        }
        // Off a side wall or through the floor: detonate where we stand.
        if nx < 0 || nx >= sim.width as i32 || ny >= sim.height as i32 {
            return Err((cx as usize, cy as usize));
        }
        // Out the top (an upward shot that never came down): stop, still alive.
        if ny < 0 {
            return Ok((cx as usize, cy as usize));
        }
        // Advance one cell per axis, exploding the moment a step would enter
        // anything that isn't open air (so it bursts on the ground rather than
        // ploughing through it the way its great density otherwise would).
        if nx != cx {
            if sim.mat_at(nx as usize, cy as usize) != EMPTY {
                return Err((cx as usize, cy as usize));
            }
            sim.try_move(cx as usize, cy as usize, nx as usize, cy as usize);
            cx = nx;
        }
        if ny != cy {
            if sim.mat_at(cx as usize, ny as usize) != EMPTY {
                return Err((cx as usize, cy as usize));
            }
            sim.try_move(cx as usize, cy as usize, cx as usize, ny as usize);
            cy = ny;
        }
    }
    Ok((cx as usize, cy as usize))
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
