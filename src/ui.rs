//! A tiny immediate-mode overlay drawn straight into the CPU pixel buffer.
//!
//! The renderer scales the `GRID_W × GRID_H` buffer up to the window with
//! nearest-neighbour filtering, so anything we stamp here shows up as crisp
//! pixel-art at whatever the current window size is — the same on native and
//! web, with no extra render pass or text-rendering crate.
//!
//! Right now the only widget is the material picker: one row per registered
//! material showing its average colour and name, with the selected one
//! highlighted. [`draw_picker`] paints it; [`hit_test`] maps a grid coordinate
//! back to the material whose row was clicked (or `None` for "not on the UI").

use crate::materials::{self, MaterialId};
use crate::sim::{GRID_H, GRID_W};

// ---- Layout (all in grid pixels) ----
const PANEL_X: i32 = 4;
const PANEL_Y: i32 = 4;
const PANEL_W: i32 = 46;
const PAD: i32 = 3;
const ROW_H: i32 = 12; // row pitch, including the 2px gap below each row
const ROW_GAP: i32 = 2;
const SWATCH: i32 = 8;

// ---- Colours ----
const PANEL_BG: [u8; 4] = [22, 22, 30, 230];
const ROW_SEL_BG: [u8; 4] = [58, 58, 74, 255];
const BORDER_SEL: [u8; 4] = [255, 255, 255, 255];
const SWATCH_BORDER: [u8; 4] = [10, 10, 12, 255];
const TEXT: [u8; 4] = [220, 220, 228, 255];
const TEXT_SEL: [u8; 4] = [255, 255, 255, 255];

/// Top-left of the i-th material row, and its drawable height.
fn row_rect(i: usize) -> (i32, i32, i32, i32) {
    let top = PANEL_Y + PAD + i as i32 * ROW_H;
    (PANEL_X, top, PANEL_W, ROW_H - ROW_GAP)
}

fn panel_height() -> i32 {
    PAD * 2 + materials::count() as i32 * ROW_H - ROW_GAP
}

/// Draw the material picker over `buf` (a tightly-packed `GRID_W*GRID_H*4`
/// RGBA8 buffer). `selected` is the currently-active material id, drawn
/// highlighted.
pub fn draw_picker(buf: &mut [u8], selected: MaterialId) {
    fill_rect(buf, PANEL_X, PANEL_Y, PANEL_W, panel_height(), PANEL_BG);

    for id in 0..materials::count() {
        let info = materials::get(id as MaterialId).info();
        let (rx, ry, rw, rh) = row_rect(id);
        let is_sel = id as MaterialId == selected;

        if is_sel {
            fill_rect(buf, rx, ry, rw, rh, ROW_SEL_BG);
            rect_border(buf, rx, ry, rw, rh, BORDER_SEL);
        }

        // Colour swatch, vertically centred in the row, with a dark border so a
        // black/empty swatch still reads as a box against the panel.
        let sx = rx + PAD;
        let sy = ry + (rh - SWATCH) / 2;
        fill_rect(buf, sx, sy, SWATCH, SWATCH, info.average_color());
        rect_border(buf, sx, sy, SWATCH, SWATCH, SWATCH_BORDER);

        // Name, to the right of the swatch (font is 5px tall → centre it).
        let tx = sx + SWATCH + PAD;
        let ty = ry + (rh - FONT_H) / 2;
        draw_text(buf, tx, ty, info.name, if is_sel { TEXT_SEL } else { TEXT });
    }
}

