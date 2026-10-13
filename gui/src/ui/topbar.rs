//! The chat column's top bar: the project and session crumbs, the fork
//! and working pills, and the panel toggles at the far end.

use kage_client::Session;

use crate::icons::Icon;
use crate::store::Store;
use crate::theme::{FS_SM, FS_XS, PANEL_HEAD_H, Palette, R_MD, SP_4};
use crate::ui::forms::icon_button;
use crate::ui::sidebar::project_name;

/// What the top bar asked for; the shell carries it out after its
/// panels are drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TopbarEvent {
    ShowSidebar,
    ToggleWorkbench,
    Session(SessionAction),
    Rename(String),
}

/// The session actions of the menu at the top bar's end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionAction {
    Pin,
    Fork,
    Compact,
    Export,
    CopyId,
    Close,
    Archive,
}

const RENAME_DRAFT: &str = "topbar-rename-draft";
const RENAME_INPUT: &str = "topbar-rename-input";

fn start_rename(ui: &mut egui::Ui, title: String) {
    ui.data_mut(|data| data.insert_temp(egui::Id::new(RENAME_DRAFT), title));
    ui.ctx()
        .memory_mut(|memory| memory.request_focus(egui::Id::new(RENAME_INPUT)));
}

/// Draws the top bar over the chat column. `sidebar_hidden` shows the
/// control that brings the sidebar back.
pub fn show(ui: &mut egui::Ui, store: &Store, sidebar_hidden: bool, events: &mut Vec<TopbarEvent>) {
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
                let title = store.display_title(&session.id);
                rename_or_title(ui, palette, title, events);
                if store.fork_parent(&session.id).is_some() {
                    pill(ui, palette, |ui| {
                        ui.label(egui::RichText::new("fork").size(FS_XS).color(palette.muted));
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
                if let Some(session) = store.active_session() {
                    session_menu(ui, store, session, palette, events);
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

/// The session title, which a double click or the menu turns into an
/// inline field. Enter or leaving the field names the session; Escape
/// drops the edit.
fn rename_or_title(
    ui: &mut egui::Ui,
    palette: &Palette,
    title: String,
    events: &mut Vec<TopbarEvent>,
) {
    let draft_id = egui::Id::new(RENAME_DRAFT);
    let Some(mut draft) = ui.data(|data| data.get_temp::<String>(draft_id)) else {
        let response = ui
            .add(
                egui::Label::new(
                    egui::RichText::new(&title).size(FS_SM).color(palette.ink),
                )
                .truncate()
                .sense(egui::Sense::click()),
            )
            .on_hover_text("Double-click to rename");
        if response.double_clicked() {
            start_rename(ui, title);
        }
        return;
    };
    let edit = ui.add(
        egui::TextEdit::singleline(&mut draft)
            .id(egui::Id::new(RENAME_INPUT))
            .desired_width(240.0)
            .font(egui::FontId::new(FS_SM, crate::theme::family(crate::theme::FAMILY_BODY_REGULAR))),
    );
    let (enter, escape) = ui.input(|input| {
        (
            input.key_pressed(egui::Key::Enter),
            input.key_pressed(egui::Key::Escape),
        )
    });
    if escape {
        ui.data_mut(|data| data.remove::<String>(draft_id));
    } else if enter || edit.lost_focus() {
        ui.data_mut(|data| data.remove::<String>(draft_id));
        events.push(TopbarEvent::Rename(draft));
    } else {
        ui.data_mut(|data| data.insert_temp(draft_id, draft));
    }
}

/// The session actions behind the ellipsis button.
fn session_menu(
    ui: &mut egui::Ui,
    store: &Store,
    session: &Session,
    palette: &Palette,
    events: &mut Vec<TopbarEvent>,
) {
    let running = session.in_turn || session.running;
    let pinned = store.prefs().pinned.contains(&session.id);
    let (pin_label, pin_icon) = if pinned {
        ("Unpin", Icon::PinOff)
    } else {
        ("Pin", Icon::Pin)
    };
    egui::containers::menu::MenuButton::from_button(
        egui::Button::image(crate::icons::themed_image(
            Icon::Ellipsis,
            16.0,
            palette.muted,
        ))
        .fill(egui::Color32::TRANSPARENT)
        .corner_radius(egui::CornerRadius::same(R_MD as u8)),
    )
    .ui(ui, |ui| {
        ui.set_min_width(220.0);
        if ui
            .add(egui::Button::image_and_text(
                crate::icons::themed_image(Icon::Pencil, 14.0, palette.ink),
                "Rename",
            ))
            .clicked()
        {
            start_rename(ui, store.display_title(&session.id));
        }
        menu_item(
            ui,
            palette,
            pin_icon,
            pin_label,
            true,
            SessionAction::Pin,
            events,
        );
        menu_item(
            ui,
            palette,
            Icon::GitFork,
            "Fork session",
            true,
            SessionAction::Fork,
            events,
        );
        menu_item(
            ui,
            palette,
            Icon::Layers,
            "Compact now",
            !running,
            SessionAction::Compact,
            events,
        );
        menu_item(
            ui,
            palette,
            Icon::ExternalLink,
            "Export markdown",
            true,
            SessionAction::Export,
            events,
        );
        menu_item(
            ui,
            palette,
            Icon::Copy,
            "Copy session ID",
            true,
            SessionAction::CopyId,
            events,
        );
        ui.separator();
        menu_item(
            ui,
            palette,
            Icon::X,
            "Close session",
            true,
            SessionAction::Close,
            events,
        );
        menu_item(
            ui,
            palette,
            Icon::Archive,
            "Archive",
            true,
            SessionAction::Archive,
            events,
        );
    });
}

/// One row of the session menu.
fn menu_item(
    ui: &mut egui::Ui,
    palette: &Palette,
    icon: Icon,
    label: &str,
    enabled: bool,
    action: SessionAction,
    events: &mut Vec<TopbarEvent>,
) {
    let button =
        egui::Button::image_and_text(crate::icons::themed_image(icon, 14.0, palette.ink), label);
    if ui.add_enabled(enabled, button).clicked() {
        events.push(TopbarEvent::Session(action));
    }
}
