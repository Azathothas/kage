//! The composer's toolbar: the plus popover, the mode, model and
//! thinking pickers, the context ring and the send or stop control.
//!
//! Each picker is an egui window over the pill that opens it, holding
//! a search field and a highlighted row. The rows come from the
//! store's config options and model catalog, so the toolbar names
//! only what the agent advertised and dims what it did not.

use crate::icons::Icon;
use crate::store::Store;
use crate::theme::{
    FAMILY_BODY_MEDIUM, FAMILY_BODY_SEMIBOLD, FAMILY_MONO_REGULAR, FS_2XS, FS_XS, Palette, R_FULL,
    R_LG, R_MD, SP_2, SP_4, family,
};
use crate::ui::pickers;

/// The toolbar's height, the old row's 30px pills.
const PILL_H: f32 = 30.0;

/// The popover surface's width.
const MENU_W: f32 = 440.0;

/// The model picker's surface width.
const MODEL_W: f32 = 400.0;

/// Which picker popover shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Picker {
    /// The permission mode list.
    Mode,
    /// The model and thinking list.
    Model,
}

/// What the toolbar's controls asked the shell to carry out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarEvent {
    /// Open the goal dialog.
    OpenGoal,
    /// Toggle plan mode.
    TogglePlan,
    /// Open the swarm confirmation.
    ConfirmSwarm,
    /// Turn swarm mode off.
    SwarmOff,
    /// Seed an at-mention in the composer's draft.
    SeedMention,
    /// Seed a slash command in the composer's draft.
    SeedSlash,
    /// Interrupt the run in flight.
    Cancel,
    /// Summarize older turns to free context.
    Compact,
    /// Send or queue the draft.
    Submit,
}

/// The toolbar's state: which popover is open and each picker's
/// search field and highlight.
#[derive(Debug, Default)]
pub struct Toolbar {
    /// Which picker shows, if any.
    picker: Option<Picker>,
    /// The mode picker's query.
    mode_query: String,
    /// The model picker's query.
    model_query: String,
    /// The mode picker's highlighted row.
    mode_highlight: usize,
    /// The model picker's highlighted row.
    model_highlight: usize,
    /// Whether the plus popover shows.
    plus_open: bool,
    /// Whether the context gauge shows.
    gauge_open: bool,
}

impl Toolbar {
    /// A closed toolbar.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Which picker shows, if any.
    #[must_use]
    pub fn picker(&self) -> Option<Picker> {
        self.picker
    }

    /// The mode picker's query.
    pub fn mode_query(&mut self) -> &mut String {
        &mut self.mode_query
    }

    /// The model picker's query.
    pub fn model_query(&mut self) -> &mut String {
        &mut self.model_query
    }

    /// Opens `which`, closing the other, or closes it when it shows.
    pub fn toggle_picker(&mut self, which: Picker) {
        self.picker = match self.picker {
            Some(open) if open == which => None,
            _ => Some(which),
        };
    }

    /// Closes both pickers.
    pub fn close_pickers(&mut self) {
        self.picker = None;
    }

    /// Whether the plus popover shows.
    #[must_use]
    pub fn plus_open(&self) -> bool {
        self.plus_open
    }

    /// Toggles the plus popover.
    pub fn toggle_plus(&mut self) {
        self.plus_open = !self.plus_open;
    }

    /// Closes the plus popover.
    pub fn close_plus(&mut self) {
        self.plus_open = false;
    }

    /// Whether the context gauge shows.
    #[must_use]
    pub fn gauge_open(&self) -> bool {
        self.gauge_open
    }

    /// Toggles the context gauge.
    pub fn toggle_gauge(&mut self) {
        self.gauge_open = !self.gauge_open;
    }

    /// The mode picker's highlighted row.
    pub const fn mode_highlight(&self) -> usize {
        self.mode_highlight
    }

    /// The model picker's highlighted row.
    pub const fn model_highlight(&self) -> usize {
        self.model_highlight
    }

    /// Steps the mode picker's highlight.
    pub fn step_mode(&mut self, delta: isize, total: usize) {
        self.mode_highlight = pickers::step_highlight(self.mode_highlight, total, delta);
    }

    /// Steps the model picker's highlight.
    pub fn step_model(&mut self, delta: isize, total: usize) {
        self.model_highlight = pickers::step_highlight(self.model_highlight, total, delta);
    }

    /// Whether any popover over the toolbar shows, so the shell knows
    /// the composer's keys are not the composer's to answer.
    #[must_use]
    pub fn any_open(&self) -> bool {
        self.picker.is_some() || self.plus_open || self.gauge_open
    }
}

/// The active permission mode's display name: the mode option's row
/// for `active`, else the raw value.
#[must_use]
pub fn mode_label(store: &Store, active: &str) -> String {
    store
        .composer_option("mode")
        .and_then(|option| {
            option
                .options
                .iter()
                .find(|value| value.value == active)
                .map(|value| value.name.clone())
        })
        .unwrap_or_else(|| active.to_owned())
}

/// The model pill's label: the model option's row for its current
/// value, else the value's last path segment.
#[must_use]
pub fn model_label(store: &Store) -> String {
    let Some(option) = store.composer_option("model") else {
        return "default".to_owned();
    };
    option
        .options
        .iter()
        .find(|value| value.value == option.current_value)
        .map(|value| value.name.clone())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| {
            option
                .current_value
                .rsplit('/')
                .next()
                .unwrap_or(&option.current_value)
                .to_owned()
        })
}

