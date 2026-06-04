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
use crate::materials::{EMPTY, MaterialId};

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
}

impl Default for Input {
    fn default() -> Self {
        Self {
            cursor: (0.0, 0.0),
            drawing: false,
            material: 1, // Sand
            brush: 4,
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

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
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

            WindowEvent::MouseInput { state: btn, button: MouseButton::Left, .. } => {
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
                    state.update(self.input.material);
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
        match code {
            // Material selection. These ids match `Materials::default_set`.
            KeyCode::Digit1 => self.input.material = 1, // Sand
            KeyCode::Digit2 => self.input.material = 2, // Stone
            KeyCode::Digit3 => self.input.material = 3, // Water
            KeyCode::Digit4 => self.input.material = 4, // Lava
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
            _ => {}
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

    log::info!(
        "Controls: click the picker (top-left) or 1=Sand  2=Stone  3=Water  4=Lava  0/Backspace=Erase  [ ]=brush size  C=clear  (hold left mouse to draw)"
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
