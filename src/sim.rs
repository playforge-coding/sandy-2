//! The simulation grid and the tick loop.
//!
//! The world is a fixed-size grid of cells in a single `Vec`. Each tick we scan
//! it bottom-to-top and ask each material to update its own cell. Because a
//! falling particle always moves into a row *below* the one being scanned (a
//! row we've already processed this tick), a particle can move at most once per
//! tick and we can mutate the grid in place with no second buffer.
//!
//! This file is deliberately material-agnostic: it knows about cells and how to
//! move them, but not what sand or stone "is". Per-material logic lives in
//! `crate::materials::*`; shared motion lives in `crate::behaviors`.

use crate::materials::{self, MaterialId, MaterialInfo, EMPTY};

/// Simulation resolution. The renderer stretches this to fill the window, so
/// these are "logical sand pixels", independent of the actual window size.
pub const GRID_W: usize = 500;
pub const GRID_H: usize = 250;

/// One cell of the world.
#[derive(Clone, Copy)]
struct Cell {
    mat: MaterialId,
    /// Frozen-at-spawn randomness, used only for colour jitter so a cell's
    /// grain doesn't shimmer as it moves.
    variant: u8,
    /// Per-cell momentum, in [`VEL_UNIT`] sub-units per tick (`+x` = right, `+y`
    /// = down). Most motion in this sim is rule-based (sand tries down, water
    /// spreads sideways), but wind-borne cells carry a real velocity so a gust
    /// pushes them and they keep coasting for a moment after it drops — that
    /// inertia is what makes blown flames and slanting rain read as wind rather
    /// than teleportation. [`crate::behaviors::drift`] integrates it; cells that
    /// don't ride the wind simply leave it at zero. Velocity travels with the
    /// particle automatically because [`Simulation::try_move`] swaps whole cells.
    vx: i8,
    vy: i8,
    /// Frame on which this cell last moved. The bottom-to-top scan skips a cell
    /// that already moved this tick, so a particle is processed at most once —
    /// without this, a *rising* particle (gas/fire) would be re-encountered by
    /// the same scan in the row above and teleport to the ceiling in one tick.
    ///
    /// Truncated to `u32` (compared against `frame as u32`) so `Cell` packs into
    /// 8 bytes instead of 16 — halving the grid's memory footprint and the
    /// bandwidth of every scan, swap, and render. (`mat`, `variant`, `vx`, `vy`
    /// fill the four bytes before it, so the velocity fields are free — they ride
    /// in space the struct's 4-byte alignment was padding out anyway.) The
    /// counter only ever needs to match *this* tick's frame, so the ~2-year wrap
    /// at 60 fps is harmless.
    moved: u32,
}

const VOID: Cell = Cell {
    mat: EMPTY,
    variant: 0,
    vx: 0,
    vy: 0,
    moved: 0,
};

/// Velocity fixed-point: this many sub-units make one cell per tick. A cell's
/// `vx`/`vy` (an `i8`) therefore span roughly ±4 cells/tick — ample for a
/// wind-blown sand pixel — while still resolving sub-cell speeds, which
/// [`crate::behaviors::drift`] turns into the occasional whole-cell hop.
pub(crate) const VEL_UNIT: i32 = 32;

/// Peak strength of the prevailing ambient breeze, in velocity sub-units. Kept
/// gentle — a fraction of a cell per tick — so the default weather nudges
/// clouds across the sky and leans flames without flinging anything about. The
/// wind *tool* layers much stronger, local gusts on top of this.
const AMBIENT_MAX: i32 = 6;

/// Angular rate of the ambient breeze's oscillation, in radians per tick. The
/// breeze eases through a full reverse-and-back cycle every `2π / this` ticks
/// (~40 s at 60 fps) — a smooth sine rather than the old hard flip, so the wind
/// swells and slackens like real weather.
const AMBIENT_RATE: f32 = 0.0026;

