//! The palette in the real shell: the sidebar's Search row opens it
//! and the opening click does not close it. The palette draws from a
//! clone the shell writes back at the end of the frame, so an open
//! raised while the frame runs has to land in that clone, or the
//! palette flashes for one frame and vanishes.

use egui_kittest::{Harness, kittest::Queryable};

mod testutil;

/// The state a shell harness carries between frames.
struct Rig {
    /// The real window body, built on the harness's own context the
    /// first frame it is drawn, so the fonts it installs are the ones
    /// the harness renders with.
    app: Option<kage_desktop::launch::ShellApp>,
    /// Set once the theme has been installed on a frame that did not
    /// draw.
    installed: bool,
}

/// A harness over the real shell over the replay transport.
fn harness() -> Harness<'static, Rig> {
    Harness::builder()
        .with_size(egui::Vec2::new(1000.0, 760.0))
        .build_ui_state(
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

/// Steps until the sidebar's Search row is on screen, then hands the
/// harness back. The replay pump asks for a repaint on every event,
/// so `run` would refuse; `step` drives the frames one at a time.
fn booted(harness: &mut Harness<'static, Rig>) {
    for _ in 0..2_000 {
        harness.step();
        if harness.query_by_label("Search").is_some() {
            return;
        }
    }
    panic!("the sidebar's Search row never drew");
}

#[test]
fn clicking_search_opens_the_palette_and_it_stays_open() {
    let mut harness = harness();
    booted(&mut harness);

    // The click opens the palette; its rows draw the frame after.
    harness
        .query_by_label("Search")
        .expect("Search row")
        .click();
    harness.step();
    harness.step();
    assert!(
        harness.query_by_label("Settings").is_some(),
        "the click on Search opens the palette"
    );

    // The clone write-back at the end of the opening frame must not
    // close it again.
    harness.step();
    harness.step();
    harness.step();
    assert!(
        harness.query_by_label("Settings").is_some(),
        "the palette stays open after the frame that opened it"
    );

    // A click on the card's dead space must not close it: the card
    // answers its own clicks before the backdrop below. The replay
    // streams the palette's rows in over the first frames, so the
    // hints wait for the card to settle before aiming; the point is
    // in the hint row, left of every hint widget, where only the
    // card can answer.
    let close_word = {
        let mut held: Option<egui::Rect> = None;
        let mut still = 0;
        let mut found: Option<egui::Rect> = None;
        for _ in 0..2_000 {
            harness.step();
            let rect = harness.query_by_label("close").map(|node| node.rect());
            if let Some(rect) = rect {
                if held == Some(rect) {
                    still += 1;
                    if still == 8 {
                        found = Some(rect);
                        break;
                    }
                } else {
                    still = 0;
                }
                held = Some(rect);
            }
        }
        found.expect("the palette's hint row never drew")
    };
    let dead = egui::pos2(close_word.left() - 60.0, close_word.center().y);
    harness.hover_at(dead);
    harness.drag_at(dead);
    harness.drop_at(dead);
    harness.step();
    harness.step();
    assert!(
        harness.query_by_label("Settings").is_some(),
        "a click on the card's dead space keeps the palette open"
    );

    // A click on the backdrop, outside the card, still closes it.
    let outside = egui::pos2(10.0, 700.0);
    harness.hover_at(outside);
    harness.drag_at(outside);
    harness.drop_at(outside);
    harness.step();
    harness.step();
    assert!(
        harness.query_by_label("Settings").is_none(),
        "a click outside the card closes the palette"
    );
}
