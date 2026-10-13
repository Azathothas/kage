//! The chat column's top bar: the project and session crumbs, the fork
//! and working pills, and the panel toggles at the far end.

use kage_client::Session;

use crate::icons::Icon;
use crate::store::Store;
use crate::theme::{FS_SM, FS_XS, PANEL_HEAD_H, Palette, R_LG, R_MD, R_SM, SP_4};
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
    let input = egui::Id::new(RENAME_INPUT);
    // The whole title starts selected, so typing replaces it.
    let mut state = egui::text_edit::TextEditState::load(ui.ctx(), input).unwrap_or_default();
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::two(
            egui::text::CCursor::new(0),
            egui::text::CCursor::new(title.chars().count()),
        )));
    state.store(ui.ctx(), input);
    ui.data_mut(|data| data.insert_temp(egui::Id::new(RENAME_DRAFT), title));
    ui.ctx().memory_mut(|memory| memory.request_focus(input));
}

/// Draws the top bar over the chat column. `sidebar_hidden` shows the
/// control that brings the sidebar back.
pub fn show(
    ui: &mut egui::Ui,
    store: &Store,
    sidebar_hidden: bool,
    workbench_visible: bool,
    events: &mut Vec<TopbarEvent>,
) {
    let palette = Palette::active_of(ui.ctx());
    let (bar, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), PANEL_HEAD_H),
        egui::Sense::hover(),
    );
    let inner = egui::Rect::from_min_max(
        bar.min + egui::vec2(14.0, 0.0),
        bar.max - egui::vec2(10.0, 0.0),
    );
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = SP_4;
            if sidebar_hidden
                && icon_button(ui, "show-sidebar", Icon::PanelLeft, palette)
                    .on_hover_text("Show sidebar (Ctrl \\)")
                    .clicked()
            {
                events.push(TopbarEvent::ShowSidebar);
            }
            if let Some(session) = store.active_session() {
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.label(
                        egui::RichText::new(project_name(session.cwd.as_deref()))
                            .size(FS_SM)
                            .color(palette.muted),
                    );
                    ui.label(egui::RichText::new("/").size(FS_SM).color(palette.ghost));
                    let title = store.display_title(&session.id);
                    rename_or_title(ui, palette, title, events);
                });
                if store.fork_parent(&session.id).is_some() {
                    pill(ui, palette, |ui| {
                        ui.label(egui::RichText::new("fork").size(FS_XS).color(palette.muted));
                    });
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = SP_4;
                if ghost(
                    ui,
                    Icon::PanelRight,
                    workbench_visible,
                    "Workbench (Ctrl B)",
                    palette,
                )
                .clicked()
                {
                    events.push(TopbarEvent::ToggleWorkbench);
                }
                if let Some(session) = store.active_session() {
                    session_menu(ui, store, session, palette, events);
                    // The working pill at the bar's end, with how long
                    // the run has gone, as the reference shows it.
                    if session.in_turn || session.running {
                        let took = store
                            .timings(&session.id)
                            .and_then(|times| times.running_for())
                            .unwrap_or_default();
                        working_pill(ui, &crate::clock::duration(took.as_secs() as i64), palette);
                    }
                }
            });
        },
    );
}

