//! The chat column's top bar: the project and session crumbs, the fork
//! and working pills, and the panel toggles at the far end.

use crate::icons::Icon;
use crate::store::Store;
use crate::theme::{FS_SM, FS_XS, PANEL_HEAD_H, Palette, SP_4};
use crate::ui::forms::icon_button;
use crate::ui::sidebar::project_name;

/// What the top bar asked for; the shell carries it out after its
/// panels are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopbarEvent {
    ShowSidebar,
    ToggleWorkbench,
}

/// Draws the top bar over the chat column. `sidebar_hidden` shows the
/// control that brings the sidebar back.
pub fn show(
    ui: &mut egui::Ui,
    store: &Store,
    sidebar_hidden: bool,
    events: &mut Vec<TopbarEvent>,
) {
    let palette = Palette::active_of(ui.ctx());
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), PANEL_HEAD_H),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = SP_4;
            if sidebar_hidden && icon_button(ui, "show-sidebar", Icon::PanelLeft, palette).clicked()
            {
                events.push(TopbarEvent::ShowSidebar);
            }
            if let Some(session) = store.active_session() {
                ui.label(
                    egui::RichText::new(project_name(session.cwd.as_deref()))
                        .size(FS_SM)
                        .color(palette.muted),
                );
                ui.label(egui::RichText::new("/").size(FS_SM).color(palette.faint));
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(store.display_title(&session.id))
                            .size(FS_SM)
                            .color(palette.ink),
                    )
                    .truncate(),
                );
                if store.fork_parent(&session.id).is_some() {
                    pill(ui, palette, |ui| {
                        ui.label(
                            egui::RichText::new("fork")
                                .size(FS_XS)
                                .color(palette.muted),
                        );
                    });
                }
                if session.in_turn || session.running {
                    pill(ui, palette, |ui| {
                        crate::ui::eclipse::eclipse(ui, 14.0, None);
                        ui.label(
                            egui::RichText::new("Working")
                                .size(FS_XS)
                                .color(palette.muted),
                        );
                    });
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_button(ui, "toggle-workbench", Icon::PanelRight, palette).clicked() {
                    events.push(TopbarEvent::ToggleWorkbench);
                }
            });
        },
    );
}

/// A rounded fill holding a small row of content.
fn pill(ui: &mut egui::Ui, palette: &Palette, content: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::NONE
        .fill(palette.fill)
        .corner_radius(egui::CornerRadius::same(u8::MAX))
        .inner_margin(egui::Margin::symmetric(9, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                content(ui);
            });
        });
}
