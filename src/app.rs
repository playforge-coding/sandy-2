//! Windowing, input, and the event loop — the glue between winit, the GPU
//! state, egui, and the simulation. Identical code path on desktop and web;
//! only the async device-init handoff differs (see `resumed`).
//!
//! Window events are offered to egui first; whatever it doesn't consume (clicks
//! outside the panel, un-focused key presses) drives painting and the keyboard
//! shortcuts. Each redraw runs egui, steps the sim, and hands the tessellated UI
//! to [`State::render`] to layer over the scene.

use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::gpu::State;
use crate::materials::EMPTY;
use crate::ui;

/// Delivered once the (async) GPU state has finished initialising. On the web
/// the device request can't block, so it's built off-thread and handed back
/// through the event loop.
#[allow(dead_code)] // constructed only on the web (async-init handoff).
pub enum UserEvent {
    StateReady(State),
}

/// Mouse/keyboard painting state, plus the egui-shared [`ui::Controls`]
/// (selected material, brush size, seed) that the panel and the keyboard
/// shortcuts both drive.
struct Input {
    cursor: (f64, f64),
    drawing: bool,
    /// Grid cell the wind tool was at on the previous painted frame, so a drag
    /// yields a direction (this → now). `None` at the start of a stroke (and for
    /// the paint tool), so the first frame only seeds the position and blows
    /// nothing.
    last_wind: Option<(i32, i32)>,
    controls: ui::Controls,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            cursor: (0.0, 0.0),
            drawing: false,
            last_wind: None,
            controls: ui::Controls::default(),
        }
    }
}

/// Sub-units of wind added per grid cell the cursor sweeps (see
/// [`crate::sim::Simulation::add_wind_disk`]). A brisk flick saturates the gust
/// field for a strong, short-lived blast; a slow drag nudges gently.
const WIND_DRAG_GAIN: i32 = 9;

struct App {
    // Only consumed on the web (the async-init handoff); unused on native.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    proxy: EventLoopProxy<UserEvent>,
    state: Option<State>,
    input: Input,
    /// egui's persistent context (fonts, memory, layout). Cheap to clone — it's
    /// an `Arc` inside — so we hand clones to per-frame work freely.
    egui_ctx: egui::Context,
    /// Per-window egui input translation; built once the window exists.
    egui_state: Option<egui_winit::State>,
}

impl App {
    fn new(proxy: EventLoopProxy<UserEvent>) -> Self {
        Self {
            proxy,
            state: None,
            input: Input::default(),
            egui_ctx: egui::Context::default(),
            egui_state: None,
        }
    }

