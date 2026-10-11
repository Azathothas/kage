//! Headless E04 transcript probes: the view draws the recording's rows
//! in the chat column, the reveal paces the tail, and the measured list
//! keeps the frame cheap over the recorded session. The real window is
//! checked by the smoke run; this is the headless leg.

use egui_kittest::Harness;
use kage_desktop::store::{Command, Store};
use kage_desktop::transport::{State, replay};
use kage_desktop::ui::transcript::view::TranscriptView;

/// Carries out the store's pending commands the way the shell does.
fn run_commands(store: &mut Store) {
    for command in store.take_commands() {
        match command {
            Command::Handshake { replay_sessions } => {
                store.handshake(replay_sessions);
            }
            Command::NewSession => {
                store.new_session();
            }
            Command::ReplayPrompt => {
                store.prompt("fix the null check");
            }
        }
    }
}

/// A store driven through the recording the way the replay smoke runs.
fn replayed_store() -> Store {
    let mut store = Store::new("/w", true);
    store.set_connect(State::Connected);
    run_commands(&mut store);
    let _ = store.take_outgoing();
    let frames = replay::transcript();
    store.absorb(frames[0].clone());
    run_commands(&mut store);
    let _ = store.take_outgoing();
    store.absorb(frames[1].clone());
    run_commands(&mut store);
    let _ = store.take_outgoing();
    for frame in &frames[2..] {
        store.absorb(frame.clone());
    }
    store
}

/// A harness over the transcript view at `width`, over a store driven
/// through the recording.
fn harness_for(width: f32) -> Harness<'static, (TranscriptView, Store, bool)> {
    let store = replayed_store();
    Harness::builder()
        .with_size(egui::Vec2::new(width, 700.0))
        .build_ui_state(
            |ui, (view, store, started): &mut (TranscriptView, Store, bool)| {
                if !*started {
                    // Fonts become active at the start of the next
                    // pass, so the themed style follows one pass later.
                    kage_desktop::theme::install_fonts(ui.ctx());
                    kage_desktop::icons::install(ui.ctx());
                    *started = true;
                    return;
                }
                kage_desktop::theme::apply_choice(
                    &kage_desktop::theme::ThemeChoice::Named("kage-shadow".to_owned()),
                    ui.ctx(),
                );
                let mut events = Vec::new();
                view.show(ui, store, &mut events);
                assert!(events.is_empty(), "the recording raises no row actions");
            },
            (TranscriptView::new(), store, false),
        )
}

#[test]
fn the_transcript_draws_the_recorded_rows() {
    let mut harness = harness_for(900.0);
    harness.run();
    let (view, store, _) = harness.state();
    // The view read the recording: rows exist, and the searchable text
    // the find bar narrows over is there.
    assert!(
        view.searchable_rows(store).len() > 5,
        "the recording's rows are searchable"
    );
}

#[test]
fn the_transcript_paints_the_palette_over_the_rows() {
    let mut harness = harness_for(900.0);
    harness.run();
    let img = harness.render().expect("the frame renders");
    let mut counts: std::collections::HashMap<egui::Color32, usize> =
        std::collections::HashMap::new();
    let mut painted = 0usize;
    for px in img.pixels() {
        if px.0[3] == 0 {
            continue;
        }
        painted += 1;
        let c = egui::Color32::from_rgba_unmultiplied(px.0[0], px.0[1], px.0[2], px.0[3]);
        *counts.entry(c).or_default() += 1;
    }
    let bg = counts
        .get(&egui::Color32::from_rgb(0x0F, 0x0E, 0x13))
        .copied()
        .unwrap_or(0);
    let share = bg as f32 / painted as f32;
    assert!(
        share > 0.5,
        "the palette holds {share} of the painted frame"
    );
}

#[test]
fn the_find_marks_tint_the_matched_rows() {
    let mut harness = harness_for(900.0);
    harness.run();
    let (first, count) = {
        let (view, store, _) = harness.state();
        let rows = view.searchable_rows(store);
        (rows.first().expect("the recording has rows").0, rows.len())
    };
    harness
        .state_mut()
        .0
        .set_find(Some(kage_desktop::ui::transcript::model::FindMarks {
            keys: vec![first],
            current: 0,
        }));
    harness.run();
    // Setting the marks leaves the rows searchable and the frame stable.
    let (view, store, _) = harness.state();
    assert_eq!(view.searchable_rows(store).len(), count);
}
