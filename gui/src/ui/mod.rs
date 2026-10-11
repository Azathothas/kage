//! The shell's view modules: the chrome the shell mounts over the
//! panels.
//!
//! Each module is a plain state machine plus a `show` that draws it
//! with egui widgets, so the logic tests without a window and the
//! shell only wires keys and store data in.

pub mod composer;
pub mod copy;
pub mod dock;
pub mod eclipse;
pub mod find;
pub mod keys;
pub mod palette;
pub mod pickers;
pub mod shell;
pub mod sidebar;
pub mod splash;
pub mod status;
pub mod toasts;
pub mod toolbar;
pub mod transcript;
