//! Headless E03 vim probes: the real shell answers the normal-mode
//! keys, the modeline names the mode and the cursor, the `:` line
//! opens on the colon and runs one command, and with the mode off no
//! key reaches any of it.

use egui_kittest::Harness;

mod testutil;

/// The state a vim harness carries between frames.
struct Rig {
    /// The real window body, built on the harness's own context the
    /// first frame it is drawn, so the fonts it installs are the ones
    /// the harness renders with.
    app: Option<kage_desktop::launch::ShellApp>,
    /// Set once the theme has been installed on a frame that did not
    /// draw.
    installed: bool,
}

/// A harness over the real shell over the replay transport, with vim
/// mode already on.
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
                    let mut app = kage_desktop::launch::ShellApp::new(
                        std::env::temp_dir().to_string_lossy().into_owned(),
                        true,
                        Box::new(kage_desktop::transport::replay::ReplayTransport::new()),
                        ui.ctx(),
                    );
                    app.shell.set_vim(true);
                    app
                });
                eframe::App::ui(app, ui, &mut eframe::Frame::_new_kittest());
            },
            Rig {
                app: None,
                installed: false,
            },
        )
}

/// Steps the harness until the recording's rows are on screen, then
/// hands the app back. The steps do not wait for a stable frame: the
/// replay pump asks for a repaint on every event, so `run` would
/// refuse.
fn loaded<'h>(harness: &'h mut Harness<'static, Rig>) -> &'h mut kage_desktop::launch::ShellApp {
    for _ in 0..4_000 {
        harness.step();
        if harness
            .state_mut()
            .app
            .as_ref()
            .is_some_and(|app| app.shell.transcript().row_count() > 5)
        {
            break;
        }
    }
    harness
        .state_mut()
        .app
        .as_mut()
        .expect("the shell built on the second frame")
}

/// Leaves the composer for normal mode, the way the user's Escape
/// does, so the keys that follow reach the vim layer.
fn leave_insert(harness: &mut Harness<'static, Rig>) {
    harness.event(egui::Event::Key {
        key: egui::Key::Escape,
        modifiers: egui::Modifiers::NONE,
        pressed: true,
        repeat: false,
        physical_key: None,
    });
    stepped(harness, 1);
}

fn stepped<'h>(
    harness: &'h mut Harness<'static, Rig>,
    steps: usize,
) -> &'h mut kage_desktop::launch::ShellApp {
    for _ in 0..steps {
        harness.step();
    }
    harness
        .state_mut()
        .app
        .as_mut()
        .expect("the shell built on the second frame")
}

#[test]
fn the_modeline_names_the_mode_and_the_keys_move_the_cursor() {
    use egui_kittest::kittest::Queryable;
    let mut harness = harness();
    let app = loaded(&mut harness);
    let total = app.shell.transcript().row_count();
    assert!(total > 5, "the recording opened rows: {total}");
    leave_insert(&mut harness);

    // With the mode on the modeline draws and says normal mode.
    assert!(harness.query_by_label("-- NORMAL --").is_some());

    // The cursor starts absent; one `j` puts it on the first row.
    let app = stepped(&mut harness, 1);
    assert_eq!(app.shell.transcript().cursor_row(), None);
    harness.event(egui::Event::Text("j".to_owned()));
    let app = stepped(&mut harness, 2);
    assert_eq!(
        app.shell.transcript().cursor_row(),
        Some(0),
        "j starts at the top"
    );

    // A `g g` sequence keeps the top, and shift-g jumps to the last.
    harness.event(egui::Event::Text("g".to_owned()));
    stepped(&mut harness, 1);
    harness.event(egui::Event::Text("g".to_owned()));
    stepped(&mut harness, 2);
    let app = stepped(&mut harness, 1);
    assert_eq!(
        app.shell.transcript().cursor_row(),
        Some(0),
        "g g stays at the top"
    );

    harness.event(egui::Event::Text("G".to_owned()));
    stepped(&mut harness, 2);
    let app = stepped(&mut harness, 1);
    assert_eq!(
        app.shell.transcript().cursor_row(),
        Some(total - 1),
        "shift-g jumps to the last row"
    );
}

#[test]
fn the_colon_opens_the_line_and_runs_its_command() {
    let mut harness = harness();
    let _ = loaded(&mut harness);
    leave_insert(&mut harness);

    // The `:` line opens focused.
    harness.event(egui::Event::Text(":".to_owned()));
    let app = stepped(&mut harness, 2);
    assert!(
        app.shell.vim_line_open(),
        "the colon opened the command line"
    );

    // The line runs what it holds: `vim off` turns the mode off, and
    // the keys go back to the shell's own table.
    for c in ["v", "i", "m", " ", "o", "f", "f"] {
        harness.event(egui::Event::Text(c.to_owned()));
        stepped(&mut harness, 1);
    }
    stepped(&mut harness, 1);
    harness.event(egui::Event::Key {
        key: egui::Key::Enter,
        modifiers: egui::Modifiers::NONE,
        pressed: true,
        repeat: false,
        physical_key: None,
    });
    stepped(&mut harness, 2);
    let app = stepped(&mut harness, 1);
    eprintln!(
        "MARK entered buffer={:?} open={} vim={}",
        app.shell.vim_line_text(),
        app.shell.vim_line_open(),
        app.shell.store().prefs().vim
    );
    assert!(!app.shell.store().prefs().vim, "the line ran vim off");
    assert!(!app.shell.vim_line_open(), "Enter closed the line");
}
