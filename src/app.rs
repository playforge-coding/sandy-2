//! Windowing, input, and the event loop — the glue between winit, the GPU
//! state, and the simulation. Identical code path on desktop and web; only the
//! async device-init handoff differs (see `resumed`).

use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::gpu::State;
use crate::materials::{MaterialId, EMPTY};

/// Delivered once the (async) GPU state has finished initialising. On the web
/// the device request can't block, so it's built off-thread and handed back
/// through the event loop.
#[allow(dead_code)] // constructed only on the web (async-init handoff).
pub enum UserEvent {
    StateReady(State),
}

/// Current painting state driven by keyboard/mouse.
struct Input {
    cursor: (f64, f64),
    drawing: bool,
    material: MaterialId,
    brush: i32,
    /// The world seed the next `G`/regenerate uses, and is shown in the UI.
    seed: u32,
    /// While true, digit keys type into the seed (instead of selecting a
    /// material); `Enter` commits and regenerates, `Esc` cancels.
    editing_seed: bool,
}

/// Longest seed the user can type — keeps the readout inside its panel and the
/// value comfortably within `u32`.
const MAX_SEED_DIGITS: usize = 7;

impl Default for Input {
    fn default() -> Self {
        Self {
            cursor: (0.0, 0.0),
            drawing: false,
            material: 1, // Sand
            brush: 4,
            seed: crate::worldgen::DEFAULT_SEED as u32,
            editing_seed: false,
        }
    }
}

struct App {
    // Only consumed on the web (the async-init handoff); unused on native.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    proxy: EventLoopProxy<UserEvent>,
    state: Option<State>,
    input: Input,
}

impl App {
    fn new(proxy: EventLoopProxy<UserEvent>) -> Self {
        Self {
            proxy,
            state: None,
            input: Input::default(),
        }
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
        state.window().request_redraw();
        self.state = Some(state);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
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
                    self.input.material = id;
                }
                Err(e) => log::error!("failed to load plugin {path:?}: {e}"),
            },

            WindowEvent::MouseInput {
                state: btn,
                button: MouseButton::Left,
                ..
            } => {
                if btn == ElementState::Pressed {
                    // A press on the picker selects a material; anywhere else
                    // starts painting.
                    let cursor = self.input.cursor;
                    match self.state.as_ref().and_then(|s| s.picker_at(cursor)) {
                        Some(material) => self.input.material = material,
                        None => self.input.drawing = true,
                    }
                } else {
                    self.input.drawing = false;
                }
            }

            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    if let PhysicalKey::Code(code) = event.physical_key {
                        self.handle_key(code);
                    }
                }
            }

            WindowEvent::RedrawRequested => {
                if let Some(state) = &mut self.state {
                    // Paint under the cursor while the mouse is held.
                    if self.input.drawing {
                        let (gx, gy) = state.cursor_to_grid(self.input.cursor);
                        state
                            .sim
                            .paint_disk(gx, gy, self.input.brush, self.input.material);
                    }
                    state.update(self.input.material, self.input.seed, self.input.editing_seed);
                    state.render();
                }
            }

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
    fn handle_key(&mut self, code: KeyCode) {
        // While typing a seed, digit keys feed the seed field rather than
        // selecting a material — so handle that mode separately.
        if self.input.editing_seed {
            self.handle_seed_key(code);
            return;
        }
        match code {
            // Material selection. These ids match the registry order in
            // `materials::builtins`.
            KeyCode::Digit1 => self.input.material = 1, // Sand
            KeyCode::Digit2 => self.input.material = 2, // Stone
            KeyCode::Digit3 => self.input.material = 3, // Water
            KeyCode::Digit4 => self.input.material = 4, // Lava
            KeyCode::Digit5 => self.input.material = 5, // Oil
            KeyCode::Digit6 => self.input.material = 6, // Fire
            KeyCode::Digit7 => self.input.material = 7, // Soil
            KeyCode::Digit8 => self.input.material = 8, // Wood
            KeyCode::Digit9 => self.input.material = 9, // Leaves
            KeyCode::Digit0 | KeyCode::Backspace => self.input.material = EMPTY, // Eraser
            // Brush size.
            KeyCode::BracketLeft => self.input.brush = (self.input.brush - 1).max(0),
            KeyCode::BracketRight => self.input.brush = (self.input.brush + 1).min(40),
            // Clear the world.
            KeyCode::KeyC => {
                if let Some(state) = &mut self.state {
                    state.sim.clear();
                }
            }
            // World generation.
            KeyCode::KeyG => self.regenerate(),        // (re)generate with current seed
            KeyCode::KeyR => self.randomize_seed(),    // pick a new random seed + generate
            KeyCode::KeyS => self.input.editing_seed = true, // start typing a seed
            _ => {}
        }
    }

    /// Key handling while the seed field is being edited.
    fn handle_seed_key(&mut self, code: KeyCode) {
        if let Some(d) = digit_of(code) {
            // Build up the seed digit by digit, capped so it stays in-panel.
            let mut s = self.input.seed.to_string();
            if s == "0" {
                s.clear(); // don't keep a leading zero
            }
            if s.len() < MAX_SEED_DIGITS {
                s.push(char::from(b'0' + d as u8));
                self.input.seed = s.parse().unwrap_or(0);
            }
            return;
        }
        match code {
            // Delete the last digit.
            KeyCode::Backspace => {
                self.input.seed /= 10;
            }
            // Commit the seed and build the world.
            KeyCode::Enter | KeyCode::NumpadEnter => {
                self.input.editing_seed = false;
                self.regenerate();
            }
            // Abandon editing, leaving the seed as it was.
            KeyCode::Escape => self.input.editing_seed = false,
            _ => {}
        }
    }

    /// Rebuild the world from the current seed.
    fn regenerate(&mut self) {
        if let Some(state) = &mut self.state {
            crate::worldgen::generate(&mut state.sim, self.input.seed as i32);
        }
    }

    /// Roll a fresh seed and generate the world it describes.
    fn randomize_seed(&mut self) {
        if let Some(state) = &mut self.state {
            // Keep it within MAX_SEED_DIGITS so the readout always fits.
            let seed = state.sim.rand_u32() % 10_000_000;
            self.input.seed = seed;
            crate::worldgen::generate(&mut state.sim, seed as i32);
        }
    }
}

/// Map a digit `KeyCode` (top-row or numpad) to its value `0..=9`.
fn digit_of(code: KeyCode) -> Option<u32> {
    use KeyCode::*;
    Some(match code {
        Digit0 | Numpad0 => 0,
        Digit1 | Numpad1 => 1,
        Digit2 | Numpad2 => 2,
        Digit3 | Numpad3 => 3,
        Digit4 | Numpad4 => 4,
        Digit5 | Numpad5 => 5,
        Digit6 | Numpad6 => 6,
        Digit7 | Numpad7 => 7,
        Digit8 | Numpad8 => 8,
        Digit9 | Numpad9 => 9,
        _ => return None,
    })
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
        "Controls: click the picker (top-left) or 1=Sand 2=Stone 3=Water 4=Lava 5=Oil 6=Fire 7=Soil 8=Wood 9=Leaves  0/Backspace=Erase  [ ]=brush size  C=clear  G=generate world  R=random seed  S=type a seed (digits, Enter to apply, Esc to cancel)  (hold left mouse to draw)  — drag a .rhai file onto the window to add a material"
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
