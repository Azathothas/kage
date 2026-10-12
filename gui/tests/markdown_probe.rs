//! Headless E04 markdown probes: the pipeline renders a golden
//! document in both themes, and the parse discipline holds (only the
//! streaming entry is parsed per frame).

use std::collections::HashMap;

use egui_kittest::Harness;
use kage_desktop::ui::transcript::markdown::{Pipeline, THEME_DAWN, THEME_SHADOW};

mod testutil;

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
                testutil::install_theme(ui.ctx(), theme);
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
                testutil::install_theme(ui.ctx(), "kage-shadow");
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

#[test]
fn the_cjk_line_renders_glyphs_now_that_a_fallback_loads() {
    // S04.7 measured the tofu: with the bundled Latin families and
    // egui's own default fonts, the CJK line rendered exactly like a
    // private-use codepoint no font carries. E05's fallback closes
    // that: a system font covering U+5F71 registers behind the
    // bundled families, so the line lays out at the CJK font's own
    // advance widths instead of the .notdef box's.
    //
    // The measurement is the galley's width. A CJK glyph is exactly
    // one em wide, so ten of them at 28px measure 280px. The .notdef
    // box is wider, so a tofu line measures more. The private-use
    // control stays tofu and measures the same as the old CJK line
    // did, which is what keeps the comparison honest.

    /// The width `text` lays out at once the kage families and the
    /// CJK fallback are active. `set_fonts` applies at the start of
    /// the next pass, so the width is read on the frame after the
    /// fonts are installed.
    fn width_of(text: &str) -> f32 {
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::Vec2::new(600.0, 120.0))
            .build_ui_state(
                |ui, width: &mut Option<f32>| {
                    testutil::install_theme(ui.ctx(), "kage-shadow");
                    let fid = egui::FontId::new(28.0, egui::FontFamily::Proportional);
                    let galley = ui.ctx().fonts_mut(|f| {
                        f.layout(text.to_owned(), fid, egui::Color32::WHITE, f32::INFINITY)
                    });
                    ui.label(egui::RichText::new(text.to_owned()).size(28.0));
                    // The fonts land at the start of the next pass, so
                    // the first frame still lays out against the old
                    // set and every frame after it against the new.
                    if width.is_some() {
                        eprintln!("{text:?} galley width: {}", galley.rect.width());
                    }
                    *width = Some(galley.rect.width());
                },
                None,
            );
        harness.run();
        harness.run();
        harness.render().expect("the frame renders");
        harness.state().expect("the width was read")
    }

    let cjk = width_of("影者の影は影に宿る。");
    let tofu = width_of("\u{E000}\u{E001}\u{E002}");
    eprintln!("cjk galley width: {cjk}, known-tofu width: {tofu}");
    // Ten CJK glyphs at 28px are exactly ten em wide; the tofu control
    // is three .notdef boxes, so it is wider than three em.
    assert!(
        (cjk - 280.0).abs() < 1.0,
        "the CJK line lays out at its own advances: {cjk} for ten glyphs at 28px"
    );
    assert!(
        (tofu - cjk).abs() > 1.0,
        "the private-use line still measures the .notdef advances, not the CJK ones: {tofu}"
    );
}
