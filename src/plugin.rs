//! Sandboxed material plugins.
//!
//! A plugin is a small [Rhai](https://rhai.rs) script the user writes in their
//! own editor and drops onto the window (or into a `plugins/` folder). It
//! defines a new material — colour, density, and a per-tick `update` — which is
//! compiled here, wrapped in a [`ScriptMaterial`], and appended to the material
//! registry so it shows up in the picker like any built-in.
//!
//! ## The script contract
//!
//! ```rhai
//! // Static properties, read once when the plugin loads.
//! fn info() {
//!     #{ name: "Acid", color: [120, 255, 60], jitter: 20, density: 90, movable: true }
//! }
//!
//! // Called for every cell of this material, every tick. `x`,`y` is the cell.
//! fn update(x, y) {
//!     // Reuse a built-in behaviour …
//!     liquid(x, y, 4);
//!     // … or drive the cell yourself with the host API below.
//! }
//! ```
//!
//! The script can call these host functions (see [`register_host_api`]):
//!
//! | function | meaning |
//! |----------|---------|
//! | `width()`, `height()`            | grid size |
//! | `get(x, y)`                      | material id in a cell (0 = empty, out-of-bounds = empty) |
//! | `material_id(name)`              | id of a material by name, or `-1` |
//! | `try_move(sx, sy, tx, ty)`       | move/swap if density rules allow; returns success |
//! | `set(x, y, id)`                  | overwrite a cell with a material |
//! | `neighbor(x, y, id)`            | `[nx, ny]` of an orthogonal neighbour of that material, or `[]` |
//! | `react(x, y, trigger, product)` | if a `trigger` neighbour touches, turn both into `product` |
//! | `transform(x, y, trigger, product)` | if a `trigger` neighbour touches, turn *just this* cell into `product` |
//! | `emit(x, y, product, rarity)`   | with chance `1/rarity`, shed `product` into an empty neighbour |
//! | `rand_bool()`                   | a coin flip |
//! | `powder(x, y)`, `liquid(x, y, speed)`, `gas(x, y, speed)`, `solid()` | the shared built-in behaviours |
//!
//! ## Why it's safe to run untrusted scripts
//!
//! Rhai is a pure-Rust interpreter with no file, network, or system access of
//! its own. On top of that the engine is capped (operations per tick, call
//! depth, string/array/map sizes), `import` and `eval` are disabled, and every
//! host function validates its coordinates so a script can't index out of bounds
//! or panic the host. A script that errors is reported once and then its cells
//! are left untouched — a bad plugin can waste a little time, never crash the app.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::ptr;

use rhai::{Array, Dynamic, Engine, Map, Scope, AST};

use crate::behaviors;
use crate::materials::{self, Material, MaterialId, MaterialInfo, EMPTY};
use crate::sim::Simulation;

// ---------------------------------------------------------------------------
// Lending the simulation to script-callable host functions
// ---------------------------------------------------------------------------
//
// The host functions a script calls (`try_move`, `set`, …) need `&mut
// Simulation`, but Rhai calls them through a `'static` engine and can't carry a
// borrow. So while a script's `update` runs we park a pointer to the
// (exclusively borrowed) simulation in a thread-local and hand it back out for
// the duration of the call — exactly the window in which it's valid.

thread_local! {
    static CURRENT_SIM: Cell<*mut Simulation> = const { Cell::new(ptr::null_mut()) };
}

/// RAII guard that lends `sim` to the host API for as long as it lives, then
/// revokes access on drop (including on panic/early-return). Created right
/// before a script call and dropped right after, so the parked pointer is only
/// ever live while a real `&mut Simulation` is sitting on the stack above it.
struct SimLease;

impl SimLease {
    fn new(sim: &mut Simulation) -> Self {
        CURRENT_SIM.with(|p| p.set(sim as *mut Simulation));
        SimLease
    }
}

impl Drop for SimLease {
    fn drop(&mut self) {
        CURRENT_SIM.with(|p| p.set(ptr::null_mut()));
    }
}