    /// Build the egui ↔ winit bridge once the GPU state (and thus the window)
    /// is ready. Idempotent: a no-op if already built or the window isn't up.
    fn ensure_egui(&mut self) {
        if self.egui_state.is_some() {
            return;
        }
        let Some(state) = &self.state else {
            return;
        };
        self.egui_state = Some(egui_winit::State::new(
            self.egui_ctx.clone(),
            egui::ViewportId::ROOT,
            state.window(),
            Some(state.window().scale_factor() as f32),
            None,
            Some(state.max_texture_side()),
        ));
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Only initialise once (`resumed` can fire again on some platforms).
        if self.state.is_some() {
            return;
        }

        let mut attrs = Window::default_attributes().with_title("Sandy — falling sand");
        #[cfg(not(target_arch = "wasm32"))]
        {
            attrs = attrs.with_inner_size(winit::dpi::LogicalSize::new(900.0, 600.0));
        }
        #[cfg(target_arch = "wasm32")]
        {
            use winit::platform::web::WindowAttributesExtWebSys;
            // Create and append a <canvas> to the document body.
            attrs = attrs
                .with_append(true)
                .with_inner_size(winit::dpi::LogicalSize::new(900.0, 600.0));
        }

        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));

        #[cfg(not(target_arch = "wasm32"))]
        {
            // Native: just block until the GPU is ready.
            let state = pollster::block_on(State::new(window));
            self.state = Some(state);
            self.ensure_egui();
        }
        #[cfg(target_arch = "wasm32")]
        {
            // Web: build the state asynchronously and post it back via the proxy.
            let proxy = self.proxy.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let state = State::new(window).await;
                let _ = proxy.send_event(UserEvent::StateReady(state));
            });
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        let UserEvent::StateReady(state) = event;
        self.state = Some(state);
        self.ensure_egui();
        if let Some(state) = &mut self.state {
            // On the web the canvas only gets its real size after layout, which
            // lands while the GPU device is still initialising — so the initial
            // `Resized` event fires before `state` exists and is dropped, leaving
            // the surface at the 0x0→1x1 size it was first configured with. The
            // WebGPU backend forces the canvas backing store to match that
            // config, so without this the canvas stays 1x1 and the whole page is
            // a single stretched pixel. Re-apply the window's current size now
            // that the state is live. (WebGL happened to mask this by rendering
            // at the canvas's own backing size.)
            let size = state.window().inner_size();
            state.resize(size.width, size.height);
            state.window().request_redraw();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        // Offer the event to egui first; `consumed` means it landed on the UI
        // (a click on the panel, typing into the seed box) and shouldn't also
        // paint or trigger a shortcut.
        let consumed = match (&self.state, &mut self.egui_state) {
            (Some(state), Some(egui_state)) => {
                egui_state.on_window_event(state.window(), &event).consumed
            }
            _ => false,
        };

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::Resized(size) => {
                if let Some(state) = &mut self.state {
                    state.resize(size.width, size.height);
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                self.input.cursor = (position.x, position.y);
            }

            // Drag a `.rhai` plugin onto the window to "upload" it: it compiles,
            // joins the registry, shows up in the picker, and is selected ready
            // to paint. A bad script is logged and otherwise ignored.
            WindowEvent::DroppedFile(path) => match crate::plugin::load_path(&path) {
                Ok(id) => {
                    log::info!("loaded plugin material {id} from {path:?}");
                    self.input.controls.material = id;
                }
                Err(e) => log::error!("failed to load plugin {path:?}: {e}"),
            },

            WindowEvent::MouseInput {
                state: btn,
                button: MouseButton::Left,
                ..
            } => {
                if btn == ElementState::Pressed && !consumed {
                    // The meteor and creature tools act once per click rather than
                    // painting a held stroke, so handle them here and leave
                    // `drawing` off.
                    match self.input.controls.tool {
                        ui::Tool::Meteor => self.summon_meteor(),
                        ui::Tool::Tsunami => self.summon_tsunami(),
                        ui::Tool::GammaBurst => self.summon_gamma_burst(),
                        ui::Tool::Creature => self.place_creature(),
                        _ => {
                            self.input.drawing = true;
                            self.input.last_wind = None; // a fresh stroke has no direction yet
                        }
                    }
                } else if btn != ElementState::Pressed {
                    self.input.drawing = false;
                }
            }

            // Touch (mobile / touchscreens) paints just like the mouse. The
            // location rides on the event itself rather than arriving as a
            // separate `CursorMoved`, so update the cursor here. egui sees the
            // touch first; `consumed` means it landed on the panel, so — as with
            // a click — we don't also start painting.
            WindowEvent::Touch(touch) => {
                self.input.cursor = (touch.location.x, touch.location.y);
                match touch.phase {
                    TouchPhase::Started => match (consumed, self.input.controls.tool) {
                        // One meteor / tsunami / burst / creature per tap.
                        (false, ui::Tool::Meteor) => self.summon_meteor(),
                        (false, ui::Tool::Tsunami) => self.summon_tsunami(),
                        (false, ui::Tool::GammaBurst) => self.summon_gamma_burst(),
                        (false, ui::Tool::Creature) => self.place_creature(),
                        _ => {
                            self.input.drawing = !consumed;
                            self.input.last_wind = None; // fresh stroke, no direction yet
                        }
                    },
                    TouchPhase::Moved => {} // keep painting; cursor already updated
                    TouchPhase::Ended | TouchPhase::Cancelled => self.input.drawing = false,
                }
            }

            WindowEvent::KeyboardInput { event, .. } => {
                // Skip shortcuts while egui wants the key (e.g. the seed box has
                // focus, so digits type a seed instead of selecting a material).
                if !consumed && event.state == ElementState::Pressed {
                    if let PhysicalKey::Code(code) = event.physical_key {
                        self.handle_key(code);
                    }
                }
            }

            WindowEvent::RedrawRequested => self.redraw(),

            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        // Keep animating: request the next frame continuously.
        if let Some(state) = &self.state {
            state.window().request_redraw();
        }
    }
}

