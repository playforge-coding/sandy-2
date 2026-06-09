# Sandy

A small but extensible **falling-sand** world written in Rust, using
[`wgpu`](https://docs.rs/wgpu) for rendering and custom cellular-automata
physics. Runs natively (Windows/macOS/Linux) **and** in the browser (WebGPU,
with a WebGL fallback) from the same code.

It's grown well past sand and stone: there's water, oil, fire, and lava that
react with each other; a weather-and-growth chain (clouds rain, rain wets soil,
seeds sprout trees); ambient wind and a wind tool you can sweep; meteors you
call down on a spot; creatures (ants and birds) that live *on* the grid; a
seed-based world generator; and a Rhai scripting plugin system for adding your
own materials at runtime — all on top of an architecture where adding more is
meant to be easy.

## Materials

Painted from the on-screen picker (or the number-key shortcuts):

| Material | Behaves like | Notes |
|----------|--------------|-------|
| **Sand** | powder — falls and piles | the classic |
| **Stone** | solid — immovable | |
| **Water** | liquid — flows and finds its level | turns lava to stone on contact |
| **Lava** | liquid | spits fire; turns to stone where it meets water |
| **Oil** | liquid | flammable — ignites from nearby fire |
| **Fire** | rises like a gas | drifts and leans on the wind; spreads to flammables, then burns out |
| **Soil** | solid | terrain; rain turns it to **Wet Soil** |
| **Wood** | solid | flammable (catches slowly); tree trunks |
| **Leaves** | solid | flammable (catches fast); tree canopies |
| **Cloud** | drifts sideways and bobs up | rains from its underside |
| **Wet Soil** | solid | rained-on soil; a seed in it can sprout a tree |
| **Seeds** | powder | settle like sand, then sprout a tree from wet soil |
| **Rain** | falls, rides the wind | spawned by clouds — not directly paintable |
| **Meteor** | ballistic — coasts on its velocity | summoned by the Meteor tool; explodes into fire and lava on impact |

The reaction chain ties them together: **clouds → rain → wet soil → seeds →
trees (wood + leaves) → fire**, while **water ⇄ lava → stone** runs the other
way. Denser movable materials sink through lighter ones, so sand falls through
water for free.

## Creatures

Entities live *on* the grid rather than *in* it — discrete agents with their own
position that sense the cells around them and walk or fly over them:

- **Ant** — crawls along the terrain, climbing small steps, turning at walls and
  ledges, falling if the ground gives way; drowns in water and burns in lava.
- **Bird** — wheels through the open sky, banking away from the terrain and the
  top of the world.

Pick a creature in the panel (or press **A** / **B**) and click to drop one. A
freshly generated world already comes scattered with ants and birds.

## Tools

Beyond painting, the brush can:

- **Wind** (**W**) — sweep the cursor to blow a gust that way; wind-borne cells
  (fire, rain, clouds, light powders) slant and travel with it. There's also a
  gentle ambient breeze that oscillates on its own.
- **Meteor** (**M**) — click anywhere to call a meteor down on that spot.

## Controls

| Input | Action |
|-------|--------|
| Hold **left mouse** | Use the current tool (paint / wind / meteor / drop creature) |
| **1**–**9** | Select Sand, Stone, Water, Lava, Oil, Fire, Soil, Wood, Leaves |
| **0** / **Backspace** | Eraser |
| **W** | Wind tool |
| **M** | Meteor tool |
| **A** / **B** | Drop an Ant / Bird |
| **[** / **]** | Shrink / grow the brush |
| **C** | Clear the world |
| **G** | (Re)generate the world from the current seed |
| **R** | Pick a random seed and regenerate |

The same actions are available from the on-screen panel, which also has a seed
box and **Generate** / **Random** / **Clear** buttons. The keyboard shortcuts
and the panel drive the same state, so they stay in sync.

## World generation

Type a seed (or hit **Random**) and Sandy paints a whole landscape: a
noise-driven terrain heightmap (soil over stone) from
[FastNoise Lite](https://github.com/Auburn/FastNoiseLite), water pooled below
sea level, trees scattered across the dry land, and a sprinkling of ants and
birds. The same seed always produces the same world. Generation only *places*
cells — from there the normal tick loop takes over, so the world is alive the
moment it's drawn.

## Run on desktop

```sh
cargo run --release
```

## Run on the web

The web build uses [Trunk](https://trunk-rs.github.io/trunk/):

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
│   ├── mod.rs        the Material trait + the runtime id→material REGISTRY
│   ├── empty.rs      air / nothing (id 0)
│   ├── sand.rs       a powder  (delegates motion to behaviors::powder)
│   ├── stone.rs      a solid   (delegates motion to behaviors::solid)
│   └── …             water, oil, fire, lava, cloud, rain, soil, wet_soil,
│                     seeds, wood, leaves, meteor
├── entities/       Creatures that live *on* the grid (ants, birds)
│   ├── mod.rs        the Entity trait + the kind REGISTRY + per-instance state
│   ├── behaviors.rs  shared creature motion (walk, fly)
│   ├── ant.rs        a walker
│   └── bird.rs       a flier
├── behaviors.rs    Shared cell-movement logic materials reuse (powder, liquid,
│                   gas, solid, drift, ballistic, flammable, reactions, …)
├── sim.rs          The grid + tick loop (material-agnostic), wind, entities
├── worldgen.rs     Seed-based terrain/tree/creature generation (FastNoise Lite)
├── plugin.rs       Sandboxed Rhai scripts that add materials at runtime
├── ui.rs           The egui control panel (picker, tools, brush, seed)
├── gpu.rs          wgpu setup; uploads the grid as a texture each frame, bloom
├── app.rs          winit window, input, and the event loop (desktop/web)
├── lib.rs          Module wiring + the wasm entry point
└── main.rs         Desktop entry point
```

Materials are split from motion. A material file (e.g. `sand.rs`) implements the
`Material` trait: `info()` returns its colour/density/etc., and `update()`
delegates to a shared helper in `behaviors.rs`. So `behaviors::powder` (fall,
then tumble into a pile) is written **once** and reused by every powder; the
density/displacement rule lives once in `Simulation::try_move`. Entities mirror
this: each creature kind implements the `Entity` trait and delegates to
`entities::behaviors`. The grid scan in `sim.rs` just looks up each cell's
material in the `REGISTRY` and calls `update()` — it never hard-codes any
material.

The world is a fixed grid (`GRID_W` × `GRID_H` = 500 × 250 in `sim.rs`),
stretched to fill the window. Each tick the grid is scanned **bottom-to-top** so
a particle that falls lands in an already-processed row and only moves once per
tick — which lets the whole simulation run in place with no second buffer.
Wind-borne cells additionally carry a small velocity so gusts read as inertia
rather than teleportation. Every frame the grid is written to an RGBA texture
and drawn with a single fullscreen triangle and nearest-neighbour sampling, so
individual grains stay crisp at any window size; glowing materials (fire, lava)
are flagged for a bloom pass that gives them a soft halo.

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
           MaterialInfo { name: "Dirt", color: [110, 78, 48, 255], jitter: 22, density: 160, movable: true, glow: false }
       }
       fn update(&self, sim: &mut Simulation, x: usize, y: usize) {
           behaviors::powder(sim, x, y); // <- reuses sand's falling/piling logic
       }
   }
   ```

2. Register it in `src/materials/mod.rs`: add `mod dirt;` and a `&DIRT` entry to
   `builtins()` (its position is its id — don't reorder existing entries).
3. (Optional) bind it to a key in `App::handle_key` in `src/app.rs`.

The `density` field controls sinking: a denser `movable` material displaces a
lighter one, so once a liquid exists, sand sinks through it for free. The `glow`
field flags a material for the renderer's bloom pass.

### Adding a genuinely new behaviour

For motion that doesn't exist yet, add a helper to `behaviors.rs` (model it on
`powder`), then point your material's `update()` at it. Nothing in `sim.rs`
needs to change — it dispatches through the trait. Both files have comments
marking where these go. Adding a creature works the same way under `entities/`.

## Adding a material *without* recompiling — Rhai plugins

The material registry is built at runtime, so you can add a material from a
small [Rhai](https://rhai.rs) script and have it show up in the picker like any
built-in. Write the script in your own editor and **drag the `.rhai` file onto
the window** (or drop it in a `plugins/` folder):

```rhai
// Static properties, read once when the plugin loads.
fn info() {
    #{ name: "Acid", color: [120, 255, 60], jitter: 20, density: 90, movable: true }
}

// Called for every cell of this material, every tick. `x`,`y` is the cell.
fn update(x, y) {
    liquid(x, y, 4);                  // reuse a built-in behaviour …
    // … or drive the cell yourself with host functions like get/set/try_move,
    //    neighbor, react/transform, emit, rand_bool — see src/plugin.rs.
}
```

The script is sandboxed and runs on the simulation thread, so it can call back
into the same `powder` / `liquid` / `gas` / `solid` helpers and reaction
primitives the built-ins use. See the table at the top of `src/plugin.rs` for
the full host API.
