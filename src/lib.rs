//! Sandy — a basic, extensible falling-sand simulation.
//!
//! - `materials` — one file per material + the registry (where you add new ones).
//! - `behaviors`  — shared movement logic materials reuse (falling, piling, …).
//! - `sim`        — the grid and the tick loop.
//! - `gpu`        — wgpu setup and per-frame rendering.
//! - `ui`         — pixel-art overlay (the material picker), drawn into the buffer.
//! - `plugin`     — sandboxed Rhai scripts that add new materials at runtime.
//! - `worldgen`   — seed-based terrain/tree generation (FastNoise Lite).
//! - `app`        — winit window/input/event loop.

mod app;
mod behaviors;
mod gpu;
mod materials;
mod plugin;
mod sim;
mod ui;
mod worldgen;

pub use app::run;

// Web entry point: called automatically when the wasm module loads.
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn wasm_start() {
    run();
}
