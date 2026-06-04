//! Desktop entry point. On the web the simulation is started from `lib.rs`'s
//! `wasm_start`, so this `main` is a no-op there.

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    sandy::run();
}