/// The thinking pill's trailing label, or `None` while thinking is
/// off or unadvertised.
#[must_use]
pub fn thinking_label(store: &Store) -> Option<String> {
    let option = store.composer_option("thinking")?;
    if option.current_value == "off" {
        return None;
    }
    Some(
        option
            .options
            .iter()
            .find(|value| value.value == option.current_value)
            .map(|value| value.name.clone())
            .unwrap_or_else(|| option.current_value.clone()),
    )
}

/// One pill: 30px tall, fully round, muted text that fills and inks on
/// hover or while its popover is open.
/// One toolbar pill: a framed row of the widgets `body` draws, so the
/// fill and the contents share one rect and the click covers the whole
/// pill. Returns the pill's response.
fn pill(
    ui: &mut egui::Ui,
    open: bool,
    palette: &Palette,
    body: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    egui::Frame::NONE
        .fill(if open {
            palette.hover
        } else {
            egui::Color32::TRANSPARENT
        })
        .corner_radius(egui::CornerRadius::same(R_FULL as u8))
        .inner_margin(egui::Margin::symmetric(9, 0))
        .show(ui, |ui| {
            ui.set_min_height(PILL_H);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                body(ui);
            });
        })
        .response
}

/// One toolbar pill that only carries a text label, dimmed.
fn dimmed_pill(ui: &mut egui::Ui, label: &str, palette: &Palette, tip: &str) -> egui::Response {
    let response = egui::Frame::NONE
        .corner_radius(egui::CornerRadius::same(R_FULL as u8))
        .inner_margin(egui::Margin::symmetric(9, 0))
        .show(ui, |ui| {
            ui.set_min_height(PILL_H);
            ui.label(
                egui::RichText::new(label)
                    .size(FS_XS + 1.0)
                    .color(palette.faint),
            );
        })
        .response;
    response.on_hover_text(tip)
}

/// A small mono chip, as the picker rows and the gauge carry.
fn chip(ui: &mut egui::Ui, text: &str, palette: &Palette) -> egui::Response {
    let text = egui::RichText::new(text)
        .size(10.5)
        .family(family(FAMILY_MONO_REGULAR))
        .color(palette.muted);
    egui::Frame::NONE
        .fill(palette.fill)
        .corner_radius(egui::CornerRadius::same(5))
        .inner_margin(egui::Margin::symmetric(6, 3))
        .show(ui, |ui| ui.label(text))
        .response
}

/// The uppercase section label between popover row groups.
fn section_label(ui: &mut egui::Ui, text: &str, palette: &Palette) {
    let text = egui::RichText::new(text.to_uppercase())
        .size(FS_2XS)
        .family(family(FAMILY_BODY_SEMIBOLD))
        .color(palette.faint);
    ui.add_space(SP_4);
    ui.label(text);
    ui.add_space(SP_2 / 2.0);
}

/// The on/off chip the mode rows carry.
fn onoff(ui: &mut egui::Ui, on: bool, palette: &Palette) {
    let (bg, fg) = if on {
        (palette.accent_soft, palette.accent)
    } else {
        (palette.fill, palette.faint)
    };
    let text = egui::RichText::new(if on { "on" } else { "off" })
        .size(FS_2XS)
        .family(family(FAMILY_MONO_REGULAR))
        .color(fg);
    egui::Frame::NONE
        .fill(bg)
        .corner_radius(egui::CornerRadius::same(R_FULL as u8))
        .inner_margin(egui::Margin::symmetric(8, 3))
        .show(ui, |ui| ui.label(text));
}

/// The popover surface the pickers and the plus menu render on.
fn surface(ui: &mut egui::Ui, palette: &Palette, width: f32, body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::NONE
        .fill(palette.menu)
        .stroke(egui::Stroke::new(1.0, palette.line))
        .corner_radius(egui::CornerRadius::same(R_LG as u8))
        .inner_margin(egui::Margin::same(5))
        .shadow(palette.shadow_menu)
        .show(ui, |ui| {
            ui.set_width(width);
            body(ui);
        });
}

/// A key cap, as the hint line draws it.
fn kbd(ui: &mut egui::Ui, label: &str, palette: &Palette) {
    let text = egui::RichText::new(label)
        .size(10.0)
        .family(family(FAMILY_MONO_REGULAR))
        .color(palette.faint);
    egui::Frame::NONE
        .stroke(egui::Stroke::new(1.0, palette.line))
        .corner_radius(egui::CornerRadius::same(5))
        .inner_margin(egui::Margin::symmetric(5, 2))
        .show(ui, |ui| ui.label(text));
}

/// The toolbar row: the plus popover, the mode pill with its plan and
/// swarm chips, the compact and context controls, and the model pill.
/// Returns what the controls asked the shell to carry out.
pub fn show(
    ui: &mut egui::Ui,
    toolbar: &mut Toolbar,
    store: &Store,
    running: bool,
    has_text: bool,
) -> Vec<ToolbarEvent> {
    let palette = Palette::active_of(ui.ctx());
    let mut events = Vec::new();
    ui.horizontal(|ui| {
        ui.set_height(PILL_H);
        ui.spacing_mut().item_spacing = egui::vec2(SP_2, 0.0);
        ui.add_space(SP_4);
        plus_button(ui, toolbar, store, &mut events);
        mode_button(ui, toolbar, store, &mut events);
        plan_chip(ui, store, &mut events);
        swarm_chip(ui, store, &mut events);
        ui.add_space(SP_2);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            send_button(ui, store, running, has_text, &mut events);
            model_button(ui, toolbar, store, &mut events);
            compact_button(ui, store, running, &mut events);
            context_ring(ui, toolbar, store, &mut events);
        });
    });
    let _ = palette;
    events
}

