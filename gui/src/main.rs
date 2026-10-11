//! The native binary of the kage client. The application lives in the
//! library; the egui shell arrives with S01.2 and the window wiring
//! with S01.5.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    kage_desktop::launch::run();
}