impl App {
    /// One frame: paint under the cursor, run egui, step the sim, and draw the
    /// scene with the UI on top.
    fn redraw(&mut self) {
        if self.state.is_none() || self.egui_state.is_none() {
            return;
        }

        // Paint (or blow wind) under the cursor while the mouse is held.
        if self.input.drawing {
            let controls = &self.input.controls;
            let brush = controls.brush;
            let state = self.state.as_mut().unwrap();
            let (gx, gy) = state.cursor_to_grid(self.input.cursor);
            match controls.tool {
                ui::Tool::Paint => {
                    state.sim.paint_disk(gx, gy, brush, controls.material);
                }
                ui::Tool::Wind => {
                    // Blow a gust in the direction the cursor swept since last
                    // frame. The first frame of a stroke just seeds the position.
                    if let Some((px, py)) = self.input.last_wind {
                        let dvx = (gx - px) * WIND_DRAG_GAIN;
                        let dvy = (gy - py) * WIND_DRAG_GAIN;
                        // A little heft so even a small brush makes a felt gust.
                        let radius = brush.max(5);
                        state.sim.add_wind_disk(gx, gy, radius, dvx, dvy);
                    }
                    self.input.last_wind = Some((gx, gy));
                }
                // Meteors, tsunamis, bursts and creatures act on click (see
                // `summon_meteor` / `summon_tsunami` / `summon_gamma_burst` /
                // `place_creature`), not on a held drag, so a held stroke does
                // nothing here.
                ui::Tool::Meteor
                | ui::Tool::Tsunami
                | ui::Tool::GammaBurst
                | ui::Tool::Creature => {}
            }
        }

        // Run egui for this frame. The window handle is cloned so it doesn't tie
        // up a borrow of `self.state` while we later mutate it.
        let window = self.state.as_ref().unwrap().window_arc();
        let raw_input = self.egui_state.as_mut().unwrap().take_egui_input(&window);
        let ctx = self.egui_ctx.clone();
        let mut actions = ui::Actions::default();
        let full_output = ctx.run(raw_input, |ctx| {
            actions = ui::draw(ctx, &mut self.input.controls);
        });
        self.egui_state
            .as_mut()
            .unwrap()
            .handle_platform_output(&window, full_output.platform_output);
        let paint_jobs = ctx.tessellate(full_output.shapes, full_output.pixels_per_point);

        // Apply any world-gen requests from the panel buttons.
        self.apply_actions(&actions);

        // Step the sim and draw the scene + UI.
        if let Some(state) = &mut self.state {
            state.update();
            state.render(
                paint_jobs,
                full_output.textures_delta,
                full_output.pixels_per_point,
            );
        }
    }

    /// Call a meteor down on the cell under the cursor (the Meteor tool's click).
    fn summon_meteor(&mut self) {
        if let Some(state) = &mut self.state {
            let (gx, gy) = state.cursor_to_grid(self.input.cursor);
            state.sim.spawn_meteor(gx, gy);
        }
    }

    /// Send a tsunami rolling toward the cell under the cursor (the Tsunami
    /// tool's click).
    fn summon_tsunami(&mut self) {
        if let Some(state) = &mut self.state {
            let (gx, gy) = state.cursor_to_grid(self.input.cursor);
            state.sim.spawn_tsunami(gx, gy);
        }
    }

    /// Call down a gamma-ray burst on the column under the cursor (the Gamma Ray
    /// tool's click).
    fn summon_gamma_burst(&mut self) {
        if let Some(state) = &mut self.state {
            let (gx, gy) = state.cursor_to_grid(self.input.cursor);
            state.sim.spawn_gamma_burst(gx, gy);
        }
    }

    /// Drop the selected creature at the cell under the cursor (the Creature
    /// tool's click).
    fn place_creature(&mut self) {
        if let Some(state) = &mut self.state {
            let (gx, gy) = state.cursor_to_grid(self.input.cursor);
            state.sim.spawn_entity(self.input.controls.entity, gx, gy);
        }
    }

    /// Carry out the world-generation buttons the panel reported this frame.
    fn apply_actions(&mut self, actions: &ui::Actions) {
        if actions.clear {
            if let Some(state) = &mut self.state {
                state.sim.clear();
            }
        }
        if actions.generate {
            self.regenerate();
        }
        if actions.randomize {
            self.randomize_seed();
        }
    }