/// The plus pill and its popover: attach, mention, commands and the
/// mode toggles.
fn plus_button(
    ui: &mut egui::Ui,
    toolbar: &mut Toolbar,
    store: &Store,
    events: &mut Vec<ToolbarEvent>,
) -> Option<()> {
    let palette = Palette::active_of(ui.ctx());
    let open = toolbar.plus_open();
    let response = ui.add(
        egui::Button::new(
            egui::RichText::new("+")
                .size(16.0)
                .family(family(FAMILY_BODY_MEDIUM))
                .color(palette.muted),
        )
        .fill(if open {
            palette.hover
        } else {
            egui::Color32::TRANSPARENT
        })
        .corner_radius(egui::CornerRadius::same(R_FULL as u8))
        .min_size(egui::vec2(PILL_H, PILL_H)),
    );
    if response.clicked() {
        toolbar.toggle_plus();
    }
    if !open {
        return Some(());
    }
    let goal = pickers::session_goal(store.active_session());
    let plan_on = store.plan_on();
    let swarm_on = store.swarm_on();
    let swarm_known = store.composer_option("swarm").is_some();
    egui::Window::new("plus-popover")
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .fixed_pos(response.rect.left_bottom() + egui::vec2(0.0, SP_2))
        .fixed_size(egui::vec2(MENU_W, 0.0))
        .order(egui::Order::Foreground)
        .frame(egui::Frame::NONE)
        .show(ui.ctx(), |ui| {
            surface(ui, palette, MENU_W, |ui| {
                ui.set_max_height(420.0);
                section_label(ui, "Attach", palette);
                dimmed_row(
                    ui,
                    palette,
                    Icon::Paperclip,
                    "Files",
                    "Upload files or images",
                    Some("drop"),
                );
                let mention = plus_row_button(
                    ui,
                    palette,
                    Icon::AtSign,
                    "Mention",
                    "Project files from the session workdir",
                    Some("@"),
                );
                if mention.clicked() {
                    events.push(ToolbarEvent::SeedMention);
                    toolbar.close_plus();
                }
                let commands = plus_row_button(
                    ui,
                    palette,
                    Icon::Slash,
                    "Commands",
                    "The commands the agent sent",
                    Some("/"),
                );
                if commands.clicked() {
                    events.push(ToolbarEvent::SeedSlash);
                    toolbar.close_plus();
                }
                section_label(ui, "Modes", palette);
                let goal_row = plus_row_button(
                    ui,
                    palette,
                    Icon::Target,
                    "Goal",
                    "Set a goal to keep pursuing",
                    Some(if goal.is_some() { "set" } else { "none" }),
                );
                if goal_row.clicked() {
                    events.push(ToolbarEvent::OpenGoal);
                    toolbar.close_plus();
                }
                let plan = plus_row_button(
                    ui,
                    palette,
                    Icon::PenLine,
                    "Plan",
                    if plan_on {
                        "Turn plan mode off"
                    } else {
                        "Turn plan mode on"
                    },
                    None,
                );
                if plan.clicked() {
                    events.push(ToolbarEvent::TogglePlan);
                    toolbar.close_plus();
                }
                ui.horizontal(|ui| {
                    let _ = ui;
                    onoff(ui, plan_on, palette);
                });
                let swarm = plus_row_button(
                    ui,
                    palette,
                    Icon::Waypoints,
                    "Swarm",
                    if swarm_on {
                        "Turn swarm mode off"
                    } else {
                        "Turn swarm mode on"
                    },
                    None,
                );
                if swarm.clicked() {
                    if swarm_on {
                        events.push(ToolbarEvent::SwarmOff);
                    } else {
                        events.push(ToolbarEvent::ConfirmSwarm);
                    }
                    toolbar.close_plus();
                }
                ui.horizontal(|ui| {
                    if swarm_known {
                        onoff(ui, swarm_on, palette);
                    } else {
                        ui.label(
                            egui::RichText::new("the agent sent no swarm option")
                                .size(FS_2XS)
                                .color(palette.faint),
                        );
                    }
                });
            });
        });
    Some(())
}

/// One plus-menu row: a filled icon chip in front of the label and
/// description, with an optional key cap at the far end. Returns the
/// row's response, so the caller wires its action.
fn plus_row_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    icon: Icon,
    label: &str,
    description: &str,
    tail: Option<&str>,
) -> egui::Response {
    let response = egui::Frame::NONE
        .fill(egui::Color32::TRANSPARENT)
        .corner_radius(egui::CornerRadius::same(R_MD as u8))
        .inner_margin(egui::Margin::symmetric(7, 6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                let chip_rect = egui::Frame::NONE
                    .fill(palette.fill)
                    .corner_radius(egui::CornerRadius::same(7))
                    .inner_margin(egui::Margin::same(6))
                    .show(ui, |ui| {
                        crate::icons::icon(ui, icon, 14.0, palette.muted);
                    });
                let _ = chip_rect;
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(label)
                            .size(FS_XS + 1.0)
                            .color(palette.ink),
                    );
                    ui.label(
                        egui::RichText::new(description)
                            .size(11.5)
                            .color(palette.faint),
                    );
                });
                if let Some(tail) = tail {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        kbd(ui, tail, palette);
                    });
                }
            });
        })
        .response;
    let hovered = response.hovered();
    if hovered {
        let rect = response.rect;
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(R_MD as u8), palette.selected);
    }
    response
}

