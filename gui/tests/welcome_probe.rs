//! Headless E05 probes: the welcome pane draws its cards, pill and
//! hint over a real store with no session, a card fills the composer
//! draft and holds its option, and the pill's menu asks for the
//! folder dialog.

use egui_kittest::Harness;
use kage_desktop::store::Store;
use kage_desktop::ui::composer::Composer;
use kage_desktop::ui::welcome::{Welcome, WelcomeEvent};

mod testutil;

/// A sessionless store: the link is up, no session is open, so the
/// chat column shows the welcome pane.
fn sessionless() -> Store {
    let mut store = Store::new("/w", false);
    store.set_connect(kage_desktop::transport::State::Connected);
    store.handshake(false);
    let _ = store.take_outgoing();
    store
}

type Pane = (Store, Welcome, Composer, Vec<WelcomeEvent>, bool);

/// A harness over the welcome pane, wired the way the shell is: the
/// pane hosts the composer inside its centered stack, a `Fill` sets
/// the draft and holds its option, and an `OpenFolder` is recorded
/// for the shell's dialog layer.
fn harness(store: Store) -> Harness<'static, Pane> {
    Harness::builder()
        .with_size(egui::Vec2::new(1000.0, 760.0))
        .build_ui_state(
            |ui, (store, welcome, composer, events, installed): &mut Pane| {
                if testutil::theme_installed(installed, ui.ctx()) {
                    return;
                }
                egui::CentralPanel::default().show(ui, |ui| {
                    let mut pane = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(ui.available_rect_before_wrap())
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                    );
                    let commands = store.composer_commands().to_vec();
                    let (evs, composer_event) =
                        welcome.show(&mut pane, store, composer, &commands, false, true);
                    let _ = composer_event;
                    for event in evs {
                        match event {
                            WelcomeEvent::Fill { text, option } => {
                                if let Some((id, value)) = option {
                                    store.hold_option(id, value);
                                }
                                composer.set_draft(text.to_owned());
                                events.push(WelcomeEvent::Fill { text, option });
                            }
                            other => events.push(other),
                        }
                    }
                });
            },
            (store, Welcome::new(), Composer::new(), Vec::new(), false),
        )
}

#[test]
fn the_welcome_draws_its_cards_pill_and_hint() {
    use egui_kittest::kittest::Queryable;

    let mut harness = harness(sessionless());
    harness.run();
    harness.run();
    for card in [
        "Find the failing tests and fix them",
        "Review every module for unhandled errors",
        "Plan the next change before touching code",
        "Delegate a docs check to subagents",
    ] {
        harness
            .query_by_label(card)
            .unwrap_or_else(|| panic!("{card} draws"));
    }
    // The pill names the directory the next session opens in.
    harness
        .query_by_label("w")
        .expect("the pill names the store's directory");
    harness
        .query_by_label("commands and sessions")
        .expect("the hint line draws");
}

#[test]
fn a_card_fills_the_draft_and_holds_its_option() {
    use egui_kittest::kittest::Queryable;

    let mut harness = harness(sessionless());
    harness.run();
    harness.run();
    let card = harness
        .query_by_label("Review every module for unhandled errors")
        .expect("the swarm card draws");
    card.click();
    harness.run();
    let (.., composer, events, _) = harness.state_mut();
    assert_eq!(
        composer.draft.text, "Review every module for unhandled errors",
        "the card fills the draft"
    );
    assert_eq!(
        events.as_slice(),
        &[WelcomeEvent::Fill {
            text: "Review every module for unhandled errors",
            option: Some(("swarm", "on")),
        }],
        "the card's option rides the fill"
    );
}

#[test]
fn the_pill_opens_the_menu_and_the_folder_row_asks_the_shell() {
    use egui_kittest::kittest::Queryable;

    let mut harness = harness(sessionless());
    harness.run();
    harness.run();
    let pill = harness.query_by_label("w").expect("the pill draws");
    pill.click();
    harness.run();
    harness
        .query_by_label("PROJECT")
        .expect("the menu opens with its header");
    harness
        .query_by_label("Open folder\u{2026}")
        .expect("the menu carries the folder action");
    let row = harness
        .query_by_label("Open folder\u{2026}")
        .expect("the folder row draws");
    row.click();
    harness.run();
    let (.., events, _) = harness.state_mut();
    assert_eq!(events.as_slice(), &[WelcomeEvent::OpenFolder]);
}

#[test]
fn the_pane_leaves_the_composer_its_band_and_the_stack_sits_above_it() {
    // The shell reserves the composer's band under the pane. The
    // stack must center in what is left and never reach into the
    // band, or the field draws over the cards.
    use egui_kittest::kittest::Queryable;

    let mut harness = harness(sessionless());
    harness.run();
    harness.run();
    harness.run();
    let hint = harness
        .query_by_label("commands and sessions")
        .expect("the hint draws");
    assert!(
        hint.rect().bottom() < 610.0,
        "the stack ends above the composer band, at {:?}",
        hint.rect()
    );
    let center = (hint.rect().left() + hint.rect().right()) / 2.0;
    assert!(
        (center - 500.0).abs() < 60.0,
        "the hint centers in the column, rect={:?}",
        hint.rect()
    );
    let card = harness
        .query_by_label("Find the failing tests and fix them")
        .expect("the cards draw");
    assert!(
        card.rect().right() < 1000.0,
        "the cards keep inside the pane, at {:?}",
        card.rect()
    );
    let img = harness.render().expect("the frame renders");
    if std::env::var_os("KAGE_PROBE_PNG").is_some() {
        img.save("/tmp/welcome-pane.png")
            .expect("the pane view saves");
    }
}
