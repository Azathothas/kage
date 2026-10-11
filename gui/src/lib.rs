//! The kage client: one window over one engine link.
//!
//! The shell is being rebuilt on egui (specs/epics/, E01 onward); this
//! commit carries the framework-free spine: preferences, the launch
//! gate, clocks, timings, the paced text reveal and the transports
//! that front `kage-client`. The views return with their epics.

// The icon catalog and the brand art callers return with their views;
// until then the embeds are carried but unreferenced, and CI denies
// dead code.
#[allow(dead_code)]
mod assets;
pub mod clock;
pub mod gate;
#[allow(dead_code)]
pub mod icon;
#[allow(dead_code)]
pub mod icons;
pub mod launch;
mod modes;
pub mod prefs;
// The views that call the paced reveal return with E04; until then its
// API is carried but unreferenced, and CI denies dead code.
#[allow(dead_code)]
pub mod reveal;
#[allow(dead_code)]
pub mod store;
pub mod theme;
pub mod themes;
pub mod timing;
pub mod transport;
pub mod update;

/// Reports a problem where the platform can show it: stderr on the
/// desktop, the browser console on the web.
pub(crate) fn warn(message: &str) {
    #[cfg(target_arch = "wasm32")]
    web_sys::console::warn_1(&format!("kage-desktop: {message}").into());
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("kage-desktop: {message}");
}
