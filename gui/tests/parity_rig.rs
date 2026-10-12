//! The visual parity rig: kittest snapshot baselines of the themed
//! shell at the SPIKE viewport, plus the dominant-color histograms
//! against the palette.
//!
//! The snapshots are the reference the later view epics re-baseline
//! against; the histograms are what a human eyeball used to be for
//! "does the shell still draw the palette". Both themes are checked,
//! because a light palette drift is invisible in the dark one.

use std::collections::HashMap;

use egui_kittest::{Harness, kittest::Queryable};
use kage_desktop::ui::shell::Panels;

mod testutil;

/// The SPIKE's viewport, the parity rig's reference size.
const SPIKE_VIEWPORT: [f32; 2] = [1252.0, 1317.0];

/// A harness over the full shell at the SPIKE viewport, in the theme
/// `name`, with the workbench open so all three panels are covered.
fn harness_for(theme: &str, workbench: bool) -> Harness<'static, (String, bool)> {
    let mut panels = Panels::default();
    if workbench {
        panels.toggle_workbench();
    }
    Harness::builder()
        .with_size(egui::Vec2::new(SPIKE_VIEWPORT[0], SPIKE_VIEWPORT[1]))
        .build_ui_state(
            |ui, (theme, workbench): &mut (String, bool)| {
                testutil::install_theme(ui.ctx(), theme);
                kage_desktop::ui::shell::show(
                    ui,
                    &mut panels_for(*workbench),
                    |ui| {
                        let palette = kage_desktop::theme::Palette::active_of(ui.ctx());
                        ui.colored_label(palette.accent, "sidebar");
                    },
                    |ui| {
                        let palette = kage_desktop::theme::Palette::active_of(ui.ctx());
                        ui.colored_label(palette.ink_strong, "chat");
                    },
                    |ui| {
                        let palette = kage_desktop::theme::Palette::active_of(ui.ctx());
                        ui.colored_label(palette.info, "workbench");
                    },
                );
                kage_desktop::ui::status::status_bar(ui, store());
            },
            (theme.to_owned(), workbench),
        )
}

/// The panels a harness frame draws with, rebuilt each frame so the
/// state stays outside the closure.
fn panels_for(workbench: bool) -> Panels {
    let mut panels = Panels::default();
    if workbench {
        panels.toggle_workbench();
    }
    panels
}

/// A store with no connection, for the status bar's labels.
fn store() -> &'static kage_desktop::store::Store {
    static STORE: std::sync::OnceLock<kage_desktop::store::Store> = std::sync::OnceLock::new();
    STORE.get_or_init(|| kage_desktop::store::Store::new("/w", false))
}

/// The painted color histogram.
fn histogram(img: &image::RgbaImage) -> HashMap<egui::Color32, usize> {
    let mut counts: HashMap<egui::Color32, usize> = HashMap::new();
    for px in img.pixels() {
        if px.0[3] == 0 {
            continue;
        }
        let c = egui::Color32::from_rgba_unmultiplied(px.0[0], px.0[1], px.0[2], px.0[3]);
        *counts.entry(c).or_default() += 1;
    }
    counts
}

/// The painted pixel count under `colors`, as a share of the painted
/// frame. The shell paints its surfaces with the palette's own tokens,
/// so a drift away from them shows here.
fn palette_share(img: &image::RgbaImage, colors: &[egui::Color32]) -> f32 {
    let counts = histogram(img);
    let painted: usize = counts.values().sum();
    let hits: usize = colors
        .iter()
        .map(|c| counts.get(c).copied().unwrap_or(0))
        .sum();
    hits as f32 / painted as f32
}

#[test]
fn the_shadow_shell_histograms_against_the_palette() {
    let mut harness = harness_for("kage-shadow", true);
    harness.run();
    let img = harness.render().expect("the frame renders");
    // The shell's three surfaces: the chat's app background, the
    // sidebar's `sidebar` token and the workbench's `well` token.
    let share = palette_share(
        &img,
        &[
            egui::Color32::from_rgb(0x0F, 0x0E, 0x13),
            egui::Color32::from_rgb(0x0B, 0x0A, 0x0E),
            egui::Color32::from_rgb(0x17, 0x15, 0x1D),
        ],
    );
    assert!(
        share > 0.9,
        "the palette surfaces hold {share} of the frame"
    );
    // All three panels drew.
    assert!(harness.query_by_label("sidebar").is_some());
    assert!(harness.query_by_label("chat").is_some());
    assert!(harness.query_by_label("workbench").is_some());
}

#[test]
fn the_dawn_shell_histograms_against_the_palette() {
    let mut harness = harness_for("kage-dawn", true);
    harness.run();
    let img = harness.render().expect("the frame renders");
    let share = palette_share(
        &img,
        &[
            egui::Color32::from_rgb(0xF7, 0xF5, 0xF9),
            egui::Color32::from_rgb(0xEE, 0xEB, 0xF2),
            egui::Color32::from_rgb(0xEF, 0xEC, 0xF3),
        ],
    );
    assert!(
        share > 0.9,
        "the palette surfaces hold {share} of the frame"
    );
    assert!(harness.query_by_label("sidebar").is_some());
    assert!(harness.query_by_label("chat").is_some());
    assert!(harness.query_by_label("workbench").is_some());
}

/// The snapshot baselines. Re-baseline with
/// `KITTEST_SNAPSHOT=overwrite cargo test --test parity_rig`.
#[test]
fn the_shadow_shell_snapshot_baseline() {
    let mut harness = harness_for("kage-shadow", true);
    harness.run();
    harness.snapshot("shadow-shell");
}

/// The snapshot baseline for the light palette.
#[test]
fn the_dawn_shell_snapshot_baseline() {
    let mut harness = harness_for("kage-dawn", true);
    harness.run();
    harness.snapshot("dawn-shell");
}