/// Map a grid coordinate to the material whose picker row contains it, or
/// `None` if the point isn't on the picker (so the caller can paint instead).
pub fn hit_test(gx: i32, gy: i32) -> Option<MaterialId> {
    for id in 0..materials::count() {
        let (rx, ry, rw, rh) = row_rect(id);
        if gx >= rx && gx < rx + rw && gy >= ry && gy < ry + rh {
            return Some(id as MaterialId);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Pixel-pushing primitives
// ---------------------------------------------------------------------------

#[inline]
fn put(buf: &mut [u8], x: i32, y: i32, c: [u8; 4]) {
    if x < 0 || y < 0 || x >= GRID_W as i32 || y >= GRID_H as i32 {
        return;
    }
    let i = (y as usize * GRID_W + x as usize) * 4;
    if c[3] == 255 {
        buf[i..i + 4].copy_from_slice(&c);
    } else {
        // Source-over alpha blend, so the semi-transparent panel lets a little
        // of the simulation show through behind it.
        let a = c[3] as u32;
        let ia = 255 - a;
        for k in 0..3 {
            buf[i + k] = ((c[k] as u32 * a + buf[i + k] as u32 * ia) / 255) as u8;
        }
        buf[i + 3] = 255;
    }
}

fn fill_rect(buf: &mut [u8], x: i32, y: i32, w: i32, h: i32, c: [u8; 4]) {
    for dy in 0..h {
        for dx in 0..w {
            put(buf, x + dx, y + dy, c);
        }
    }
}

fn rect_border(buf: &mut [u8], x: i32, y: i32, w: i32, h: i32, c: [u8; 4]) {
    for dx in 0..w {
        put(buf, x + dx, y, c);
        put(buf, x + dx, y + h - 1, c);
    }
    for dy in 0..h {
        put(buf, x, y + dy, c);
        put(buf, x + w - 1, y + dy, c);
    }
}

// ---------------------------------------------------------------------------
// Minimal 3×5 bitmap font (uppercase A–Z, 0–9, space). Each glyph is 5 rows of
// 3 bits; bit 0b100 is the left column. Unknown chars render as blank.
// ---------------------------------------------------------------------------

const FONT_W: i32 = 3;
const FONT_H: i32 = 5;
const GLYPH_ADVANCE: i32 = FONT_W + 1;

fn draw_text(buf: &mut [u8], mut x: i32, y: i32, text: &str, c: [u8; 4]) {
    for ch in text.chars() {
        let glyph = glyph(ch);
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..FONT_W {
                if bits & (0b100 >> col) != 0 {
                    put(buf, x + col, y + row as i32, c);
                }
            }
        }
        x += GLYPH_ADVANCE;
    }
}

fn glyph(ch: char) -> [u8; 5] {
    match ch.to_ascii_uppercase() {
        'A' => [0b111, 0b101, 0b111, 0b101, 0b101],
        'B' => [0b110, 0b101, 0b110, 0b101, 0b110],
        'C' => [0b011, 0b100, 0b100, 0b100, 0b011],
        'D' => [0b110, 0b101, 0b101, 0b101, 0b110],
        'E' => [0b111, 0b100, 0b110, 0b100, 0b111],
        'F' => [0b111, 0b100, 0b110, 0b100, 0b100],
        'G' => [0b011, 0b100, 0b101, 0b101, 0b011],
        'H' => [0b101, 0b101, 0b111, 0b101, 0b101],
        'I' => [0b111, 0b010, 0b010, 0b010, 0b111],
        'J' => [0b001, 0b001, 0b001, 0b101, 0b011],
        'K' => [0b101, 0b110, 0b100, 0b110, 0b101],
        'L' => [0b100, 0b100, 0b100, 0b100, 0b111],
        'M' => [0b101, 0b111, 0b111, 0b101, 0b101],
        'N' => [0b101, 0b111, 0b111, 0b111, 0b101],
        'O' => [0b111, 0b101, 0b101, 0b101, 0b111],
        'P' => [0b111, 0b101, 0b111, 0b100, 0b100],
        'Q' => [0b111, 0b101, 0b101, 0b111, 0b011],
        'R' => [0b110, 0b101, 0b110, 0b101, 0b101],
        'S' => [0b111, 0b100, 0b111, 0b001, 0b111],
        'T' => [0b111, 0b010, 0b010, 0b010, 0b010],
        'U' => [0b101, 0b101, 0b101, 0b101, 0b111],
        'V' => [0b101, 0b101, 0b101, 0b101, 0b010],
        'W' => [0b101, 0b101, 0b111, 0b111, 0b101],
        'X' => [0b101, 0b101, 0b010, 0b101, 0b101],
        'Y' => [0b101, 0b101, 0b010, 0b010, 0b010],
        'Z' => [0b111, 0b001, 0b010, 0b100, 0b111],
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b010, 0b100, 0b100],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        _ => [0, 0, 0, 0, 0], // space and anything unsupported
    }
}