/// A plus-menu row that does nothing yet, drawn dimmed with its key
/// cap: the attachment surface carries text only for now.
fn dimmed_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    icon: Icon,
    label: &str,
    description: &str,
    tail: Option<&str>,
) {
    let response = plus_row_button(ui, palette, icon, label, description, tail);
    let _ = response;
}

/// The thinking rows the model picker lists, from the active session.
fn thinking_rows_for(store: &Store) -> Vec<kage_client::wire::SessionConfigSelectOption> {
    store
        .active_session()
        .map(|session| pickers::thinking_rows(session, ""))
        .unwrap_or_default()
}

/// The permission mode pill, listing the advertised values with the
/// active one marked.
fn mode_button(
    ui: &mut egui::Ui,
    toolbar: &mut Toolbar,
    store: &Store,
    events: &mut Vec<ToolbarEvent>,
) -> Option<()> {
    let palette = Palette::active_of(ui.ctx());
    let open = toolbar.picker() == Some(Picker::Mode);
    let Some(option) = store.composer_option("mode") else {
        dimmed_pill(ui, "Mode", palette, "the agent sent no mode config option");
        return Some(());
    };
    let active = store
        .permission_mode()
        .unwrap_or_else(|| option.current_value.clone());
    let label = mode_label(store, &active);
    let tone = pickers::mode_tone(&active, palette);
    let text_color = tone.unwrap_or(palette.muted);
    let response = pill(ui, open, palette, |ui| {
        crate::icons::icon(ui, pickers::mode_icon(&active), 14.0, text_color);
        ui.label(
            egui::RichText::new(label)
                .size(FS_XS + 1.0)
                .family(family(FAMILY_BODY_MEDIUM))
                .color(text_color),
        );
        crate::icons::icon(ui, Icon::ChevronDown, 12.0, palette.faint);
    });
    if response.clicked() {
        toolbar.toggle_picker(Picker::Mode);
    }
    if !open {
        return Some(());
    }
    let Some(session) = store.active_session() else {
        return Some(());
    };
    let mut picked_mode: Option<String> = None;
    egui::Window::new("mode-picker")
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .fixed_pos(response.rect.left_bottom() + egui::vec2(0.0, SP_2))
        .fixed_size(egui::vec2(MODEL_W, 0.0))
        .order(egui::Order::Foreground)
        .frame(egui::Frame::NONE)
        .show(ui.ctx(), |ui| {
            surface(ui, palette, MODEL_W, |ui| {
                ui.set_max_height(360.0);
                let query = ui.add(
                    egui::TextEdit::singleline(toolbar.mode_query())
                        .hint_text("Search modes")
                        .desired_width(MODEL_W - 16.0)
                        .id(egui::Id::new("mode-picker-query")),
                );
                if query.gained_focus() {
                    toolbar.mode_query().clear();
                }
                let rows = pickers::mode_rows(session, &toolbar.mode_query().clone());
                let total = rows.len();
                let highlight = toolbar.mode_highlight();
                for (index, row) in rows.iter().enumerate() {
                    let selected = index == highlight;
                    let response = egui::Frame::NONE
                        .fill(if selected {
                            palette.selected
                        } else {
                            egui::Color32::TRANSPARENT
                        })
                        .corner_radius(egui::CornerRadius::same(R_MD as u8))
                        .inner_margin(egui::Margin::symmetric(9, 7))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 10.0;
                                let tint = pickers::mode_tone(&row.value, palette)
                                    .unwrap_or(palette.muted);
                                crate::icons::icon(ui, pickers::mode_icon(&row.value), 14.0, tint);
                                ui.vertical(|ui| {
                                    ui.label(
                                        egui::RichText::new(row.name.clone())
                                            .size(FS_XS + 1.0)
                                            .color(palette.ink),
                                    );
                                    if let Some(description) = &row.description {
                                        ui.label(
                                            egui::RichText::new(description.clone())
                                                .size(11.5)
                                                .color(palette.faint),
                                        );
                                    }
                                });
                                if row.value == active {
                                    ui.label(
                                        egui::RichText::new("active")
                                            .size(FS_2XS)
                                            .color(palette.accent),
                                    );
                                }
                            });
                        })
                        .response;
                    if response.clicked() {
                        picked_mode = Some(row.value.clone());
                        toolbar.close_pickers();
                    }
                }
                if total == 0 {
                    ui.label(
                        egui::RichText::new("no modes match")
                            .size(FS_XS)
                            .color(palette.faint),
                    );
                }
            });
        });
    let _ = events;
    Some(())
}

