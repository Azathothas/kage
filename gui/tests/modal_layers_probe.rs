//! The shell's modal layers against the keyboard: the settings dialog
//! and its theme menu, and what the composer under them does with the
//! keys. Each test drives the real shell over the replay transport, so
//! the shell's key router runs before the layers draw, as it does in
//! the window.

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

/// A harness over the real shell over the replay transport.
fn harness() -> Harness<'static, Rig> {
    Harness::builder()
        .with_size(egui::Vec2::new(1280.0, 800.0))
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

/// Steps `n` frames. The replay pump asks for a repaint on every
/// event, so `run` would refuse; `step` drives the frames one at a
/// time.
fn steps(harness: &mut Harness<'static, Rig>, n: usize) {
    for _ in 0..n {
        harness.step();
    }
}

/// Steps until the composer holds the keyboard, then hands the
/// harness back.
fn booted(harness: &mut Harness<'static, Rig>) {
    for _ in 0..2_000 {
        harness.step();
        if composer_focused(harness) {
            return;
        }
    }
    panic!("the composer never took the keyboard");
}

/// Whether the composer's field holds the keyboard.
fn composer_focused(harness: &Harness<'static, Rig>) -> bool {
    harness
        .ctx
        .memory(|memory| memory.has_focus(egui::Id::new("composer-input")))
}

/// Opens the settings dialog with its shortcut.
fn open_settings(harness: &mut Harness<'static, Rig>) {
    harness.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::Comma);
    steps(harness, 3);
}

/// Whether the settings dialog shows.
fn settings_open(harness: &Harness<'static, Rig>) -> bool {
    testutil::count(harness, "Close") == 1
}

/// Opens the General page's dark-desktop theme menu.
fn open_theme_menu(harness: &mut Harness<'static, Rig>) {
    harness
        .query_all_by(|node| node.role() == egui::accesskit::Role::ComboBox)
        .next()
        .expect("the theme select draws")
        .click();
    steps(harness, 3);
}

/// Whether a theme menu shows: its rows repeat the theme cards' names,
/// so a name that reads twice is on a card and in the menu.
fn theme_menu_open(harness: &Harness<'static, Rig>) -> bool {
    testutil::count(harness, "Kimi dark") > 1
}

#[test]
fn escape_closes_the_theme_menu_before_the_dialog() {
    let mut harness = harness();
    booted(&mut harness);
    open_settings(&mut harness);
    open_theme_menu(&mut harness);
    assert!(theme_menu_open(&harness), "the menu opened");
    harness.key_press(egui::Key::Escape);
    steps(&mut harness, 3);
    assert!(!theme_menu_open(&harness), "Escape closes the menu");
    assert!(settings_open(&harness), "the dialog stays open");
    harness.key_press(egui::Key::Escape);
    steps(&mut harness, 3);
    assert!(
        !settings_open(&harness),
        "a second Escape closes the dialog"
    );
}

#[test]
fn a_theme_menu_does_not_outlive_the_dialog() {
    let mut harness = harness();
    booted(&mut harness);
    open_settings(&mut harness);
    open_theme_menu(&mut harness);
    for _ in 0..2 {
        harness.key_press(egui::Key::Escape);
        steps(&mut harness, 3);
    }
    assert!(!settings_open(&harness), "the dialog closed");
    open_settings(&mut harness);
    assert!(
        !theme_menu_open(&harness),
        "the dialog opens again without the menu it closed over"
    );
}