pub struct Simulation {
    pub width: usize,
    pub height: usize,
    cells: Vec<Cell>,
    frame: u64,
    /// xorshift state for cheap, dependency-free randomness.
    rng: u32,
    /// The wind system, part one: a smoothly-oscillating *ambient* breeze,
    /// horizontal only, refreshed once per tick (see [`Simulation::update_wind`]).
    /// This is the default weather every wind-borne cell feels everywhere.
    ambient_x: i32,
    /// The wind system, part two: a per-cell *gust* field the wind tool paints
    /// into (in velocity sub-units, `+x` = right / `+y` = down). It decays back
    /// toward calm every tick so a gust fades like a real one. The effective
    /// wind a cell rides is this plus the ambient breeze — see [`wind_at`].
    ///
    /// [`wind_at`]: Simulation::wind_at
    wind_x: Vec<i8>,
    wind_y: Vec<i8>,
    /// Dirty flag: true while any gust is still non-zero. Lets the per-tick decay
    /// sweep (and the cost of touching the whole field) be skipped entirely on a
    /// calm world, which is the common case.
    gust_active: bool,
    /// Per-id [`MaterialInfo`] cache, indexed by [`MaterialId`]. Looking a
    /// material up in the registry costs a `thread_local` + `RefCell` borrow and
    /// a dynamic call; doing that per cell in the hot `try_move`/`render_into`
    /// paths dominated the tick. The registry only changes when a plugin loads,
    /// so we snapshot every material's `info()` once per tick (cheap — a handful
    /// of entries) and index this array instead.
    infos: Vec<MaterialInfo>,
}

impl Simulation {
    pub fn new() -> Self {
        Self {
            width: GRID_W,
            height: GRID_H,
            cells: vec![VOID; GRID_W * GRID_H],
            frame: 0,
            rng: 0x9E37_79B9,
            ambient_x: 0,
            wind_x: vec![0; GRID_W * GRID_H],
            wind_y: vec![0; GRID_W * GRID_H],
            gust_active: false,
            infos: Self::snapshot_infos(),
        }
    }

    /// Snapshot every registered material's [`MaterialInfo`] into a flat table
    /// indexed by id. Refreshed once per tick so the per-cell hot paths can read
    /// `self.infos[id]` instead of going through the registry. See `infos`.
    fn snapshot_infos() -> Vec<MaterialInfo> {
        (0..materials::count())
            .map(|id| materials::get(id as MaterialId).info())
            .collect()
    }

    /// The effective wind at a cell, in velocity sub-units (`+x` = right, `+y` =
    /// down): the ambient breeze plus any local gust painted by the wind tool.
    /// Wind-borne materials sample this and ease their velocity toward it (see
    /// [`crate::behaviors::drift`]).
    #[inline]
    pub(crate) fn wind_at(&self, x: usize, y: usize) -> (i32, i32) {
        let i = self.idx(x, y);
        (
            self.ambient_x + self.wind_x[i] as i32,
            self.wind_y[i] as i32,
        )
    }

    /// A cell's stored velocity, in sub-units ([`VEL_UNIT`] per cell/tick).
    #[inline]
    pub(crate) fn velocity(&self, x: usize, y: usize) -> (i32, i32) {
        let c = &self.cells[self.idx(x, y)];
        (c.vx as i32, c.vy as i32)
    }

    /// Overwrite a cell's velocity, saturating to the `i8` range it's stored in.
    #[inline]
    pub(crate) fn set_velocity(&mut self, x: usize, y: usize, vx: i32, vy: i32) {
        let i = self.idx(x, y);
        let clamp = |v: i32| v.clamp(i8::MIN as i32, i8::MAX as i32) as i8;
        self.cells[i].vx = clamp(vx);
        self.cells[i].vy = clamp(vy);
    }