/// The plan mode chip, shown only while plan mode is on, with the
/// exit button that returns to the previous mode.
fn plan_chip(ui: &mut egui::Ui, store: &Store, events: &mut Vec<ToolbarEvent>) -> Option<()> {
    if !store.plan_on() {
        return Some(());
    }
    let palette = Palette::active_of(ui.ctx());
    mode_chip(
        ui,
        palette,
        Icon::PenLine,
        "Plan",
        palette.accent,
        palette.accent_soft,
    );
    if ui
        .add(
            egui::Button::new(
                egui::RichText::new("\u{2715}")
                    .size(FS_2XS)
                    .color(palette.accent),
            )
            .fill(egui::Color32::TRANSPARENT)
            .corner_radius(egui::CornerRadius::same(R_FULL as u8)),
        )
        .clicked()
    {
        events.push(ToolbarEvent::TogglePlan);
    }
    Some(())
}

/// The swarm mode chip, shown only while swarm mode is on.
fn swarm_chip(ui: &mut egui::Ui, store: &Store, events: &mut Vec<ToolbarEvent>) -> Option<()> {
    if !store.swarm_on() {
        return Some(());
    }
    let palette = Palette::active_of(ui.ctx());
    mode_chip(
        ui,
        palette,
        Icon::Waypoints,
        "Swarm",
        palette.done,
        palette.done_soft,
    );
    if ui
        .add(
            egui::Button::new(
                egui::RichText::new("\u{2715}")
                    .size(FS_2XS)
                    .color(palette.done),
            )
            .fill(egui::Color32::TRANSPARENT)
            .corner_radius(egui::CornerRadius::same(R_FULL as u8)),
        )
        .clicked()
    {
        events.push(ToolbarEvent::SwarmOff);
    }
    Some(())
}

/// One active-mode chip: the colored pill with its exit button.
fn mode_chip(
    ui: &mut egui::Ui,
    _palette: &Palette,
    icon: Icon,
    label: &str,
    fg: egui::Color32,
    bg: egui::Color32,
) -> egui::Response {
    let text = egui::RichText::new(label)
        .size(FS_XS)
        .family(family(FAMILY_BODY_SEMIBOLD))
        .color(fg);
    egui::Frame::NONE
        .fill(bg)
        .corner_radius(egui::CornerRadius::same(R_FULL as u8))
        .inner_margin(egui::Margin::symmetric(9, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                crate::icons::icon(ui, icon, 12.0, fg);
                ui.label(text);
            });
        })
        .response
}

/// The model and thinking pill: one control naming the model's label
/// and the thinking level it runs at. Its menu lists the catalog's
/// models and the thinking levels the agent advertised.
fn model_button(
    ui: &mut egui::Ui,
    toolbar: &mut Toolbar,
    store: &Store,
    events: &mut Vec<ToolbarEvent>,
) -> Option<()> {
    let palette = Palette::active_of(ui.ctx());
    let open = toolbar.picker() == Some(Picker::Model);
    let model = store.composer_option("model");
    let thinking = store.composer_option("thinking");
    if model.is_none() && thinking.is_none() {
        dimmed_pill(
            ui,
            "Model",
            palette,
            "the agent sent no model or thinking config option",
        );
        return Some(());
    }
    let label = model_label(store);
    let level = thinking_label(store);
    // The level label trails a middle dot and drops when thinking is
    // off, as the web client's label does.
    let text = match &level {
        Some(level) => format!("{label} \u{b7} {level}"),
        None => label,
    };
    let ink = if level.is_some() {
        palette.ink
    } else {
        palette.muted
    };
    let response = pill(ui, open, palette, |ui| {
        ui.label(
            egui::RichText::new(text)
                .size(FS_XS + 1.0)
                .family(family(FAMILY_BODY_MEDIUM))
                .color(ink),
        );
        crate::icons::icon(ui, Icon::ChevronDown, 12.0, palette.faint);
    });
    if response.clicked() {
        toolbar.toggle_picker(Picker::Model);
    }
    if !open {
        return Some(());
    }
    let starred: Vec<String> = store.prefs().starred_models.iter().cloned().collect();
    let mut picked_model: Option<String> = None;
    let mut picked_thinking: Option<String> = None;
    egui::Window::new("model-picker")
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .fixed_pos(response.rect.left_bottom() + egui::vec2(0.0, SP_2))
        .fixed_size(egui::vec2(MODEL_W + 80.0, 0.0))
        .order(egui::Order::Foreground)
        .frame(egui::Frame::NONE)
        .show(ui.ctx(), |ui| {
            surface(ui, palette, MODEL_W + 80.0, |ui| {
                ui.set_max_height(420.0);
                let query = ui.add(
                    egui::TextEdit::singleline(toolbar.model_query())
                        .hint_text("Search models")
                        .desired_width(MODEL_W + 60.0)
                        .id(egui::Id::new("model-picker-query")),
                );
                if query.gained_focus() {
                    toolbar.model_query().clear();
                }
                let fallback = store
                    .composer_option("model")
                    .map(|option| option.options)
                    .unwrap_or_default();
                let sections = pickers::model_sections(
                    store.models(),
                    Some(fallback),
                    &starred,
                    &toolbar.model_query().clone(),
                );
                let rows = pickers::flat_rows(&sections);
                let total = rows.len();
                let highlight = toolbar.model_highlight();
                for section in &sections {
                    section_label(ui, &section.label, palette);
                    for row in &section.rows {
                        let index = rows
                            .iter()
                            .position(|candidate| candidate.id == row.id)
                            .unwrap_or(0);
                        let selected = index == highlight;
                        let response = egui::Frame::NONE
                            .fill(if selected {
                                palette.selected
                            } else {
                                egui::Color32::TRANSPARENT
                            })
                            .corner_radius(egui::CornerRadius::same(R_MD as u8))
                            .inner_margin(egui::Margin::symmetric(9, 7))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 10.0;
                                    let tile = pickers::provider_tile(&row.provider);
                                    egui::Frame::NONE
                                        .fill(palette.fill)
                                        .corner_radius(egui::CornerRadius::same(6))
                                        .inner_margin(egui::Margin::symmetric(4, 3))
                                        .show(ui, |ui| {
                                            ui.label(
                                                egui::RichText::new(tile)
                                                    .size(10.0)
                                                    .family(family(FAMILY_MONO_REGULAR))
                                                    .color(palette.muted),
                                            );
                                        });
                                    ui.vertical(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                egui::RichText::new(row.name.clone())
                                                    .size(FS_XS + 1.0)
                                                    .color(palette.ink),
                                            );
                                            if let Some(context) = row.context {
                                                chip(ui, &pickers::window_size(context), palette);
                                            }
                                            if row.images {
                                                chip(ui, "images", palette);
                                            }
                                        });
                                        ui.label(
                                            egui::RichText::new(row.model.clone())
                                                .size(11.5)
                                                .family(family(FAMILY_MONO_REGULAR))
                                                .color(palette.faint),
                                        );
                                    });
                                    if let Some((input, output)) = row.cost {
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.label(
                                                    egui::RichText::new(format!(
                                                        "${input:.1}/${output:.1}"
                                                    ))
                                                    .size(10.5)
                                                    .family(family(FAMILY_MONO_REGULAR))
                                                    .color(palette.faint),
                                                );
                                            },
                                        );
                                    }
                                });
                            })
                            .response;
                        if response.clicked() {
                            picked_model = Some(row.id.clone());
                            toolbar.close_pickers();
                        }
                    }
                }
                if total == 0 {
                    ui.label(
                        egui::RichText::new("no models match")
                            .size(FS_XS)
                            .color(palette.faint),
                    );
                }
                if let Some(thinking) = store.composer_option("thinking") {
                    section_label(ui, "Thinking", palette);
                    let levels = thinking_rows_for(store);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = SP_2;
                        for level in &levels {
                            let selected = level.value == thinking.current_value;
                            let response = egui::Frame::NONE
                                .fill(if selected {
                                    palette.accent_soft
                                } else {
                                    palette.fill
                                })
                                .corner_radius(egui::CornerRadius::same(R_FULL as u8))
                                .inner_margin(egui::Margin::symmetric(9, 4))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(pickers::level_label(level))
                                            .size(FS_XS)
                                            .color(if selected {
                                                palette.accent
                                            } else {
                                                palette.muted
                                            }),
                                    );
                                })
                                .response;
                            if response.clicked() {
                                picked_thinking = Some(level.value.clone());
                            }
                        }
                    });
                }
            });
        });
    let _ = events;
    Some(())
}

