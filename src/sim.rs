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

use crate::materials::{self, MaterialId, EMPTY};

/// Simulation resolution. The renderer stretches this to fill the window, so
/// these are "logical sand pixels", independent of the actual window size.
pub const GRID_W: usize = 300;
pub const GRID_H: usize = 200;

/// One cell of the world.
#[derive(Clone, Copy)]
struct Cell {
    mat: MaterialId,
    /// Frozen-at-spawn randomness, used only for colour jitter so a cell's
    /// grain doesn't shimmer as it moves.
    variant: u8,
    /// Frame on which this cell last moved. The bottom-to-top scan skips a cell
    /// that already moved this tick, so a particle is processed at most once —
    /// without this, a *rising* particle (gas/fire) would be re-encountered by
    /// the same scan in the row above and teleport to the ceiling in one tick.
    moved: u64,
}

const VOID: Cell = Cell {
    mat: EMPTY,
    variant: 0,
    moved: 0,
};

pub struct Simulation {
    pub width: usize,
    pub height: usize,
    cells: Vec<Cell>,
    frame: u64,
    /// xorshift state for cheap, dependency-free randomness.
    rng: u32,
}

impl Simulation {
    pub fn new() -> Self {
        Self {
            width: GRID_W,
            height: GRID_H,
            cells: vec![VOID; GRID_W * GRID_H],
            frame: 0,
            rng: 0x9E37_79B9,
        }
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
            let src_density = materials::get(self.cells[si].mat).info().density;
            let tgt = materials::get(target).info();
            tgt.movable && src_density > tgt.density
        };

        if can_move {
            self.cells.swap(si, ti);
            // The active particle now lives at `ti`; stamp it so this tick's
            // scan won't process it again (see `Cell::moved`).
            self.cells[ti].moved = self.frame;
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
    }

    /// Advance the world by one tick.
    pub fn step(&mut self) {
        self.frame = self.frame.wrapping_add(1);
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
                if cell.moved == self.frame {
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
            let info = materials::get(cell.mat).info();
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
