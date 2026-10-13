//! The command palette: its rows draw and a chosen row comes back.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kage_desktop::store::Store;
use kage_desktop::ui::palette::{Palette, PaletteEntry};

mod testutil;

fn harness() -> Harness<'static, (Store, Palette, Option<PaletteEntry>, bool)> {
    Harness::builder()
        .with_size(egui::Vec2::new(1000.0, 700.0))
        .build_ui_state(
            |ui, (store, palette, chosen, installed): &mut (Store, Palette, Option<PaletteEntry>, bool)| {
                if testutil::theme_installed(installed, ui.ctx()) {
                    return;
                }
                palette.open();
                let rows = kage_desktop::ui::palette::RegistryOwned::of(store).view().rows(palette.query());
                *chosen = palette.show(ui, &rows).or(chosen.take());
            },
            (Store::new("/w", false), Palette::default(), None, false),
        )
}

#[test]
fn the_palette_lists_the_global_commands_without_a_session() {
    let mut harness = harness();
    harness.run();
    harness.run();
    for label in ["Settings", "Toggle sidebar", "Toggle workbench", "Change theme"] {
        assert!(
            harness.query_all_by_label(label).count() > 0,
            "the palette lists {label}"
        );
    }
}

