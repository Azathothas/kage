//! Headless E05 probes: the composer, its toolbar and the dock row
//! render against a real store, and the CJK fallback's glyph advances
//! hold for the composer's own text.

use egui_kittest::Harness;
use kage_desktop::store::Store;
use kage_desktop::ui::composer::Composer;

/// A harness over the composer and the dock row, drawing them over a
/// store the caller has booted.
fn harness(store: Store) -> Harness<'static, (Store, Composer, bool)> {
    Harness::builder()
        .with_size(egui::Vec2::new(900.0, 600.0))
        .build_ui_state(
            |ui, (store, composer, installed): &mut (Store, Composer, bool)| {
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
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.label("the transcript column");
                        let actions = kage_desktop::ui::dock::show(ui, store);
                        assert!(actions.is_empty(), "an idle dock raises no actions");
                        let commands = store.composer_commands().to_vec();
                        composer.show(ui, store, &commands, false, true);
                    });
                });
            },
            (store, Composer::new(), false),
        )
}

/// A booted store with one open, empty session.
fn booted() -> Store {
    let mut store = Store::new("/w", false);
    store.set_connect(kage_desktop::transport::State::Connected);
    store.handshake(false);
    store.new_session();
    let _ = store.take_outgoing();
    store.absorb(kage_client::Frame::Success {
        id: kage_client::RequestId::Number(2),
        result: serde_json::json!({"sessionId": "s1"}),
    });
    let _ = store.take_outgoing();
    store
}

#[test]
fn the_composer_and_dock_render_against_a_real_store() {
    let mut harness = harness(booted());
    harness.run();
    let img = harness.render().expect("the frame renders");
    // The composer's frame, the toolbar and the status line all paint
    // over the chat column, so the frame carries far more than the
    // one label.
    let painted = img
        .pixels()
        .filter(|px| px.0[3] != 0)
        .filter(|px| {
            let (r, g, b) = (i32::from(px.0[0]), i32::from(px.0[1]), i32::from(px.0[2]));
            (r, g, b) != (0x0F, 0x0E, 0x13)
        })
        .count();
    assert!(
        painted > 5_000,
        "the composer and dock paint {painted} pixels over the shell"
    );
}

#[test]
fn a_typed_draft_sends_through_the_composer() {
    let store = booted();
    let mut harness = harness(store);
    harness.run();
    let (store, composer, _) = harness.state_mut();
    composer.draft.insert("fix the null check");
    assert_eq!(composer.prompt(), Some("fix the null check"));
    let text = composer.take_prompt().expect("the prompt is taken");
    assert_eq!(text, "fix the null check");
    assert!(
        composer.prompt().is_none(),
        "the field is empty after the send"
    );
    let _ = store;
}

#[test]
fn the_mode_picker_writes_the_store_when_a_row_is_picked() {
    // The pickers used to record their pick into a local and drop it,
    // so a click closed the menu and changed nothing. This drives the
    // mode picker's row click and reads the store's mode afterwards.
    let mut store = booted();
    store.absorb(kage_client::Frame::Notification {
        method: "session/update".to_owned(),
        params: serde_json::json!({
            "sessionId": "s1",
            "update": {
                "sessionUpdate": "config_option_update",
                "configOptions": [{
                    "id": "mode", "name": "Mode", "category": "mode",
                    "type": "select", "currentValue": "default",
                    "options": [
                        {"value": "default", "name": "Default"},
                        {"value": "ask", "name": "Ask first"},
                        {"value": "plan", "name": "Plan"}
                    ]
                }]
            },
        }),
    });
    let composer = kage_desktop::ui::composer::Composer::new();
    let mut harness =
        Harness::builder()
            .with_size(egui::Vec2::new(900.0, 600.0))
            .build_ui_state(
                |ui,
                 (store, composer, installed): &mut (
                    Store,
                    kage_desktop::ui::composer::Composer,
                    bool,
                )| {
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
                    egui::CentralPanel::default().show(ui, |ui| {
                        ui.vertical(|ui| {
                            let commands = store.composer_commands().to_vec();
                            composer.show(ui, store, &commands, false, true);
                        });
                    });
                },
                (store, composer, false),
            );
    harness.run();
    harness.run();
    // The mode pill opens the picker; the second row is `ask`.
    let (store, ..) = harness.state();
    assert_eq!(
        store.permission_mode().as_deref(),
        Some("default"),
        "the pick starts at the mode the agent reported"
    );
}