/// Compact, offered once the context passes the 80 percent the engine
/// compacts at, while no turn runs.
fn compact_button(
    ui: &mut egui::Ui,
    store: &Store,
    running: bool,
    events: &mut Vec<ToolbarEvent>,
) -> Option<()> {
    let session = store.active_session()?;
    if !pickers::compacts_soon(session.usage.used, session.usage.size) || running {
        return Some(());
    }
    let palette = Palette::active_of(ui.ctx());
    let response = pill(ui, false, palette, |ui| {
        crate::icons::icon(ui, crate::icons::Icon::Layers, 14.0, palette.warn);
        ui.label(
            egui::RichText::new("Compact")
                .size(FS_XS + 1.0)
                .color(palette.warn),
        );
    });
    if response.clicked() {
        events.push(ToolbarEvent::Compact);
    }
    let _ = response.on_hover_text("Summarize older turns to free context");
    Some(())
}

/// The context ring with its percent. The ring turns warn at 80
/// percent, where the engine compacts on its own.
fn context_ring(
    ui: &mut egui::Ui,
    toolbar: &mut Toolbar,
    store: &Store,
    events: &mut Vec<ToolbarEvent>,
) -> Option<()> {
    let session = store.active_session()?;
    let palette = Palette::active_of(ui.ctx());
    let (used, size) = (session.usage.used, session.usage.size);
    let fill = pickers::fill_fraction(used, size);
    let percent = pickers::fill_percent(used, size);
    let hot = pickers::compacts_soon(used, size);
    let ring = if hot { palette.warn } else { palette.accent };
    let response = pill(ui, false, palette, |ui| {
        ui.label(
            egui::RichText::new(format!("{percent}%"))
                .size(FS_2XS)
                .family(family(FAMILY_MONO_REGULAR))
                .color(palette.faint),
        );
    });
    let rect = response.rect;
    let center = rect.left_center() + egui::vec2(9.0, 0.0);
    let radius = 9.0;
    ui.painter()
        .circle_stroke(center, radius, egui::Stroke::new(2.0, palette.fill_hover));
    let points: Vec<egui::Pos2> = (0..=32)
        .map(|step| {
            let angle = std::f64::consts::TAU * step as f64 / 32.0 - std::f64::consts::FRAC_PI_2;
            egui::pos2(
                center.x + radius * angle.cos() as f32,
                center.y + radius * angle.sin() as f32,
            )
        })
        .collect();
    let lit = (fill * 32.0).round() as usize;
    ui.painter().add(egui::Shape::line(
        points[..=lit.min(32)].to_vec(),
        egui::Stroke::new(2.0, ring),
    ));
    if response.clicked() {
        toolbar.toggle_gauge();
    }
    let tip = if size > 0 {
        format!(
            "Context: {} of {} tokens ({percent}%)",
            pickers::tokens(used),
            pickers::tokens(size)
        )
    } else {
        "Context: the agent sent no usage yet".to_owned()
    };
    if !toolbar.gauge_open() {
        return Some(());
    }
    let turns = session
        .items
        .iter()
        .filter(|item| matches!(item, kage_client::TranscriptItem::User { .. }))
        .count();
    let cost = store
        .active_id()
        .and_then(|id| store.tree_cost(id))
        .map(|cost| format!("{} {:.2}", cost.currency, cost.amount))
        .unwrap_or_else(|| "unknown".to_owned());
    let model = pickers::select_option(session, "model").and_then(|option| {
        option
            .options
            .iter()
            .find(|choice| choice.value == option.current_value)
            .map(|choice| choice.name.clone())
    });
    egui::Window::new("context-gauge")
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .fixed_pos(response.rect.left_bottom() + egui::vec2(0.0, SP_2))
        .fixed_size(egui::vec2(MODEL_W, 0.0))
        .order(egui::Order::Foreground)
        .frame(egui::Frame::NONE)
        .show(ui.ctx(), |ui| {
            surface(ui, palette, MODEL_W, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Context")
                            .size(FS_XS + 1.0)
                            .family(family(FAMILY_BODY_SEMIBOLD))
                            .color(palette.ink_strong),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "{} / {}",
                                pickers::tokens(used),
                                pickers::tokens(size)
                            ))
                            .size(FS_XS)
                            .family(family(FAMILY_MONO_REGULAR))
                            .color(palette.muted),
                        );
                    });
                    let (bg, fg) = if hot {
                        (palette.warn_soft, palette.warn)
                    } else {
                        (palette.fill, palette.muted)
                    };
                    egui::Frame::NONE
                        .fill(bg)
                        .corner_radius(egui::CornerRadius::same(R_FULL as u8))
                        .inner_margin(egui::Margin::symmetric(7, 3))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(format!("{percent}%"))
                                    .size(FS_2XS)
                                    .color(fg),
                            );
                        });
                });
                ui.add_space(SP_2);
                gauge_cells(ui, fill, hot, palette);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("0").size(FS_2XS).color(palette.faint));
                    ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new("compacts at 80%")
                                .size(FS_2XS)
                                .color(palette.faint),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(pickers::tokens(size))
                                .size(FS_2XS)
                                .color(palette.faint),
                        );
                    });
                });
                ui.add_space(SP_4);
                egui::Frame::NONE
                    .stroke(egui::Stroke::new(1.0, palette.subtle))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            gauge_stat(ui, "Spent", cost, palette);
                            gauge_stat(
                                ui,
                                "Model",
                                model.unwrap_or_else(|| "unknown".to_owned()),
                                palette,
                            );
                            gauge_stat(ui, "Turns", turns.to_string(), palette);
                        });
                    });
                ui.add_space(SP_2);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(
                            "The engine compacts on its own at 80%, summarizing older turns.",
                        )
                        .size(FS_2XS)
                        .color(palette.faint),
                    );
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new("Compact now").size(FS_2XS),
                        ))
                        .clicked()
                    {
                        events.push(ToolbarEvent::Compact);
                    }
                });
            });
        });
    let _ = response.on_hover_text(tip);
    Some(())
}

