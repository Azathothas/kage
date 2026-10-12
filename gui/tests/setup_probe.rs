//! Headless S07.5 probes: the setup screen. The screen replaces the
//! whole shell while no engine can start, so each test drives the
//! screen's own controls and reads what it asked the shell to do.

use egui_kittest::{Harness, kittest::Queryable};
use kage_desktop::ui::setup::{SetupEvent, SetupView};

/// The probe's harness state: the screen, what it asked for, and the
/// one-frame font-install flag.
type Probe = (SetupView, Option<SetupEvent>, bool);

/// A harness over the setup screen.
fn harness_for(view: SetupView) -> Harness<'static, Probe> {
    Harness::builder()
        .with_size(egui::Vec2::new(1000.0, 760.0))
        .build_ui_state(
            |ui, (screen, asked, installed): &mut Probe| {
                if !*installed {
                    kage_desktop::theme::install_fonts(ui.ctx());
                    kage_desktop::icons::install(ui.ctx());
                    kage_desktop::theme::apply_choice(
                        &kage_desktop::theme::ThemeChoice::Named("kage-shadow".to_owned()),
                        ui.ctx(),
                    );
                    *installed = true;
                    return;
                }
                screen.show(ui);
                // A single run spans several frames; only a fresh
                // event replaces the one a button asked for.
                if let Some(event) = screen.take_asked() {
                    *asked = Some(event);
                }
            },
            (view, None, false),
        )
}

/// How many nodes carry `label`.
fn count(harness: &Harness<'_, Probe>, label: &str) -> usize {
    harness.query_all_by_label(label).count()
}

/// How many text fields draw. A field's text is its value, so the
/// probes read them by role.
fn text_fields(harness: &Harness<'_, Probe>) -> usize {
    harness
        .query_all_by(|node| node.role() == egui::accesskit::Role::TextInput)
        .count()
}

/// The one text field, by its order on screen.
fn field<'a>(harness: &'a Harness<'a, Probe>) -> egui_kittest::Node<'a> {
    harness
        .query_all_by(|node| node.role() == egui::accesskit::Role::TextInput)
        .next()
        .expect("the path field draws")
}

#[test]
fn the_screen_says_what_is_missing_and_how_to_fix_it() {
    let mut harness = harness_for(SetupView::new());
    harness.run();
    assert_eq!(
        count(&harness, "kage is not installed"),
        1,
        "the screen names the problem"
    );
    assert_eq!(text_fields(&harness), 1, "the path field draws");
    assert_eq!(count(&harness, "Use"), 1, "the probe button draws");
    assert_eq!(count(&harness, "Check again"), 1, "the retry button draws");
    assert_eq!(
        count(
            &harness,
            "cargo install --locked --git https://github.com/QaidVoid/kage kage-cli"
        ),
        1,
        "the cargo install command draws"
    );
    assert_eq!(
        count(&harness, "nix profile install github:QaidVoid/kage"),
        1,
        "the nix command draws"
    );
    assert_eq!(
        count(&harness, "https://github.com/QaidVoid/kage/releases/latest"),
        1,
        "the release page draws"
    );
}

#[test]
fn an_empty_path_is_refused_where_the_user_can_see_it() {
    let mut harness = harness_for(SetupView::new());
    harness.run();
    harness.get_by_label("Use").click();
    harness.run();
    assert_eq!(
        harness.state().0.error(),
        Some("Enter the path to a kage binary."),
        "the screen says what is missing"
    );
    assert_eq!(harness.state().1, None, "nothing is handed to the shell");
    assert_eq!(
        count(&harness, "Enter the path to a kage binary."),
        1,
        "the refusal draws on the screen"
    );
}

#[test]
fn a_program_that_is_not_kage_is_refused_with_why() {
    let mut harness = harness_for(SetupView::new());
    harness.run();
    field(&harness).click();
    harness.run();
    field(&harness).type_text("true");
    harness.run();
    harness.get_by_label("Use").click();
    harness.run();
    assert_eq!(
        harness.state().0.error(),
        Some("true is not kage"),
        "the probe named what it found"
    );
    assert_eq!(harness.state().1, None, "the shell is not told to run it");
}

#[test]
fn a_probed_binary_is_handed_to_the_shell() {
    let stub = std::env::temp_dir().join("kage-setup-probe-probe");
    std::fs::write(&stub, "#!/bin/sh\necho 'kage 9.9.9'\n").expect("the stub is written");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755))
            .expect("the stub runs");
    }
    let path = stub.to_string_lossy().into_owned();
    let mut harness = harness_for(SetupView::new());
    harness.run();
    field(&harness).click();
    harness.run();
    field(&harness).type_text(&path);
    harness.run();
    harness.get_by_label("Use").click();
    harness.run();
    assert_eq!(
        harness.state().1,
        Some(SetupEvent::Use(path.clone())),
        "the shell is told which binary to run"
    );
    assert_eq!(harness.state().0.error(), None, "the refusal is cleared");
    let _ = std::fs::remove_file(&stub);
}

#[test]
fn the_check_again_button_asks_for_the_path_again() {
    let mut harness = harness_for(SetupView::new());
    harness.run();
    harness.get_by_label("Check again").click();
    harness.run();
    assert_eq!(
        harness.state().1,
        Some(SetupEvent::Retry),
        "the screen asks the shell to look again"
    );
}
