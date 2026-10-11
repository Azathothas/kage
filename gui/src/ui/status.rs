//! The status bar and the gate banner.
//!
//! The bottom bar names the connection state, the agent once it is
//! known, and the model in effect. The banner sits above it when the
//! initialize answer fell short of what the shell gates on, and
//! dismisses for that answer; a reconnect that still falls short
//! brings it back.

use crate::gate::Report;
use crate::store::Store;
use crate::theme::{FS_XS, Palette, SP_4, SP_6};

/// The status bar's height.
const BAR_H: f32 = 26.0;

/// The bottom bar: the connection state, the agent and the model.
///
/// The connection string comes from the store's own state, which the
/// pump keeps in step; the agent name and version arrive with the
/// handshake, and the model label with the first session's config.
pub fn status_bar(ui: &mut egui::Ui, store: &Store) {
    let palette = Palette::active_of(ui.ctx());
    let state = store.state();
    let connect = store.connect().label();
    let agent = state
        .agent
        .as_ref()
        .map(|agent| format!("{} {}", agent.name, agent.version.as_deref().unwrap_or("?")))
        .unwrap_or_else(|| "connecting".to_owned());
    let model = store
        .active_session()
        .and_then(|session| crate::modes::select_option(session, "model"))
        .map(|option| option.current_value.clone())
        .unwrap_or_else(|| "default".to_owned());

    egui::Panel::bottom("status")
        .exact_size(BAR_H)
        .frame(
            egui::Frame::NONE
                .fill(palette.bg)
                .inner_margin(egui::Margin::symmetric(SP_6 as i8, 0)),
        )
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.set_height(BAR_H);
                ui.spacing_mut().item_spacing.x = SP_6;
                let dot = match store.connect() {
                    crate::transport::State::Connected => palette.ok,
                    crate::transport::State::Refused(_) => palette.danger,
                    crate::transport::State::Reconnecting { .. } => palette.warn,
                    _ => palette.muted,
                };
                let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 4.0, dot);
                ui.label(
                    egui::RichText::new(connect)
                        .size(FS_XS)
                        .color(palette.muted),
                );
                ui.separator();
                ui.label(
                    egui::RichText::new(agent)
                        .size(FS_XS)
                        .color(palette.ink_strong),
                );
                ui.separator();
                ui.label(egui::RichText::new(model).size(FS_XS).color(palette.muted));
            });
        });
}

/// The gate banner over the status bar, when the initialize answer
/// fell short. Dismissible for that answer: the shell calls
/// [`Banner::dismiss`] on the close click and [`Banner::reset`] when a
/// reconnect starts, so a still-short answer shows again.
#[derive(Debug, Clone, Default)]
pub struct Banner {
    /// Whether the user dismissed the current report.
    dismissed: bool,
}

impl Banner {
    /// Takes the dismissal back, for a fresh answer.
    pub fn reset(&mut self) {
        self.dismissed = false;
    }

    /// Dismisses the current report.
    pub fn dismiss(&mut self) {
        self.dismissed = true;
    }

    /// Whether the banner shows now: a report that falls short and no
    /// dismissal against it.
    #[must_use]
    pub fn shows(&self, report: &Report) -> bool {
        !report.is_clean() && !self.dismissed
    }

    /// Draws the banner, when it shows. The close click dismisses.
    pub fn show(&mut self, ui: &mut egui::Ui, report: &Report) {
        if !self.shows(report) {
            return;
        }
        let palette = Palette::active_of(ui.ctx());
        let mut dismiss = false;
        egui::Panel::top("gate-banner")
            .frame(
                egui::Frame::NONE
                    .fill(palette.warn_soft)
                    .inner_margin(egui::Margin::symmetric(SP_6 as i8, SP_4 as i8))
                    .stroke(egui::Stroke::new(1.0, palette.warn_bd)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("this agent is newer than the shell expects")
                            .size(FS_XS)
                            .color(palette.ink_strong),
                    );
                    for line in report.lines() {
                        ui.label(egui::RichText::new(line).size(FS_XS).color(palette.muted));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new("dismiss")
                                    .size(FS_XS)
                                    .color(palette.muted),
                            ))
                            .clicked()
                        {
                            dismiss = true;
                        }
                    });
                });
            });
        if dismiss {
            self.dismiss();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Banner;
    use crate::gate::{Capability, Report};

    #[test]
    fn a_clean_report_shows_nothing() {
        let banner = Banner::default();
        assert!(!banner.shows(&Report::default()));
    }

    #[test]
    fn a_short_report_shows_until_dismissed() {
        let mut banner = Banner::default();
        let report = Report {
            too_old: Some("0.0.1".to_owned()),
            missing: vec![Capability::Steer],
        };
        assert!(banner.shows(&report));
        banner.dismiss();
        assert!(!banner.shows(&report), "the dismissal holds");
    }

    #[test]
    fn a_reconnect_brings_a_short_report_back() {
        let mut banner = Banner::default();
        let report = Report {
            too_old: Some("0.0.1".to_owned()),
            missing: Vec::new(),
        };
        banner.dismiss();
        banner.reset();
        assert!(banner.shows(&report), "a fresh answer shows again");
    }
}