/// Run `f` against the currently-leased simulation, or return `default` if no
/// script call is in flight. This is the only door from script-land back to the
/// grid, so a script can never reach a simulation that isn't actively updating.
fn with_sim<R>(default: R, f: impl FnOnce(&mut Simulation) -> R) -> R {
    CURRENT_SIM.with(|p| {
        let raw = p.get();
        if raw.is_null() {
            return default;
        }
        // SAFETY: the pointer is non-null only between `SimLease::new` and its
        // drop, during which the borrowed `&mut Simulation` is parked and
        // untouched. The simulation is single-threaded and Rhai calls are
        // synchronous, so this is the only live reference while `f` runs — no
        // aliasing.
        f(unsafe { &mut *raw })
    })
}

// ---------------------------------------------------------------------------
// A material backed by a script
// ---------------------------------------------------------------------------

/// A material whose per-tick behaviour is a compiled Rhai `update` function.
/// Its identity ([`MaterialInfo`]) is read once at load time and cached, so the
/// hot paths (rendering, density checks) never re-enter the interpreter.
struct ScriptMaterial {
    engine: Engine,
    ast: AST,
    /// Persists across ticks so script-level state/`let` bindings survive.
    scope: RefCell<Scope<'static>>,
    info: MaterialInfo,
    /// Whether the script defines an `update` (a purely static material needn't).
    has_update: bool,
    /// Latches after the first runtime error so a broken plugin reports once
    /// instead of once per cell per frame.
    reported: Cell<bool>,
}

impl Material for ScriptMaterial {
    fn info(&self) -> MaterialInfo {
        self.info
    }