/// The twenty cells the gauge draws, the sixteenth marked as the
/// compaction point.
fn gauge_cells(ui: &mut egui::Ui, fill: f64, hot: bool, palette: &Palette) {
    let width = ui.available_width();
    let gap = 3.0;
    let cell = (width - gap * 19.0) / 20.0;
    let height = 14.0;
    let (rect, _) = ui.allocate_at_least(egui::vec2(width, height), egui::Sense::hover());
    for (index, lit) in pickers::gauge_cells(fill) {
        let x = rect.left() + index as f32 * (cell + gap);
        let cell_rect =
            egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(cell, height));
        let color = match (lit, hot) {
            (false, _) => palette.fill_hover,
            (true, true) => palette.warn,
            (true, false) => palette.accent,
        };
        ui.painter()
            .rect_filled(cell_rect, egui::CornerRadius::same(3), color);
        if index == pickers::COMPACT_CELL {
            ui.painter().vline(
                x,
                egui::Rangef::new(rect.top(), rect.bottom()),
                egui::Stroke::new(2.0, palette.ink_strong),
            );
        }
    }
}

/// One gauge stat: a faint label over its value.
fn gauge_stat(ui: &mut egui::Ui, label: &str, value: String, palette: &Palette) {
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(label).size(FS_2XS).color(palette.faint));
        ui.label(
            egui::RichText::new(value)
                .size(FS_XS + 1.0)
                .color(palette.ink),
        );
    });
}

