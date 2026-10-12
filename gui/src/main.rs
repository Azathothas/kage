//! The native binary of the kage client. The application lives in the
//! library; the browser build has no binary, its entry is the wasm
//! glue in `web.rs`.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    kage_desktop::launch::run();
}