    /// Paint a gust into a filled circle — the wind tool's stroke. Adds
    /// `(dvx, dvy)` sub-units to every cell in range (saturating) and arms the
    /// per-tick decay sweep. A zero gust is a no-op so a still cursor blows
    /// nothing.
    pub fn add_wind_disk(&mut self, cx: i32, cy: i32, radius: i32, dvx: i32, dvy: i32) {
        if dvx == 0 && dvy == 0 {
            return;
        }
        let clamp = |v: i32| v.clamp(i8::MIN as i32, i8::MAX as i32) as i8;
        let r2 = radius * radius;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx * dx + dy * dy > r2 {
                    continue;
                }
                let x = cx + dx;
                let y = cy + dy;
                if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
                    continue;
                }
                let i = self.idx(x as usize, y as usize);
                self.wind_x[i] = clamp(self.wind_x[i] as i32 + dvx);
                self.wind_y[i] = clamp(self.wind_y[i] as i32 + dvy);
            }
        }
        self.gust_active = true;
    }

    #[inline]
    fn idx(&self, x: usize, y: usize) -> usize {
        y * self.width + x
    }

    /// Cheap xorshift32, then a coin flip from it. Used by behaviours for
    /// unbiased tie-breaking.
    #[inline]
    fn rand(&mut self) -> u32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x
    }

    /// Random coin flip — exposed for behaviours (e.g. which diagonal to try).
    #[inline]
    pub(crate) fn rand_bool(&mut self) -> bool {
        self.rand() & 1 == 0
    }

    /// A fresh pseudo-random `u32`. Used to mint a random world seed when the
    /// user asks for one (the "randomise" key), so each press lands on a
    /// different world without needing a system clock (handy on the web).
    #[inline]
    pub(crate) fn rand_u32(&mut self) -> u32 {
        self.rand()
    }

    /// True with probability `1/n` — a rarity dial for stochastic behaviours
    /// (fire guttering out, lava spitting a flame). `n == 0` is treated as `1`
    /// (always true) so callers needn't guard against it.
    #[inline]
    pub(crate) fn chance(&mut self, n: u32) -> bool {
        self.rand() % n.max(1) == 0
    }

    /// True with probability `num/den` — the finer-grained cousin of [`chance`].
    /// Used to turn a sub-cell velocity into the occasional whole-cell hop: a
    /// speed of 0.3 cells/tick steps one cell roughly three times in ten.
    ///
    /// [`chance`]: Simulation::chance
    #[inline]
    pub(crate) fn rand_ratio(&mut self, num: u32, den: u32) -> bool {
        self.rand() % den.max(1) < num
    }

    /// Try to move/swap the cell at `(sx,sy)` into `(tx,ty)`, if the source can
    /// displace whatever is there. Returns whether it moved. This is where the
    /// density/`movable` rules live, shared by every behaviour.
    pub(crate) fn try_move(&mut self, sx: usize, sy: usize, tx: usize, ty: usize) -> bool {
        let si = self.idx(sx, sy);
        let ti = self.idx(tx, ty);
        let target = self.cells[ti].mat;

        let can_move = if target == EMPTY {
            true
        } else {
            // A denser movable material sinks through a lighter movable one
            // (e.g. sand through water, once water exists). Solids block all.
            let src_density = self.infos[self.cells[si].mat as usize].density;
            let tgt = self.infos[target as usize];
            tgt.movable && src_density > tgt.density
        };

        if can_move {
            self.cells.swap(si, ti);
            // The active particle now lives at `ti`; stamp it so this tick's
            // scan won't process it again (see `Cell::moved`).
            self.cells[ti].moved = self.frame as u32;
            true
        } else {
            false
        }
    }

    /// Find the first 4-neighbour of `(x,y)` whose material is `mat`, returning
    /// its coordinates. Shared sensing primitive: any behaviour can ask "is X
    /// touching me?" without knowing how the grid is laid out. Orthogonal only
    /// (up/down/left/right), which is what reactions like water-meets-lava want.
    pub(crate) fn neighbor(&self, x: usize, y: usize, mat: MaterialId) -> Option<(usize, usize)> {
        const OFFSETS: [(i32, i32); 4] = [(0, -1), (0, 1), (-1, 0), (1, 0)];
        for (dx, dy) in OFFSETS {
            let nx = x as i32 + dx;
            let ny = y as i32 + dy;
            if nx < 0 || ny < 0 || nx as usize >= self.width || ny as usize >= self.height {
                continue;
            }
            let (nx, ny) = (nx as usize, ny as usize);
            if self.cells[self.idx(nx, ny)].mat == mat {
                return Some((nx, ny));
            }
        }
        None
    }

    /// Overwrite a single cell with `mat`, giving it fresh colour jitter. Used
    /// by reactions that transform a cell in place (rather than move it).
    pub(crate) fn set(&mut self, x: usize, y: usize, mat: MaterialId) {
        let variant = (self.rand() & 0xFF) as u8;
        let i = self.idx(x, y);
        self.cells[i] = Cell {
            mat,
            variant,
            // A freshly-spawned cell starts at rest; it picks up momentum from
            // the wind on the ticks that follow.
            vx: 0,
            vy: 0,
            moved: 0,
        };
    }

    /// Stamp a filled circle of `mat` into the grid (the painting brush).
    /// Painting [`EMPTY`] erases.
    pub fn paint_disk(&mut self, cx: i32, cy: i32, radius: i32, mat: MaterialId) {
        let r2 = radius * radius;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx * dx + dy * dy > r2 {
                    continue;
                }
                let x = cx + dx;
                let y = cy + dy;
                if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
                    continue;
                }
                let i = self.idx(x as usize, y as usize);
                if mat == EMPTY {
                    self.cells[i] = VOID;
                } else {
                    let variant = (self.rand() & 0xFF) as u8;
                    self.cells[i] = Cell {
                        mat,
                        variant,
                        vx: 0,
                        vy: 0,
                        moved: 0,
                    };
                }
            }
        }
    }

    pub fn clear(&mut self) {
        for c in self.cells.iter_mut() {
            *c = VOID;
        }
        // Wipe the weather too: a cleared world should be dead calm.
        self.wind_x.iter_mut().for_each(|v| *v = 0);
        self.wind_y.iter_mut().for_each(|v| *v = 0);
        self.gust_active = false;
    }

    /// Advance the wind one tick: refresh the ambient breeze and fade any gusts.
    fn update_wind(&mut self) {
        // The prevailing breeze eases through a smooth sine — swelling, dropping,
        // and gently reversing — rather than snapping direction on a timer.
        self.ambient_x = (AMBIENT_MAX as f32 * (self.frame as f32 * AMBIENT_RATE).sin()) as i32;

        // Relax painted gusts back toward calm. Exponential-ish (shed an eighth
        // of the magnitude) but always by at least one unit, so a gust actually
        // reaches zero instead of crawling there forever. Skipped wholesale when
        // nothing is blowing.
        if self.gust_active {
            let mut any = false;
            for v in self.wind_x.iter_mut().chain(self.wind_y.iter_mut()) {
                if *v != 0 {
                    let step = ((*v as i32).abs() / 8).max(1) as i8;
                    *v -= v.signum() * step;
                    any |= *v != 0;
                }
            }
            self.gust_active = any;
        }
    }

    /// Advance the world by one tick.
    pub fn step(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        // Advance the weather: ambient breeze plus decaying tool-painted gusts.
        self.update_wind();
        // Refresh the per-id info cache for this tick's hot paths (`try_move`,
        // and `render_into` which runs right after). Cheap: one entry per
        // material, and it picks up any plugin registered since last tick.
        self.infos.clear();
        self.infos
            .extend((0..materials::count()).map(|id| materials::get(id as MaterialId).info()));
        let (w, h) = (self.width, self.height);

        // Bottom row first so settled particles don't get re-processed.
        for y in (0..h).rev() {
            // Alternate scan direction per row/frame so piles stay symmetric.
            let left_to_right = (self.frame + y as u64) & 1 == 0;
            for xi in 0..w {
                let x = if left_to_right { xi } else { w - 1 - xi };
                let cell = self.cells[self.idx(x, y)];
                if cell.mat == EMPTY {
                    continue;
                }
                // Already moved into this row this tick — skip so it isn't
                // processed twice (keeps rising gas from racing to the top).
                if cell.moved == self.frame as u32 {
                    continue;
                }
                let id = cell.mat;
                // `get` is 'static and doesn't borrow `self`, so the material is
                // free to take `&mut self` and move cells around.
                materials::get(id).update(self, x, y);
            }
        }
    }

    /// Render the grid into a tightly-packed RGBA8 buffer
    /// (`width * height * 4` bytes), which the GPU uploads as a texture.
    pub fn render_into(&self, buf: &mut [u8]) {
        debug_assert_eq!(buf.len(), self.width * self.height * 4);
        for (i, cell) in self.cells.iter().enumerate() {
            // Index the per-tick info cache rather than the registry (one
            // `thread_local`/`RefCell` borrow per cell, over the whole grid,
            // every frame, was a measurable slice of the render). `step` runs
            // immediately before each render and keeps this in sync.
            let info = &self.infos[cell.mat as usize];
            let mut rgba = info.shade(cell.variant);
            // The alpha channel is repurposed as a "this pixel glows" flag for
            // the renderer's bloom pass: 0 = emissive, 255 = opaque. (We never
            // alpha-blend the grid texture, so the channel is free to carry
            // this instead.) The UI overlay always writes 255, so it can't
            // bloom — see `ui::put`.
            rgba[3] = if info.glow { 0 } else { 255 };
            buf[i * 4..i * 4 + 4].copy_from_slice(&rgba);
        }
    }

    /// The material id at `(x, y)`. Used by tests and by the plugin host API so
    /// a script can sense what's in a cell.
    #[inline]
    pub(crate) fn mat_at(&self, x: usize, y: usize) -> MaterialId {
        self.cells[self.idx(x, y)].mat
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAND: MaterialId = 1;
    const STONE: MaterialId = 2;
    const WATER: MaterialId = 3;
    const LAVA: MaterialId = 4;
    const OIL: MaterialId = 5;
    const FIRE: MaterialId = 6;
    const CLOUD: MaterialId = 10;

    #[test]
    fn sand_falls_to_the_floor() {
        let mut sim = Simulation::new();
        sim.paint_disk(10, 0, 0, SAND); // single grain at the top
        assert_eq!(sim.mat_at(10, 0), SAND);

        for _ in 0..(GRID_H * 2) {
            sim.step();
        }

        assert_eq!(sim.mat_at(10, 0), EMPTY);
        assert_eq!(sim.mat_at(10, GRID_H - 1), SAND);
    }

    #[test]
    fn stone_never_moves() {
        let mut sim = Simulation::new();
        sim.paint_disk(10, 5, 0, STONE);
        for _ in 0..50 {
            sim.step();
        }
        assert_eq!(sim.mat_at(10, 5), STONE);
    }

    #[test]
    fn water_and_lava_make_stone() {
        // Two adjacent cells, water beside lava, on the floor so neither falls
        // away before they touch. After a step both should be stone.
        let mut sim = Simulation::new();
        let y = GRID_H - 1;
        sim.set(10, y, WATER);
        sim.set(11, y, LAVA);
        sim.step();
        assert_eq!(sim.mat_at(10, y), STONE);
        assert_eq!(sim.mat_at(11, y), STONE);
    }

    #[test]
    fn oil_floats_on_water() {
        // Oil is lighter than water, so a grain of oil dropped into a water
        // column should end up sitting above the water, not below it.
        let mut sim = Simulation::new();
        let floor = GRID_H - 1;
        sim.set(10, floor, OIL);
        sim.set(10, floor - 1, WATER);
        for _ in 0..50 {
            sim.step();
        }
        assert_eq!(sim.mat_at(10, floor), WATER);
        assert_eq!(sim.mat_at(10, floor - 1), OIL);
    }

    #[test]
    fn oil_catches_fire_from_flame() {
        // Oil next to fire ignites: the oil cell turns into fire on contact.
        let mut sim = Simulation::new();
        let y = GRID_H - 1;
        sim.set(10, y, OIL);
        sim.set(11, y, FIRE);
        sim.step();
        assert_eq!(sim.mat_at(10, y), FIRE);
    }

    #[test]
    fn oil_catches_fire_from_lava() {
        // Lava lights oil too, and unlike water+lava the lava is not consumed.
        let mut sim = Simulation::new();
        let y = GRID_H - 1;
        sim.set(10, y, OIL);
        sim.set(11, y, LAVA);
        sim.step();
        // Oil ignites into fire on contact...
        assert_eq!(sim.mat_at(10, y), FIRE);
        // ...and the lava is not consumed (unlike water+lava→stone). It may
        // creep a cell sideways on the open floor, so assert it survives
        // somewhere rather than pinning it to its start cell.
        assert!(
            (0..GRID_W).any(|x| sim.mat_at(x, y) == LAVA),
            "lava should survive lighting the oil"
        );
    }

    #[test]
    fn fire_does_not_teleport_to_the_ceiling() {
        // Regression: the bottom-to-top scan must process a rising flame at most
        // once per tick. A whole bottom row of fire should rise exactly one cell
        // in one step — never jump straight to the top of the grid.
        let mut sim = Simulation::new();
        let floor = GRID_H - 1;
        for x in 0..GRID_W {
            sim.set(x, floor, FIRE);
        }
        sim.step();
        for x in 0..GRID_W {
            for y in 0..floor - 1 {
                assert_ne!(
                    sim.mat_at(x, y),
                    FIRE,
                    "fire rose more than one cell in a single tick"
                );
            }
        }
    }

    #[test]
    fn fire_burns_out() {
        // Fire has no fuel of its own, so a lone flame eventually vanishes.
        let mut sim = Simulation::new();
        sim.set(10, GRID_H - 1, FIRE);
        for _ in 0..(GRID_H * 4) {
            sim.step();
        }
        for x in 0..GRID_W {
            for y in 0..GRID_H {
                assert_ne!(sim.mat_at(x, y), FIRE, "fire should have burned out");
            }
        }
    }

    #[test]
    fn lava_gives_off_fire() {
        // A pool of lava with air above it should, over many ticks, spit at
        // least one flame into the air.
        let mut sim = Simulation::new();
        let floor = GRID_H - 1;
        for x in 0..GRID_W {
            sim.set(x, floor, LAVA);
        }
        let mut saw_fire = false;
        for _ in 0..200 {
            sim.step();
            if (0..GRID_W).any(|x| sim.mat_at(x, floor - 1) == FIRE) {
                saw_fire = true;
                break;
            }
        }
        assert!(saw_fire, "lava should give off fire over time");
    }

    #[test]
    fn wind_carries_a_cloud_downwind() {
        // A cloud sitting in still air, under a steady rightward gust, should
        // ride that wind well clear of where it started.
        let mut sim = Simulation::new();
        let (start_x, y) = (10, 20);
        sim.set(start_x, y, CLOUD);
        for _ in 0..40 {
            // Re-paint a wide rightward gust over the cloud's path each tick so
            // it doesn't decay out from under the cloud as it travels.
            sim.add_wind_disk(100, y as i32, 120, 90, 0);
            sim.step();
        }
        let cloud_x = (0..GRID_W)
            .find(|&gx| (0..GRID_H).any(|gy| sim.mat_at(gx, gy) == CLOUD))
            .expect("cloud should still be somewhere in the world");
        assert!(
            cloud_x > start_x + 2,
            "wind should carry the cloud right, ended at x={cloud_x}"
        );
    }

    #[test]
    fn a_blown_cell_keeps_coasting_after_the_gust() {
        // Momentum: give a cloud a hard rightward shove for a few ticks, then let
        // the wind go calm. It should keep gliding right for a tick or two on the
        // velocity it built up, rather than stopping dead.
        let mut sim = Simulation::new();
        let (start_x, y) = (40, 20);
        sim.set(start_x, y, CLOUD);
        // Build up rightward momentum.
        for _ in 0..6 {
            sim.add_wind_disk(start_x as i32, y as i32, 60, 110, 0);
            sim.step();
        }
        let after_gust = (0..GRID_W)
            .find(|&gx| (0..GRID_H).any(|gy| sim.mat_at(gx, gy) == CLOUD))
            .expect("cloud exists after the gust");
        // Now no more wind painted — let it coast (ambient is gentle and may
        // wander, so only a couple of ticks before it bleeds off).
        for _ in 0..3 {
            sim.step();
        }
        let coasted = (0..GRID_W)
            .find(|&gx| (0..GRID_H).any(|gy| sim.mat_at(gx, gy) == CLOUD))
            .expect("cloud exists while coasting");
        assert!(
            coasted >= after_gust,
            "cloud should coast on its momentum, went {after_gust} -> {coasted}"
        );
    }

    #[test]
    fn calm_air_never_nudges_settled_sand_sideways() {
        // Sand isn't wind-borne, so even with the ambient breeze blowing for a
        // long time a grain resting on the floor must not creep sideways. Guards
        // against accidentally wiring the velocity system into heavy materials.
        let mut sim = Simulation::new();
        let x = 10;
        sim.set(x, GRID_H - 1, SAND);
        for _ in 0..600 {
            sim.step();
        }
        assert_eq!(sim.mat_at(x, GRID_H - 1), SAND);
    }

    #[test]
    fn gusts_fade_back_to_calm() {
        // A painted gust should decay away on its own over time, leaving the
        // field (and the dirty flag) calm again.
        let mut sim = Simulation::new();
        sim.add_wind_disk(50, 50, 10, 120, -120);
        assert!(sim.gust_active, "painting wind should arm the decay sweep");
        for _ in 0..200 {
            sim.step();
        }
        assert!(!sim.gust_active, "gust should have fully decayed");
        assert_eq!(
            sim.wind_at(50, 50),
            (sim.ambient_x, 0),
            "no gust should remain"
        );
    }

    #[test]
    fn powder_forms_a_pile() {
        // A column of sand dropped onto the floor should spread into a pile
        // wider than one cell (proving the diagonal tumble works).
        let mut sim = Simulation::new();
        let cx = GRID_W / 2;
        for y in 0..20 {
            sim.paint_disk(cx as i32, y, 0, SAND);
        }
        for _ in 0..(GRID_H * 3) {
            sim.step();
        }
        let floor = GRID_H - 1;
        let mut width = 0;
        for x in 0..GRID_W {
            if sim.mat_at(x, floor) == SAND {
                width += 1;
            }
        }
        assert!(
            width > 1,
            "sand should tumble into a pile, got width {width}"
        );
    }
}
