//! The shell chrome in the real window: the sidebar's own controls,
//! the narrow-width overlays and the palette over them, each driven
//! through the shipped `ShellApp` over the replay transport.

use egui_kittest::{Harness, kittest::Queryable};

mod testutil;

/// The state a shell harness carries between frames.
struct Rig {
    /// The real window body, built on the harness's own context the
    /// first frame it is drawn.
    app: Option<kage_desktop::launch::ShellApp>,
    /// Set once the theme has been installed on a frame that did not
    /// draw.
    installed: bool,
}

/// A harness over the real shell at `size`.
fn harness(size: egui::Vec2) -> Harness<'static, Rig> {
    Harness::builder().with_size(size).build_ui_state(
        |ui, rig: &mut Rig| {
            if !rig.installed {
                testutil::install_theme(ui.ctx(), "kage-shadow");
                rig.installed = true;
                return;
            }
            let app = rig.app.get_or_insert_with(|| {
                kage_desktop::launch::ShellApp::new(
                    std::env::temp_dir().to_string_lossy().into_owned(),
                    true,
                    Box::new(kage_desktop::transport::replay::ReplayTransport::new()),
                    ui.ctx(),
                )
            });
            eframe::App::ui(app, ui, &mut eframe::Frame::_new_kittest());
        },
        Rig {
            app: None,
            installed: false,
        },
    )
}

/// Steps until the composer's send button is on screen, then a few
/// frames more so the replayed session settles.
fn booted(harness: &mut Harness<'static, Rig>) {
    for _ in 0..2_000 {
        harness.step();
        if harness.query_by_label("Send").is_some() {
            steps(harness, 5);
            return;
        }
    }
    panic!("the composer never drew");
}

/// Steps `n` frames.
fn steps(harness: &mut Harness<'static, Rig>, n: usize) {
    for _ in 0..n {
        harness.step();
    }
}

/// Whether some node carries `label`.
fn shows(harness: &Harness<'static, Rig>, label: &str) -> bool {
    harness.query_by_label(label).is_some()
}

#[test]
fn the_sidebars_collapse_button_hides_it() {
    let mut harness = harness(egui::vec2(1200.0, 800.0));
    booted(&mut harness);
    assert!(shows(&harness, "Search"), "the sidebar draws while wide");
    harness.get_by_label("Collapse sidebar (Ctrl \\)").click();
    steps(&mut harness, 3);
    assert!(!shows(&harness, "Search"), "the click hides the sidebar");
}