/// The working pill: the turning eclipse and the elapsed label on the
/// fill, 24px tall.
fn working_pill(ui: &mut egui::Ui, elapsed: &str, palette: &Palette) {
    let label = format!("Working {elapsed}");
    let galley = ui.painter().layout_no_wrap(
        label.clone(),
        egui::FontId::new(
            FS_XS,
            crate::theme::family(crate::theme::FAMILY_BODY_REGULAR),
        ),
        palette.muted,
    );
    let size = egui::vec2(9.0 + 14.0 + 6.0 + galley.size().x + 9.0, 24.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &label));
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(u8::MAX), palette.fill);
    let mark = egui::Rect::from_min_size(
        egui::pos2(rect.left() + 9.0, rect.center().y - 7.0),
        egui::vec2(14.0, 14.0),
    );
    crate::ui::eclipse::paint(ui, mark, 14.0, Some(crate::clock::epoch()), palette);
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(250));
    ui.painter().galley(
        egui::pos2(mark.right() + 6.0, rect.center().y - galley.size().y / 2.0),
        galley,
        palette.muted,
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
                egui::Label::new(egui::RichText::new(&title).size(FS_SM).color(palette.ink))
                    .truncate()
                    .sense(egui::Sense::click()),
            )
            .on_hover_text("Double-click to rename");
        if response.double_clicked() {
            start_rename(ui, title);
        }
        return;
    };
    // The reference's small input: 260px by 26, its text 12px in, the
    // accent ring around it while it holds the focus.
    let edit = ui.add(
        egui::TextEdit::singleline(&mut draft)
            .id(egui::Id::new(RENAME_INPUT))
            .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(12, 0)))
            .min_size(egui::vec2(RENAME_W, RENAME_H))
            .desired_width(RENAME_W - 24.0)
            .vertical_align(egui::Align::Center)
            .text_color(palette.ink_strong)
            .font(egui::FontId::new(
                FS_SM,
                crate::theme::family(crate::theme::FAMILY_BODY_REGULAR),
            )),
    );
    ui.painter().rect_stroke(
        edit.rect,
        egui::CornerRadius::same(R_LG as u8),
        egui::Stroke::new(2.0, palette.accent_bd),
        egui::StrokeKind::Outside,
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

/// The session actions behind the ellipsis button: the reference's
/// popup menu, opening under the button's right edge.
fn session_menu(
    ui: &mut egui::Ui,
    store: &Store,
    session: &Session,
    palette: &Palette,
    events: &mut Vec<TopbarEvent>,
) {
    let open_id = egui::Id::new("session-menu-open");
    let open = ui.data(|data| data.get_temp::<bool>(open_id).unwrap_or(false));
    let button = ghost(ui, Icon::Ellipsis, open, "Session actions", palette);
    let mut open = open ^ button.clicked();
    if !open {
        ui.data_mut(|data| data.insert_temp(open_id, false));
        return;
    }
    let running = session.in_turn || session.running;
    let pinned = store.prefs().pinned.contains(&session.id);
    let (pin_label, pin_icon) = if pinned {
        ("Unpin", Icon::PinOff)
    } else {
        ("Pin", Icon::Pin)
    };
    let items: [Option<MenuItem>; 9] = [
        Some((Icon::Pencil, "Rename", true, None)),
        Some((pin_icon, pin_label, true, Some(SessionAction::Pin))),
        Some((
            Icon::GitFork,
            "Fork session",
            true,
            Some(SessionAction::Fork),
        )),
        Some((
            Icon::Layers,
            "Compact now",
            !running,
            Some(SessionAction::Compact),
        )),
        Some((
            Icon::ExternalLink,
            "Export markdown",
            true,
            Some(SessionAction::Export),
        )),
        Some((
            Icon::Copy,
            "Copy session ID",
            true,
            Some(SessionAction::CopyId),
        )),
        None,
        Some((Icon::X, "Close session", true, Some(SessionAction::Close))),
        Some((Icon::Archive, "Archive", true, Some(SessionAction::Archive))),
    ];
    let area = egui::Area::new(egui::Id::new("session-menu"))
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::RIGHT_TOP)
        .fixed_pos(button.rect.right_bottom() + egui::vec2(0.0, MENU_GAP))
        .show(ui.ctx(), |ui| {
            egui::Frame::NONE
                .fill(palette.menu)
                .stroke(egui::Stroke::new(1.0, palette.line))
                .corner_radius(egui::CornerRadius::same(R_MD as u8))
                .shadow(palette.shadow_menu)
                .inner_margin(egui::Margin::symmetric(0, MENU_PAD_Y as i8))
                .show(ui, |ui| {
                    ui.set_width(MENU_W);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for item in items {
                        let Some((icon, label, enabled, action)) = item else {
                            ui.add_space(4.0);
                            let (rule, _) = ui
                                .allocate_exact_size(egui::vec2(MENU_W, 1.0), egui::Sense::hover());
                            ui.painter()
                                .rect_filled(rule, egui::CornerRadius::ZERO, palette.line);
                            ui.add_space(3.0);
                            continue;
                        };
                        if menu_row(ui, icon, label, enabled, palette).clicked() {
                            open = false;
                            match action {
                                Some(action) => events.push(TopbarEvent::Session(action)),
                                None => start_rename(ui, store.display_title(&session.id)),
                            }
                        }
                    }
                });
        });
    let outside = ui.input(|i| i.pointer.any_pressed())
        && ui
            .input(|i| i.pointer.interact_pos())
            .is_some_and(|at| !area.response.rect.contains(at) && !button.rect.contains(at));
    if outside || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        open = false;
    }
    ui.data_mut(|data| data.insert_temp(open_id, open));
}

/// One session menu row: its icon, label, whether it is enabled, and
/// the action it asks for, or `None` for rename, which the bar does
/// itself.
type MenuItem<'a> = (Icon, &'a str, bool, Option<SessionAction>);

/// The rename field's width, the reference's 260px floor.
const RENAME_W: f32 = 260.0;
/// The rename field's height, the reference's small input.
const RENAME_H: f32 = 26.0;

/// The session menu's width, the reference's 220px floor.
const MENU_W: f32 = 220.0;
/// The space between the ellipsis button and the menu under it.
const MENU_GAP: f32 = 4.0;
/// The menu's padding above its first row and under its last.
const MENU_PAD_Y: f32 = 2.0;
/// One menu row's height.
const MENU_ROW_H: f32 = 28.0;

/// One menu row: the small icon and the label, the hover fill inset.
fn menu_row(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    enabled: bool,
    palette: &Palette,
) -> egui::Response {
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(MENU_W, MENU_ROW_H), sense);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    if enabled && response.hovered() {
        ui.painter().rect_filled(
            rect.shrink2(egui::vec2(4.0, 0.0)),
            egui::CornerRadius::same(R_SM as u8),
            palette.hover,
        );
    }
    let ink = if enabled { palette.ink } else { palette.faint };
    let mid = rect.center().y;
    crate::icons::themed_image(icon, 12.0, ink).paint_at(
        ui,
        egui::Rect::from_min_size(
            egui::pos2(rect.left() + 12.0, mid - 6.0),
            egui::vec2(12.0, 12.0),
        ),
    );
    ui.painter().text(
        egui::pos2(rect.left() + 25.0, mid),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::new(
            FS_SM,
            crate::theme::family(crate::theme::FAMILY_BODY_REGULAR),
        ),
        ink,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The reference's ghost icon button: 28px, the icon in the ink, filled
/// while hovered or while what it toggles is on.
fn ghost(
    ui: &mut egui::Ui,
    icon: Icon,
    selected: bool,
    tooltip: &str,
    palette: &Palette,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tooltip));
    if selected || response.hovered() {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(R_MD as u8),
            palette.fill_hover,
        );
    }
    crate::icons::themed_image(icon, 16.0, palette.ink).paint_at(
        ui,
        egui::Rect::from_center_size(rect.center(), egui::vec2(16.0, 16.0)),
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tooltip)
}
