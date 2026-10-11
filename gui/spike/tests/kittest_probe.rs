//! Headless kittest probe for the GPUI exit spike:
//! render-window checks, input dispatch, geometry assertions and a
//! paced streaming bench, all without a display server.

use std::time::Duration;

use egui_kittest::{Harness, kittest::Queryable};
use spike::{Options, ProbeApp};

fn harness_for(app: ProbeApp, size: [f32; 2]) -> Harness<'static, ProbeApp> {
    Harness::builder()
        .with_size(egui::Vec2::new(size[0], size[1]))
        .build_ui_state(
            |ui, app: &mut ProbeApp| {
                ui.ctx().set_visuals(egui::Visuals::dark());
                app.show(ui);
            },
            app,
        )
}

/// Q1a: with 5,000 items loaded, one frame must lay out only the visible
/// window (plus overscan), never the whole list.
#[test]
fn list_renders_only_visible_window() {
    let app = ProbeApp::new(Options {
        rows: 5000,
        ..Default::default()
    });
    let mut harness = harness_for(app, [900.0, 600.0]);
    harness.run();
    harness.run();
    let parses = harness.state().parses_last_frame();
    assert!(
        parses < 100,
        "laid out {parses} of 5000 items in a single frame"
    );
    harness.root().scroll_down();
    harness.run();
    let parses = harness.state().parses_last_frame();
    assert!(parses < 100, "after scroll laid out {parses} items");
}

/// Q1b: paced 30 Hz streaming over the 5,000-row list completes every
/// tick headless and keeps the render window small.
#[test]
fn headless_stream_bench_5s() {
    let app = ProbeApp::new(Options {
        rows: 5000,
        stream: true,
        bench_secs: 5,
        ..Default::default()
    });
    let mut harness = harness_for(app, [1100.0, 800.0]);
    let start = std::time::Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(16));
        // run() would panic: the stream requests a repaint every frame,
        // and run() only tolerates max_steps (4) consecutive demands.
        harness.run_steps(1);
        if harness.state().bench_done() || start.elapsed() > Duration::from_secs(30) {
            break;
        }
    }
    assert!(harness.state().bench_done(), "bench did not finish in 30s");
    eprintln!("[headless] {}", harness.state().summary());
    let parses = harness.state().parses_last_frame();
    assert!(parses < 100, "streaming laid out {parses} items per frame");
}

/// Q2a: geometry assertions work through the accesskit tree (stand-in
/// for the composer-column test).
#[test]
fn sidebar_panel_takes_270px() {
    let app = ProbeApp::new(Options::default());
    let mut harness = Harness::builder()
        .with_size(egui::Vec2::new(900.0, 600.0))
        .build_ui_state(
            |ui, app: &mut ProbeApp| {
                ui.ctx().set_visuals(egui::Visuals::dark());
                egui::Panel::left("sidebar")
                    .exact_size(270.0)
                    .resizable(false)
                    .show_inside(ui, |ui| {
                        ui.label("sidebar");
                        let body = ui.allocate_response(ui.available_size(), egui::Sense::hover());
                        body.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "sidebar-body")
                        });
                    });
                egui::CentralPanel::default().show(ui, |ui| {
                    let body = ui.allocate_response(ui.available_size(), egui::Sense::hover());
                    body.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "central-body")
                    });
                    app.show(ui);
                });
            },
            app,
        );
    harness.run();
    harness.run();
    let rect = harness.get_by_label("sidebar-body").rect();
    assert!(
        (rect.width() - 262.0).abs() < 12.0,
        "sidebar body width {} not ~= panel 270 minus margins",
        rect.width()
    );
    let central = harness.get_by_label("central-body").rect();
    assert!(
        central.width() > 590.0 && central.width() < 640.0,
        "central body width {} outside expected range",
        central.width()
    );
}

/// Q2b: synthesized click and text input land (stand-in for input
/// dispatch tests).
#[test]
fn input_dispatch_click_and_type() {
    struct State {
        count: u32,
        composer: String,
    }
    let mut harness = Harness::builder()
        .with_size(egui::Vec2::new(600.0, 400.0))
        .build_ui_state(
            |ui, st: &mut State| {
                ui.ctx().set_visuals(egui::Visuals::dark());
                ui.label(format!("count: {}", st.count));
                if ui.button("ping").clicked() {
                    st.count += 1;
                }
                egui::TextEdit::singleline(&mut st.composer)
                    .hint_text("composer")
                    .show(ui)
                    .response
                    .widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "composer")
                    });
            },
            State {
                count: 0,
                composer: String::new(),
            },
        );
    harness.get_by_label("ping").click();
    harness.run();
    harness.get_by_label("count: 1"); // panics if the click did not land
    let composer = harness.get_by_label("composer");
    composer.click();
    composer.type_text("hello");
    harness.run();
    assert_eq!(harness.state().composer, "hello");
}

/// Q2c: pixel snapshots work headless (theme-regression mechanics).
#[test]
fn snapshot_dark_shell() {
    let app = ProbeApp::new(Options::default());
    let mut harness = harness_for(app, [1100.0, 800.0]);
    harness.run();
    harness.run();
    harness.snapshot("spike-shell-dark");
}