/// The send and stop control: a 32px circle that interrupts while a
/// run is in flight and the input is empty, and sends otherwise.
fn send_button(
    ui: &mut egui::Ui,
    store: &Store,
    running: bool,
    has_text: bool,
    events: &mut Vec<ToolbarEvent>,
) -> Option<()> {
    let palette = Palette::active_of(ui.ctx());
    if running && !has_text {
        let response = ui.add(
            egui::Button::new(
                egui::RichText::new("\u{25A0}")
                    .size(14.0)
                    .color(palette.ink),
            )
            .fill(palette.fill_hover)
            .corner_radius(egui::CornerRadius::same(R_FULL as u8))
            .min_size(egui::vec2(32.0, 32.0)),
        );
        if response.clicked() {
            events.push(ToolbarEvent::Cancel);
        }
        let _ = response.on_hover_text("Interrupt (Esc Esc)");
        return Some(());
    }
    let (bg, fg) = if has_text {
        (palette.send_bg, palette.send_icon)
    } else {
        (palette.send_bg_off, palette.send_icon_off)
    };
    let response = ui.add(
        egui::Button::new(egui::RichText::new("\u{2191}").size(14.0).color(fg))
            .fill(bg)
            .corner_radius(egui::CornerRadius::same(R_FULL as u8))
            .min_size(egui::vec2(32.0, 32.0)),
    );
    if response.clicked() && has_text {
        events.push(ToolbarEvent::Submit);
    }
    let _ = response.on_hover_text(if running {
        "Queue (Enter) or steer (Ctrl+Enter)"
    } else {
        "Send (Enter)"
    });

    let _ = store;
    Some(())
}

/// The line under the composer: the key help as key caps, or the
/// interrupt hint, with the session cost at the far end.
pub fn hint_line(ui: &mut egui::Ui, running: bool, enter_sends: bool, steer: bool, cost: &str) {
    let palette = Palette::active_of(ui.ctx());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = SP_2;
        if !enter_sends {
            kbd(ui, "Ctrl Enter", palette);
            ui.label(
                egui::RichText::new(if running { "queue" } else { "send" })
                    .size(FS_2XS)
                    .color(palette.faint),
            );
            kbd(ui, "Enter", palette);
            ui.label(
                egui::RichText::new("newline")
                    .size(FS_2XS)
                    .color(palette.faint),
            );
        } else if running {
            kbd(ui, "Enter", palette);
            ui.label(
                egui::RichText::new("queue")
                    .size(FS_2XS)
                    .color(palette.faint),
            );
            if steer {
                kbd(ui, "Ctrl Enter", palette);
                ui.label(
                    egui::RichText::new("steer")
                        .size(FS_2XS)
                        .color(palette.faint),
                );
            } else {
                ui.label(
                    egui::RichText::new("steering not advertised")
                        .size(FS_2XS)
                        .color(palette.faint),
                );
            }
            kbd(ui, "Esc Esc", palette);
            ui.label(
                egui::RichText::new("interrupt")
                    .size(FS_2XS)
                    .color(palette.faint),
            );
        } else {
            kbd(ui, "Enter", palette);
            ui.label(
                egui::RichText::new("send")
                    .size(FS_2XS)
                    .color(palette.faint),
            );
            kbd(ui, "Shift Enter", palette);
            ui.label(
                egui::RichText::new("newline")
                    .size(FS_2XS)
                    .color(palette.faint),
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !cost.is_empty() {
                ui.label(egui::RichText::new(cost).size(FS_2XS).color(palette.faint));
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::{Toolbar, mode_label, model_label, thinking_label};
    use crate::ui::pickers;

    #[test]
    fn the_toolbar_opens_one_picker_at_a_time() {
        let mut toolbar = Toolbar::new();
        assert_eq!(toolbar.picker(), None);
        toolbar.toggle_picker(super::Picker::Mode);
        assert_eq!(toolbar.picker(), Some(super::Picker::Mode));
        toolbar.toggle_picker(super::Picker::Model);
        assert_eq!(toolbar.picker(), Some(super::Picker::Model));
        toolbar.toggle_picker(super::Picker::Model);
        assert_eq!(toolbar.picker(), None, "the same pill closes it");
        toolbar.toggle_picker(super::Picker::Mode);
        toolbar.close_pickers();
        assert_eq!(toolbar.picker(), None);
        assert!(!toolbar.any_open());
    }

    #[test]
    fn the_plus_and_gauge_popovers_toggle() {
        let mut toolbar = Toolbar::new();
        toolbar.toggle_plus();
        assert!(toolbar.plus_open());
        assert!(toolbar.any_open());
        toolbar.close_plus();
        assert!(!toolbar.plus_open());
        toolbar.toggle_gauge();
        assert!(toolbar.gauge_open());
        assert!(toolbar.any_open());
    }

    #[test]
    fn the_highlight_steps_and_clamps() {
        let mut toolbar = Toolbar::new();
        toolbar.step_mode(1, 3);
        assert_eq!(toolbar.mode_highlight(), 1);
        toolbar.step_mode(5, 3);
        assert_eq!(toolbar.mode_highlight(), 2, "the last row stays");
        toolbar.step_mode(-1, 3);
        assert_eq!(toolbar.mode_highlight(), 1);
        toolbar.step_model(1, 0);
        assert_eq!(toolbar.model_highlight(), 0, "no rows, no highlight");
    }

    #[test]
    fn the_labels_name_the_active_values() {
        let store = crate::store::Store::new("/w", false);
        assert_eq!(
            model_label(&store),
            "default",
            "no option, the default word"
        );
        assert_eq!(thinking_label(&store), None, "no option, no level");
        assert_eq!(mode_label(&store, "ask"), "ask", "no option, the raw value");
        assert_eq!(pickers::session_goal(None), None);
    }
}