    fn handle_key(&mut self, code: KeyCode) {
        let c = &mut self.input.controls;
        // Selecting any material (the digit keys) implies the paint tool — set it
        // once here so each arm needn't repeat it.
        if matches!(
            code,
            KeyCode::Digit1
                | KeyCode::Digit2
                | KeyCode::Digit3
                | KeyCode::Digit4
                | KeyCode::Digit5
                | KeyCode::Digit6
                | KeyCode::Digit7
                | KeyCode::Digit8
                | KeyCode::Digit9
                | KeyCode::Digit0
                | KeyCode::Backspace
        ) {
            c.tool = ui::Tool::Paint;
        }
        match code {
            // Material selection. These ids match the registry order in
            // `materials::builtins`.
            KeyCode::Digit1 => c.material = 1, // Sand
            KeyCode::Digit2 => c.material = 2, // Stone
            KeyCode::Digit3 => c.material = 3, // Water
            KeyCode::Digit4 => c.material = 4, // Lava
            KeyCode::Digit5 => c.material = 5, // Oil
            KeyCode::Digit6 => c.material = 6, // Fire
            KeyCode::Digit7 => c.material = 7, // Soil
            KeyCode::Digit8 => c.material = 8, // Wood
            KeyCode::Digit9 => c.material = 9, // Leaves
            KeyCode::Digit0 | KeyCode::Backspace => c.material = EMPTY, // Eraser
            // Wind tool: sweep the cursor to blow a gust.
            KeyCode::KeyW => c.tool = ui::Tool::Wind,
            // Meteor tool: click to call a meteor down on that spot.
            KeyCode::KeyM => c.tool = ui::Tool::Meteor,
            // Tsunami tool: click to send a wave rolling toward that spot.
            KeyCode::KeyT => c.tool = ui::Tool::Tsunami,
            // Gamma-ray-burst tool: click to annihilate that column from the sky.
            // (G is taken by world-generate, so the burst lives on Y.)
            KeyCode::KeyY => c.tool = ui::Tool::GammaBurst,
            // Creature tools: click to drop the chosen creature on that spot.
            KeyCode::KeyA => {
                c.entity = crate::entities::ANT;
                c.tool = ui::Tool::Creature;
            }
            KeyCode::KeyB => {
                c.entity = crate::entities::BIRD;
                c.tool = ui::Tool::Creature;
            }
            // Brush size.
            KeyCode::BracketLeft => c.brush = (c.brush - 1).max(0),
            KeyCode::BracketRight => c.brush = (c.brush + 1).min(40),
            // Clear the world.
            KeyCode::KeyC => {
                if let Some(state) = &mut self.state {
                    state.sim.clear();
                }
            }
            // World generation.
            KeyCode::KeyG => self.regenerate(), // (re)generate with current seed
            KeyCode::KeyR => self.randomize_seed(), // pick a new random seed + generate
            _ => {}
        }
    }

    /// Rebuild the world from the current seed and selected preset.
    fn regenerate(&mut self) {
        if let Some(state) = &mut self.state {
            let c = &self.input.controls;
            crate::worldgen::generate_world(&mut state.sim, c.seed_value() as i32, c.world);
        }
    }

    /// Roll a fresh seed and generate the world it describes.
    fn randomize_seed(&mut self) {
        if let Some(state) = &mut self.state {
            // Keep it within the seed box's digit cap so the value always fits.
            let seed = state.sim.rand_u32() % 10_000_000;
            self.input.controls.set_seed(seed);
            crate::worldgen::generate_world(&mut state.sim, seed as i32, self.input.controls.world);
        }
    }
}

/// Entry point shared by the desktop binary and the web (wasm) build.
pub fn run() {
    // Logging: env_logger on native, console_log + panic hook in the browser.
    #[cfg(not(target_arch = "wasm32"))]
    {
        env_logger::init();
    }
    #[cfg(target_arch = "wasm32")]
    {
        console_error_panic_hook::set_once();
        let _ = console_log::init_with_level(log::Level::Info);
    }

    // Auto-load any plugin materials sitting in ./plugins so they're in the
    // picker from the first frame. (No such folder is the normal case.)
    let loaded = crate::plugin::load_dir(std::path::Path::new("plugins"));
    if loaded > 0 {
        log::info!("loaded {loaded} plugin material(s) from ./plugins");
    }

    log::info!(
        "Controls: use the panel, or press 1=Sand 2=Stone 3=Water 4=Lava 5=Oil 6=Fire 7=Soil 8=Wood 9=Leaves  0/Backspace=Erase  W=wind tool (sweep to blow a gust)  M=meteor tool (click to summon)  T=tsunami (click to send a wave)  Y=gamma ray (click to annihilate a column)  A=ant B=bird (click to drop one)  [ ]=brush size  C=clear  G=generate world  R=random seed  (hold left mouse to draw)  — drag a .rhai file onto the window to add a material"
    );

    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .expect("build event loop");
    event_loop.set_control_flow(ControlFlow::Poll);

    let app = App::new(event_loop.create_proxy());

    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut app = app;
        event_loop.run_app(&mut app).expect("run event loop");
    }
    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::EventLoopExtWebSys;
        event_loop.spawn_app(app);
    }
}
