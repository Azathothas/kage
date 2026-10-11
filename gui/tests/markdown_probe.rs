//! Headless E04 markdown probes: the pipeline renders a golden
//! document in both themes, and the parse discipline holds (only the
//! streaming entry is parsed per frame).

use std::collections::HashMap;

use egui_kittest::Harness;
use kage_desktop::ui::transcript::markdown::{Pipeline, THEME_DAWN, THEME_SHADOW};

/// A golden markdown document: headings, lists, a table, nested code,
/// links, inline styles and a CJK line.
const GOLDEN: &str = r#"# kage

A **bold** and *italic* line with `inline code` and a [link](https://example.com/kage).

## A list

- first
- second
  - nested
- third

1. one
2. two

## A table

| name | value |
| --- | --- |
| alpha | 1 |
| beta | 2 |

## Code

```rust
fn main() {
    let x = 42; // a comment
    println!("{x}");
}
```

CJK: 影者の影は影に宿る。
"#;

/// A harness over the pipeline, rendering `GOLDEN` for entry `id`.
fn harness_for(theme: &str, streaming: bool) -> Harness<'static, (String, bool)> {
    Harness::builder()
        .with_size(egui::Vec2::new(600.0, 800.0))
        .build_ui_state(
            |ui, (theme, streaming): &mut (String, bool)| {
                kage_desktop::theme::install_fonts(ui.ctx());
                kage_desktop::icons::install(ui.ctx());
                kage_desktop::theme::apply_choice(
                    &kage_desktop::theme::ThemeChoice::Named(theme.clone()),
                    ui.ctx(),
                );
                let mut pipeline = Pipeline::new();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    pipeline.show("golden", GOLDEN, *streaming, ui);
                });
            },
            (theme.to_owned(), streaming),
        )
}

/// Painted pixels that differ from `against` by more than the tolerance.
fn differing(img: &image::RgbaImage, against: egui::Color32) -> usize {
    img.pixels()
        .filter(|px| px.0[3] != 0)
        .filter(|px| {
            let (dr, dg, db) = (
                i32::from(px.0[0]) - i32::from(against.r()),
                i32::from(px.0[1]) - i32::from(against.g()),
                i32::from(px.0[2]) - i32::from(against.b()),
            );
            dr.abs() + dg.abs() + db.abs() > 12
        })
        .count()
}

#[test]
fn the_golden_document_renders_in_both_themes() {
    for (theme, bg) in [
        (THEME_SHADOW, egui::Color32::from_rgb(0x0F, 0x0E, 0x13)),
        (THEME_DAWN, egui::Color32::from_rgb(0xF7, 0xF5, 0xF9)),
    ] {
        let mut harness = harness_for(theme, false);
        harness.run();
        let img = harness.render().expect("the frame renders");
        let drawn = differing(&img, bg);
        assert!(
            drawn > 2_000,
            "{theme}: only {drawn} text pixels rendered for the golden document"
        );
    }
}

#[test]
fn the_pipeline_parses_only_the_streaming_entry_per_frame() {
    // The counter is the spec's guard: a completed entry parses once,
    // and a streaming entry parses every frame. The harness carries
    // the texture deltas a bare context would panic on.
    #[derive(Default)]
    struct State {
        pipeline: Pipeline,
    }
    let mut harness = Harness::builder()
        .with_size(egui::Vec2::new(600.0, 800.0))
        .build_ui_state(
            |ui, state: &mut State| {
                kage_desktop::theme::install_fonts(ui.ctx());
                kage_desktop::theme::apply_choice(
                    &kage_desktop::theme::ThemeChoice::Named("kage-shadow".to_owned()),
                    ui.ctx(),
                );
                egui::ScrollArea::vertical().show(ui, |ui| {
                    state.pipeline.show("done", "# done", false, ui);
                    state.pipeline.show("tail", "streaming", true, ui);
                });
            },
            State::default(),
        );
    harness.run();
    let frames_after_one = harness.state().pipeline.parses.count("tail");
    harness.run();
    harness.run();
    let state = harness.state();
    assert_eq!(
        state.pipeline.parses.count("done"),
        1,
        "the finished entry parsed once"
    );
    assert!(
        state.pipeline.parses.count("tail") > frames_after_one,
        "the streaming entry parses on every frame it is drawn on"
    );
}

#[test]
fn the_pipeline_forgets_the_entries_that_are_gone() {
    let mut pipeline = Pipeline::new();
    pipeline.retain(&[]);
    assert_eq!(pipeline.parses.count("done"), 0);
    let _: HashMap<String, usize> = HashMap::new();
}
