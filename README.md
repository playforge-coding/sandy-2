# Sandy

A basic but extensible **falling-sand** simulation written in Rust, using
[`wgpu`](https://docs.rs/wgpu) for rendering and custom cellular-automata
physics. Runs natively (Windows/macOS/Linux) **and** in the browser (WebGPU,
with a WebGL fallback) from the same code.

Ships with two materials — **Sand** and **Stone** — and is structured so adding
more is trivial.

## Controls

| Input | Action |
|-------|--------|
| Hold **left mouse** | Draw with the current material |
| **1** | Select Sand |
| **2** | Select Stone |
| **0** / **Backspace** | Eraser |
| **[** / **]** | Shrink / grow the brush |
| **C** | Clear the world |

## Run on desktop

```sh
cargo run --release
```

## Run on the web

The web build uses [Trunk](https://trunkrs.dev):

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk
trunk serve --release      # then open http://localhost:8080
```

`trunk build --release` produces a static site in `dist/` you can host
anywhere. (Serving over HTTPS or `localhost` is required for the browser to
expose WebGPU; without it, the build falls back to WebGL automatically.)

## How it works

```
src/
├── materials/      One file per material; each declares its properties and
│   ├── mod.rs        the Material trait + the id→material REGISTRY
│   ├── empty.rs      air / nothing (id 0)
│   ├── sand.rs       a powder  (delegates motion to behaviors::powder)
│   └── stone.rs      a solid   (delegates motion to behaviors::solid)
├── behaviors.rs    Shared movement logic materials reuse (falling/piling, …)
├── sim.rs          The grid + tick loop (material-agnostic) and physics helpers
├── gpu.rs          wgpu setup; uploads the grid as a texture each frame and blits it
├── app.rs          winit window, input, and the event loop (shared desktop/web)
├── lib.rs          Module wiring + the wasm entry point
└── main.rs         Desktop entry point
```

Materials are split from motion. A material file (e.g. `sand.rs`) implements the
`Material` trait: `info()` returns its colour/density/etc., and `update()`
delegates to a shared helper in `behaviors.rs`. So `behaviors::powder` (fall,
then tumble into a pile) is written **once** and reused by every powder; the
density/displacement rule lives once in `Simulation::try_move`. The grid scan in
`sim.rs` just looks up each cell's material in the `REGISTRY` and calls
`update()` — it never hard-codes any material.

The world is a fixed grid (`GRID_W` × `GRID_H` in `sim.rs`). Each tick the grid
is scanned **bottom-to-top** so a particle that falls lands in an
already-processed row and only moves once per tick — which lets the whole
simulation run in place with no second buffer. Every frame the grid is written
to a small RGBA texture and drawn to the window with a single fullscreen
triangle and nearest-neighbour sampling, so individual grains stay crisp at any
window size.

## Adding a new material

If it moves like something that already exists, it's a small new file that
**reuses the shared behaviour**. Three steps:

1. Create `src/materials/dirt.rs` (copy `sand.rs`):

   ```rust
   use super::{Material, MaterialInfo};
   use crate::behaviors;
   use crate::sim::Simulation;

   pub struct Dirt;

   impl Material for Dirt {
       fn info(&self) -> MaterialInfo {
           MaterialInfo { name: "Dirt", color: [110, 78, 48, 255], jitter: 22, density: 160, movable: true }
       }
       fn update(&self, sim: &mut Simulation, x: usize, y: usize) {
           behaviors::powder(sim, x, y); // <- reuses sand's falling/piling logic
       }
   }
   ```

2. Register it in `src/materials/mod.rs`: add `mod dirt;` and `&dirt::Dirt,` to
   `REGISTRY` (its position is its id).
3. (Optional) bind it to a key in `App::handle_key` in `src/app.rs`.

The `density` field controls sinking: a denser `movable` material displaces a
lighter one, so once a liquid exists, sand sinks through it for free.

### Adding a genuinely new behaviour

For motion that doesn't exist yet (a liquid that flows sideways, a gas that
rises): add a helper to `behaviors.rs` (model it on `powder`), then point your
material's `update()` at it. Nothing in `sim.rs` needs to change — it dispatches
through the trait. Both files have comments marking where these go.