    fn update(&self, sim: &mut Simulation, x: usize, y: usize) {
        if !self.has_update {
            return;
        }
        // Lend the simulation, then run the script. The lease drops at the end
        // of the scope, revoking access before we return to the tick loop.
        let _lease = SimLease::new(sim);
        let mut scope = self.scope.borrow_mut();
        let result =
            self.engine
                .call_fn::<()>(&mut scope, &self.ast, "update", (x as i64, y as i64));
        if let Err(err) = result {
            if !self.reported.replace(true) {
                log::error!("plugin \"{}\" update failed: {err}", self.info.name);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The sandboxed engine + host API
// ---------------------------------------------------------------------------

/// Build a fresh, locked-down engine. Each plugin gets its own so one script
/// can't see or clobber another's globals.
fn build_engine() -> Engine {
    let mut engine = Engine::new();

    // Resource caps: a script can't hang the tick loop or exhaust memory.
    engine.set_max_operations(100_000); // per `call_fn` run → bounds infinite loops
    engine.set_max_call_levels(32);
    engine.set_max_expr_depths(64, 32);
    engine.set_max_string_size(8 * 1024);
    engine.set_max_array_size(1024);
    engine.set_max_map_size(1024);
    engine.set_max_modules(0); // no `import`
    engine.disable_symbol("eval"); // no dynamic code generation

    register_host_api(&mut engine);
    engine
}

/// Convert a script-supplied coordinate to an in-bounds cell index component,
/// or `None` if it's negative or past the edge. Keeps every host function from
/// panicking on a hostile or buggy index.
#[inline]
fn cell(v: i64, max: usize) -> Option<usize> {
    if v < 0 || v as u128 >= max as u128 {
        None
    } else {
        Some(v as usize)
    }
}

/// A material id is valid if it's currently registered.
#[inline]
fn valid_mat(id: i64) -> Option<MaterialId> {
    if id >= 0 && (id as usize) < materials::count() {
        Some(id as MaterialId)
    } else {
        None
    }
}

/// Register every function a plugin script may call. All of them go through
/// [`with_sim`] and validate their inputs, so they're safe to expose to
/// untrusted code.
fn register_host_api(engine: &mut Engine) {
    engine.register_fn("width", || with_sim(0_i64, |s| s.width as i64));
    engine.register_fn("height", || with_sim(0_i64, |s| s.height as i64));

    // Material id in a cell; out-of-bounds reads as empty.
    engine.register_fn("get", |x: i64, y: i64| {
        with_sim(EMPTY as i64, |s| {
            match (cell(x, s.width), cell(y, s.height)) {
                (Some(x), Some(y)) => s.mat_at(x, y) as i64,
                _ => EMPTY as i64,
            }
        })
    });

    // Look up a material id by name (e.g. to react with "Water"). -1 if unknown.
    engine.register_fn("material_id", |name: &str| {
        materials::id_by_name(name).map_or(-1_i64, |id| id as i64)
    });

    engine.register_fn("rand_bool", || with_sim(false, |s| s.rand_bool()));

    // Move/swap, honouring the shared density/movable rules. False if either end
    // is out of bounds or the move is blocked.
    engine.register_fn("try_move", |sx: i64, sy: i64, tx: i64, ty: i64| {
        with_sim(false, |s| {
            match (
                cell(sx, s.width),
                cell(sy, s.height),
                cell(tx, s.width),
                cell(ty, s.height),
            ) {
                (Some(sx), Some(sy), Some(tx), Some(ty)) => s.try_move(sx, sy, tx, ty),
                _ => false,
            }
        })
    });

    // Overwrite a cell with a material (ignored if the cell or id is invalid).
    engine.register_fn("set", |x: i64, y: i64, mat: i64| {
        with_sim((), |s| {
            if let (Some(x), Some(y), Some(mat)) =
                (cell(x, s.width), cell(y, s.height), valid_mat(mat))
            {
                s.set(x, y, mat);
            }
        })
    });

    // `[nx, ny]` of an orthogonal neighbour of material `mat`, else `[]`.
    engine.register_fn("neighbor", |x: i64, y: i64, mat: i64| -> Array {
        with_sim(Array::new(), |s| {
            match (cell(x, s.width), cell(y, s.height), valid_mat(mat)) {
                (Some(x), Some(y), Some(mat)) => match s.neighbor(x, y, mat) {
                    Some((nx, ny)) => vec![Dynamic::from(nx as i64), Dynamic::from(ny as i64)],
                    None => Array::new(),
                },
                _ => Array::new(),
            }
        })
    });

    // Contact reaction: if a `trigger` neighbour touches, turn both cells into
    // `product`. Mirrors `behaviors::react_on_contact`.
    engine.register_fn("react", |x: i64, y: i64, trigger: i64, product: i64| {
        with_sim(false, |s| {
            match (
                cell(x, s.width),
                cell(y, s.height),
                valid_mat(trigger),
                valid_mat(product),
            ) {
                (Some(x), Some(y), Some(trigger), Some(product)) => {
                    behaviors::react_on_contact(s, x, y, trigger, product)
                }
                _ => false,
            }
        })
    });

    // One-sided reaction: turn just this cell into `product` on contact with a
    // `trigger` neighbour (a catalyst that isn't consumed). Mirrors
    // `behaviors::transform_on_contact`.
    engine.register_fn("transform", |x: i64, y: i64, trigger: i64, product: i64| {
        with_sim(false, |s| {
            match (
                cell(x, s.width),
                cell(y, s.height),
                valid_mat(trigger),
                valid_mat(product),
            ) {
                (Some(x), Some(y), Some(trigger), Some(product)) => {
                    behaviors::transform_on_contact(s, x, y, trigger, product)
                }
                _ => false,
            }
        })
    });

    // Occasionally shed `product` into an empty neighbour (a source like lava
    // giving off fire). Mirrors `behaviors::emit`.
    engine.register_fn("emit", |x: i64, y: i64, product: i64, rarity: i64| {
        with_sim(false, |s| {
            match (cell(x, s.width), cell(y, s.height), valid_mat(product)) {
                (Some(x), Some(y), Some(product)) => {
                    behaviors::emit(s, x, y, product, rarity.clamp(1, i64::from(u32::MAX)) as u32)
                }
                _ => false,
            }
        })
    });

    // The shared built-in behaviours, so a plugin can get powder/liquid/gas/
    // solid motion for free — exactly like a built-in material delegating to them.
    engine.register_fn("powder", |x: i64, y: i64| {
        with_sim((), |s| {
            if let (Some(x), Some(y)) = (cell(x, s.width), cell(y, s.height)) {
                behaviors::powder(s, x, y);
            }
        })
    });
    engine.register_fn("liquid", |x: i64, y: i64, speed: i64| {
        with_sim((), |s| {
            if let (Some(x), Some(y)) = (cell(x, s.width), cell(y, s.height)) {
                behaviors::liquid(s, x, y, speed.clamp(0, 64) as usize);
            }
        })
    });
    engine.register_fn("gas", |x: i64, y: i64, speed: i64| {
        with_sim((), |s| {
            if let (Some(x), Some(y)) = (cell(x, s.width), cell(y, s.height)) {
                behaviors::gas(s, x, y, speed.clamp(0, 64) as usize);
            }
        })
    });
    engine.register_fn("solid", || {});
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

/// Compile a plugin from source and register it, returning its new id.
///
/// On any problem — parse error, missing/invalid `info`, a full id space — it
/// returns a human-readable message instead of registering anything.
pub fn load_source(src: &str) -> Result<MaterialId, String> {
    let engine = build_engine();
    let ast = engine
        .compile(src)
        .map_err(|e| format!("parse error: {e}"))?;

    // `info()` is required and read exactly once.
    let mut scope = Scope::new();
    let info_map: Map = engine
        .call_fn(&mut scope, &ast, "info", ())
        .map_err(|e| format!("info() failed (a plugin must define `fn info()`): {e}"))?;
    let info = parse_info(&info_map)?;

    let has_update = ast
        .iter_functions()
        .any(|f| f.name == "update" && f.params.len() == 2);

    let material = ScriptMaterial {
        engine,
        ast,
        scope: RefCell::new(scope),
        info,
        has_update,
        reported: Cell::new(false),
    };

    // Leak it: a registered material lives for the rest of the run, and the
    // registry stores `&'static dyn Material`. (Reloading a plugin leaks the old
    // copy — fine for an occasional action, and each engine is memory-capped.)
    let leaked: &'static ScriptMaterial = Box::leak(Box::new(material));
    materials::register(leaked).ok_or_else(|| "material registry is full".to_string())
}

/// Read a `.rhai` file and load it as a plugin.
pub fn load_path(path: &Path) -> Result<MaterialId, String> {
    let src = std::fs::read_to_string(path).map_err(|e| format!("cannot read {path:?}: {e}"))?;
    load_source(&src)
}

/// Load every `.rhai` file in `dir` (if it exists), returning how many loaded.
/// Per-file failures are logged and skipped so one bad plugin can't block the
/// rest.
pub fn load_dir(dir: &Path) -> usize {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return 0, // no plugins/ folder is the normal case
    };
    let mut loaded = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rhai") {
            continue;
        }
        match load_path(&path) {
            Ok(id) => {
                log::info!("loaded plugin {path:?} as material {id}");
                loaded += 1;
            }
            Err(e) => log::error!("skipping plugin {path:?}: {e}"),
        }
    }
    loaded
}

// ---------------------------------------------------------------------------
// Parsing the `info()` map
// ---------------------------------------------------------------------------

fn parse_info(map: &Map) -> Result<MaterialInfo, String> {
    let name = map
        .get("name")
        .map(|d| d.clone().into_string())
        .transpose()
        .map_err(|_| "info() field `name` must be a string")?
        .filter(|s| !s.is_empty())
        .ok_or("info() must include a non-empty string `name`")?;
    // The registry stores `&'static str`; a plugin's name lives for the run, so
    // leaking the string is the simplest way to satisfy that.
    let name: &'static str = Box::leak(name.into_boxed_str());

    let color = parse_color(map.get("color"))?;
    let jitter = int_field(map, "jitter", 0).clamp(0, 255) as u8;
    let density = int_field(map, "density", 128).clamp(0, 255) as u8;
    let movable = map
        .get("movable")
        .and_then(|d| d.as_bool().ok())
        .unwrap_or(true);

    Ok(MaterialInfo {
        name,
        color,
        jitter,
        density,
        movable,
    })
}

/// Read `color` as `[r, g, b]` or `[r, g, b, a]` (0–255). Defaults alpha to 255.
fn parse_color(value: Option<&Dynamic>) -> Result<[u8; 4], String> {
    let arr = value
        .cloned()
        .map(Dynamic::into_array)
        .transpose()
        .map_err(|_| "info() field `color` must be an array")?
        .ok_or("info() must include `color: [r, g, b]`")?;
    if arr.len() < 3 {
        return Err("`color` needs at least 3 components [r, g, b]".into());
    }
    let ch = |i: usize| arr[i].as_int().unwrap_or(0).clamp(0, 255) as u8;
    let a = if arr.len() >= 4 {
        arr[3].as_int().unwrap_or(255).clamp(0, 255) as u8
    } else {
        255
    };
    Ok([ch(0), ch(1), ch(2), a])
}

fn int_field(map: &Map, key: &str, default: i64) -> i64 {
    map.get(key)
        .and_then(|d| d.as_int().ok())
        .unwrap_or(default)
}
