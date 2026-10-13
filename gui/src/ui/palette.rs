//! The command palette: a modal over the action registry, the agent's
//! commands and the session directory.
//!
//! The modal is a plain state machine (open, query, selected row) over
//! pure row building: [`app_actions`] names what the shell can carry
//! out, [`palette_entries`] narrows the agent's commands and the
//! sessions, and [`Registry::rows`] puts both under one query.
//!
//! The shell's key router owns the nav keys while the palette is open:
//! up and down call [`Palette::move_selection`], Enter calls
//! [`Palette::run`] and Escape calls [`Palette::close`]. The query
//! field inside the modal takes egui's focus, so typing lands in it.

use kage_client::Session;
use serde_json::Value;

use crate::theme::{
    FS_2XS, FS_SM, FS_XS, Palette as Theme, R_MD, R_XL, SP_2, SP_3, SP_4, SP_5, SP_6,
};

/// The dialog's width, the old palette's 620px.
const DIALOG_W: f32 = 620.0;

/// The list's height cap, the old palette's 420px.
const LIST_MAX_H: f32 = 420.0;

/// The rows an empty query lists before the rest give way.
const RECENT_CAP: usize = 5;

/// The rows a query lists, a little wider than the empty list.
const QUERY_CAP: usize = 8;

/// An app command the palette offers. The shell matches on it; this
/// module only names the commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Show the welcome pane for a fresh session.
    NewSession,
    /// Turn swarm mode on or off.
    Swarm,
    /// Turn plan mode on or off.
    Plan,
    /// Set or clear the goal.
    Goal,
    /// Show or hide the workbench.
    Workbench,
    /// Show or hide the sidebar.
    Sidebar,
    /// Show the agents list.
    Agents,
    /// Show the files the session changed.
    Changes,
    /// Open the settings.
    Settings,
    /// Open the theme cards.
    Theme,
}

/// One command palette row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteEntry {
    /// An app command.
    Action {
        /// What runs.
        action: Action,
        /// The row text.
        label: String,
        /// Its shortcut, when it has one.
        keys: Option<&'static str>,
    },
    /// An agent command; running it fills the composer draft with the
    /// command text, ready for arguments.
    Command {
        /// The command name, without the leading slash.
        name: String,
        /// What the agent described the command as.
        description: String,
    },
    /// A session; running it follows the session.
    Session {
        /// The session id.
        id: String,
        /// The display title, when the agent set one.
        title: Option<String>,
        /// Whether the connection holds the session open.
        open: bool,
    },
}

impl PaletteEntry {
    /// The text the row renders and the query narrows over.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Action { label, .. } => label.clone(),
            Self::Command { name, .. } => format!("/{name}"),
            Self::Session { title, .. } => title
                .clone()
                .unwrap_or_else(|| "untitled session".to_owned()),
        }
    }

    /// The kind word the row carries.
    #[must_use]
    pub fn badge(&self) -> &'static str {
        match self {
            Self::Action { .. } | Self::Command { .. } => "command",
            Self::Session { open: true, .. } => "session",
            Self::Session { open: false, .. } => "recorded",
        }
    }

    /// The detail line a row carries beside its label.
    #[must_use]
    pub fn detail(&self) -> String {
        match self {
            Self::Action { .. } => String::new(),
            Self::Command { description, .. } => description.clone(),
            Self::Session { id, .. } => id.clone(),
        }
    }
}

/// The app commands for a palette over `session`: the session ones
/// only while a session shows, named for the state they flip.
#[must_use]
pub fn app_actions(session: Option<&Session>, plan_on: bool) -> Vec<PaletteEntry> {
    let action = |action, label: &str, keys: Option<&'static str>| PaletteEntry::Action {
        action,
        label: label.to_owned(),
        keys,
    };
    let mut out = vec![action(Action::NewSession, "New session", Some("Ctrl N"))];
    if let Some(session) = session {
        let swarm = session
            .config_options
            .iter()
            .any(|option| option.id == "swarm" && option.current_value == "on");
        out.push(action(
            Action::Swarm,
            if swarm {
                "Turn swarm mode off"
            } else {
                "Turn swarm mode on"
            },
            None,
        ));
        out.push(action(
            Action::Plan,
            if plan_on {
                "Turn plan mode off"
            } else {
                "Turn plan mode on"
            },
            None,
        ));
        out.push(action(Action::Goal, "Set goal", None));
    }
    out.push(action(
        Action::Workbench,
        "Toggle workbench",
        Some("Ctrl B"),
    ));
    out.push(action(Action::Sidebar, "Toggle sidebar", Some("Ctrl \\")));
    if session.is_some() {
        out.push(action(Action::Agents, "Show agents", None));
        out.push(action(Action::Changes, "Review changes", None));
    }
    out.push(action(Action::Settings, "Settings", Some("Ctrl ,")));
    out.push(action(Action::Theme, "Change theme", None));
    out
}

/// The palette rows for the agent's commands and the session
/// directory, narrowed by `query` case insensitively. Commands come
/// first, then open sessions, then recorded ones. A session narrows
/// over its title and its id.
#[must_use]
pub fn palette_entries(
    commands: &[Value],
    sessions: &[(String, Option<String>, bool)],
    query: &str,
) -> Vec<PaletteEntry> {
    let needle = query.to_lowercase();
    let mut entries = Vec::new();
    for command in commands {
        let Some(name) = command.get("name").and_then(Value::as_str) else {
            continue;
        };
        let entry = PaletteEntry::Command {
            name: name.to_owned(),
            description: command
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        };
        if needle.is_empty() || entry.label().to_lowercase().contains(&needle) {
            entries.push(entry);
        }
    }
    for (id, title, open) in sessions {
        let entry = PaletteEntry::Session {
            id: id.clone(),
            title: title.clone(),
            open: *open,
        };
        let hit =
            entry.label().to_lowercase().contains(&needle) || id.to_lowercase().contains(&needle);
        if needle.is_empty() || hit {
            entries.push(entry);
        }
    }
    entries
}

/// What the palette filters over, read off the store each frame.
#[derive(Debug, Clone, Copy)]
pub struct Registry<'a> {
    /// The active session, when one shows.
    pub session: Option<&'a Session>,
    /// Whether plan mode is on for the active session.
    pub plan_on: bool,
    /// The agent's commands, raw as the wire carried them.
    pub commands: &'a [Value],
    /// The sessions to offer: id, display title, and whether the
    /// connection holds the session open.
    pub sessions: &'a [(String, Option<String>, bool)],
}

impl Registry<'_> {
    /// The rows for `query`: the app commands first, then the agent's
    /// commands and the sessions, narrowed case insensitively. Recent
    /// sessions are a short list; a query widens it a little.
    #[must_use]
    pub fn rows(&self, query: &str) -> Vec<PaletteEntry> {
        let needle = query.to_lowercase();
        let mut entries: Vec<PaletteEntry> = app_actions(self.session, self.plan_on)
            .into_iter()
            .filter(|entry| needle.is_empty() || entry.label().to_lowercase().contains(&needle))
            .collect();
        let cap = if needle.is_empty() {
            RECENT_CAP
        } else {
            QUERY_CAP
        };
        let mut found = palette_entries(self.commands, self.sessions, query);
        let commands = found
            .iter()
            .take_while(|entry| matches!(entry, PaletteEntry::Command { .. }))
            .count();
        found.truncate(commands + cap);
        entries.extend(found);
        entries
    }
}

/// The registry a store offers, owned so the palette's rows can be
/// built once per frame without borrowing the store across the draw.
#[derive(Debug, Clone, Default)]
pub struct RegistryOwned {
    /// The active session, when one shows.
    pub session: Option<Session>,
    /// Whether plan mode is on for the active session.
    pub plan_on: bool,
    /// The agent's commands, raw as the wire carried them.
    pub commands: Option<Vec<Value>>,
    /// The sessions to offer: id, display title, and whether the
    /// connection holds the session open.
    pub sessions: Vec<(String, Option<String>, bool)>,
}

impl RegistryOwned {
    /// The registry a store offers: the active session, its plan
    /// state, its commands and every session's title.
    #[must_use]
    pub fn of(store: &crate::store::Store) -> RegistryOwned {
        let session = store.active_session();
        let plan_on = session.is_some_and(|session| {
            crate::modes::active_mode(session).is_some_and(|mode| mode == crate::modes::PLAN_MODE)
        });
        let commands = session.map(|session| session.commands.clone());
        let sessions = store
            .state()
            .sessions
            .iter()
            .map(|(id, session)| (id.clone(), session.title.clone(), !session.items.is_empty()))
            .collect();
        RegistryOwned {
            session: session.cloned(),
            plan_on,
            commands,
            sessions,
        }
    }

    /// The borrowed view the rows are built from.
    #[must_use]
    pub fn view(&self) -> Registry<'_> {
        Registry {
            session: self.session.as_ref(),
            plan_on: self.plan_on,
            commands: self.commands.as_deref().unwrap_or_default(),
            sessions: &self.sessions,
        }
    }
}

/// The command palette: a modal over the action registry, the agent's
/// commands and the session directory. The query narrows, the arrows
/// move, Enter runs and Esc closes.
#[derive(Debug, Clone, Default)]
pub struct Palette {
    /// Whether the modal shows.
    open: bool,
    /// The query the rows narrow over.
    query: String,
    /// The highlighted row index.
    selected: usize,
}

impl Palette {
    /// A closed palette over an empty query.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the palette shows.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens the palette over a fresh query with the first row
    /// highlighted.
    pub fn open(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
    }

    /// Closes the palette.
    pub fn close(&mut self) {
        self.open = false;
    }

    /// The query the rows narrow over.
    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Replaces the query, which returns the highlight to the first
    /// row the way a keystroke in the query field did.
    pub fn set_query(&mut self, query: impl Into<String>) {
        self.query = query.into();
        self.selected = 0;
    }

    /// The highlighted row index.
    #[must_use]
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Moves the highlight by `delta` over `total` rows, wrapping at
    /// the ends. No rows leaves it where it is.
    pub fn move_selection(&mut self, delta: isize, total: usize) {
        if total == 0 {
            return;
        }
        let total = total as isize;
        let next = self.selected as isize + delta;
        self.selected = (((next % total) + total) % total) as usize;
    }

    /// Clamps the highlight to the row count, dropping back to the
    /// first row when the rows shrank past it.
    pub fn clamp_selection(&mut self, total: usize) {
        if self.selected >= total {
            self.selected = 0;
        }
    }

    /// Runs the highlighted row: the palette closes and the row comes
    /// back for the shell to carry out. `None` when there are no rows.
    pub fn run(&mut self, rows: &[PaletteEntry]) -> Option<PaletteEntry> {
        self.clamp_selection(rows.len());
        let entry = rows.get(self.selected).cloned()?;
        self.close();
        Some(entry)
    }

    /// Draws the modal over `rows`, centered with a dimmed backdrop.
    /// A click on a row runs it and the row comes back for the shell
    /// to carry out; a click on the backdrop closes the palette.
    pub fn show(&mut self, ui: &mut egui::Ui, rows: &[PaletteEntry]) -> Option<PaletteEntry> {
        self.clamp_selection(rows.len());
        let palette = Theme::active_of(ui.ctx());
        let selected = self.selected;
        let mut run = None;
        let mut backdrop_clicked = false;
        let mut query_before = self.query.clone();
        egui::Area::new(egui::Id::new("kage-palette"))
            .anchor(egui::Align2::LEFT_TOP, egui::Vec2::ZERO)
            .order(egui::Order::Foreground)
            .interactable(true)
            .movable(false)
            .show(ui.ctx(), |ui| {
                let screen = ui.ctx().viewport_rect();
                ui.set_min_size(screen.size());
                let backdrop = ui.allocate_response(screen.size(), egui::Sense::click());
                ui.painter().rect_filled(
                    backdrop.rect,
                    egui::CornerRadius::ZERO,
                    egui::Color32::from_black_alpha(115),
                );
                if backdrop.clicked() {
                    backdrop_clicked = true;
                }
                let dialog = egui::Rect::from_min_size(
                    egui::pos2((screen.width() - DIALOG_W) / 2.0, screen.height() * 0.12),
                    egui::vec2(DIALOG_W, LIST_MAX_H + 96.0),
                );
                ui.scope_builder(
                    egui::UiBuilder::new().max_rect(dialog),
                    |ui| {
                        let frame = egui::Frame::NONE
                            .fill(palette.bg)
                            .stroke(egui::Stroke::new(1.0, palette.line))
                            .inner_margin(egui::Margin::same(0))
                            .corner_radius(egui::CornerRadius::same(R_XL as u8))
                            .shadow(palette.shadow_2);
                        frame.show(ui, |ui| {
                            ui.set_max_width(DIALOG_W);
                            ui.vertical(|ui| {
                            // The query field, focused so typing lands
                            // in it.
                            let field = egui::TextEdit::singleline(&mut self.query)
                                .id(egui::Id::new("kage-palette-query"))
                                .hint_text("Run a command or switch session")
                                .text_color(palette.ink_strong)
                                .font(egui::FontSelection::FontId(egui::FontId::new(
                                    15.0,
                                    crate::theme::family(crate::theme::FAMILY_BODY_REGULAR),
                                )))
                                .frame(egui::Frame::NONE)
                                .margin(egui::Margin::symmetric(SP_6 as i8, SP_5 as i8))
                                .desired_width(DIALOG_W);
                            let response = ui.add(field);
                            response.request_focus();
                            ui.separator();
                            // The rows, capped in height and scrolled.
                            egui::ScrollArea::vertical()
                                .max_height(LIST_MAX_H)
                                .show(ui, |ui| {
                                    ui.set_max_width(DIALOG_W);
                                    if rows.is_empty() {
                                        ui.label(
                                            egui::RichText::new("Nothing matches")
                                                .size(FS_SM)
                                                .color(palette.faint),
                                        );
                                    }
                                    let mut last_kind = "";
                                    for (index, entry) in rows.iter().enumerate() {
                                        let kind = match entry {
                                            PaletteEntry::Action { .. }
                                            | PaletteEntry::Command { .. } => "command",
                                            PaletteEntry::Session { .. } => "session",
                                        };
                                        if kind != last_kind {
                                            last_kind = kind;
                                            let label = match kind {
                                                "session" if self.query.trim().is_empty() => {
                                                    "RECENT SESSIONS"
                                                }
                                                "session" => "SESSIONS",
                                                _ => "COMMANDS",
                                            };
                                            ui.add_space(SP_3);
                                            ui.label(
                                                egui::RichText::new(label)
                                                    .size(FS_2XS)
                                                    .color(palette.faint),
                                            );
                                        }
                                        let (icon, mono_label) = match entry {
                                            PaletteEntry::Action { action, .. } => {
                                                (action_icon(*action), false)
                                            }
                                            PaletteEntry::Command { .. } => {
                                                (crate::icons::Icon::Command, true)
                                            }
                                            PaletteEntry::Session { .. } => {
                                                (crate::icons::Icon::MessageSquare, false)
                                            }
                                        };
                                        let fill = if index == selected {
                                            palette.selected
                                        } else {
                                            palette.fill
                                        };
                                        let row = egui::Frame::NONE
                                            .fill(fill)
                                            .inner_margin(egui::Margin::symmetric(
                                                SP_4 as i8,
                                                SP_2 as i8,
                                            ))
                                            .corner_radius(egui::CornerRadius::same(R_MD as u8))
                                            .show(ui, |ui| {
                                                ui.set_max_width(DIALOG_W);
                                                ui.horizontal_top(|ui| {
                                                    crate::icons::icon(ui, icon, 14.0, palette.faint);
                                                    ui.add_space(SP_3);
                                                    ui.vertical(|ui| {
                                                        let mut text = egui::RichText::new(entry.label())
                                                            .color(palette.ink);
                                                        if mono_label {
                                                            text = text.font(egui::FontId::new(
                                                                12.5,
                                                                crate::theme::family(
                                                                    crate::theme::FAMILY_MONO_REGULAR,
                                                                ),
                                                            ));
                                                        } else {
                                                            text = text.size(FS_SM);
                                                        }
                                                        ui.label(text);
                                                        let detail = entry.detail();
                                                        if !detail.is_empty() {
                                                            ui.label(
                                                                egui::RichText::new(detail)
                                                                    .size(FS_XS)
                                                                    .color(palette.muted),
                                                            );
                                                        }
                                                    });
                                                    ui.with_layout(
                                                        egui::Layout::right_to_left(
                                                            egui::Align::Center,
                                                        ),
                                                        |ui| match entry {
                                                            PaletteEntry::Action { keys, .. } => {
                                                                match keys {
                                                                    Some(keys) => {
                                                                        kbd_chip(ui, keys, palette)
                                                                    }
                                                                    None => {
                                                                        ui.allocate_response(
                                                                            egui::Vec2::ZERO,
                                                                            egui::Sense::hover(),
                                                                        )
                                                                    }
                                                                }
                                                            }
                                                            _ => badge_chip(ui, entry.badge(), palette),
                                                        },
                                                    );
                                                });
                                            });
                                        if row.response.interact(egui::Sense::click()).clicked() {
                                            run = Some(index);
                                        }
                                    }
                                });
                            // The footer hints.
                            ui.add_space(SP_2);
                            ui.separator();
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = SP_5;
                                ui.add_space(SP_6);
                                hint(ui, "\u{2191}\u{2193}", "move", palette);
                                hint(ui, "Enter", "run", palette);
                                hint(ui, "Esc", "close", palette);
                            });
                            });
                        });
                    },
                );
            });
        if backdrop_clicked {
            self.close();
            return None;
        }
        if run.is_some() {
            return self.run(rows);
        }
        if self.query != query_before {
            query_before = std::mem::replace(&mut self.query, query_before);
            self.set_query(query_before);
        }
        None
    }
}

/// The icon an app command's row leads with.
#[must_use]
fn action_icon(action: Action) -> crate::icons::Icon {
    match action {
        Action::NewSession => crate::icons::Icon::SquarePen,
        Action::Swarm => crate::icons::Icon::Waypoints,
        Action::Plan => crate::icons::Icon::ListTodo,
        Action::Goal => crate::icons::Icon::Target,
        Action::Workbench => crate::icons::Icon::PanelRight,
        Action::Sidebar => crate::icons::Icon::PanelLeft,
        Action::Agents => crate::icons::Icon::Users,
        Action::Changes => crate::icons::Icon::FileDiff,
        Action::Settings => crate::icons::Icon::Settings,
        Action::Theme => crate::icons::Icon::Moon,
    }
}

/// The bordered keyboard hint chip of the design.
pub(crate) fn kbd_chip(ui: &mut egui::Ui, label: &str, palette: &Theme) -> egui::Response {
    egui::Frame::NONE
        .fill(palette.fill)
        .stroke(egui::Stroke::new(1.0, palette.line))
        .inner_margin(egui::Margin::symmetric(SP_2 as i8, 1))
        .corner_radius(egui::CornerRadius::same(5))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(label)
                    .monospace()
                    .size(FS_2XS)
                    .color(palette.faint),
            )
        })
        .response
}

/// The small rounded meta chip of the design's badges.
fn badge_chip(ui: &mut egui::Ui, label: &str, palette: &Theme) -> egui::Response {
    egui::Frame::NONE
        .fill(palette.fill)
        .inner_margin(egui::Margin::symmetric(SP_3 as i8, 0))
        .corner_radius(egui::CornerRadius::same(4))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(label).size(FS_2XS).color(palette.muted))
        })
        .response
}

/// One footer hint: the chip and the word beside it.
fn hint(ui: &mut egui::Ui, keys: &str, word: &str, palette: &Theme) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = SP_2;
        kbd_chip(ui, keys, palette);
        ui.label(egui::RichText::new(word).size(FS_2XS).color(palette.faint));
    });
}

#[cfg(test)]
mod tests {
    use super::{Action, Palette, PaletteEntry, app_actions, palette_entries};
    use kage_client::Session;

    /// The action rows a sessionless palette offers.
    fn sessionless_actions() -> Vec<PaletteEntry> {
        app_actions(None, false)
    }

    /// The action rows a palette over one open session offers.
    fn session_actions(swarm: bool, plan_on: bool) -> Vec<PaletteEntry> {
        let mut session = Session::new("s1");
        if swarm {
            session
                .config_options
                .push(kage_client::wire::SessionConfigOption {
                    id: "swarm".into(),
                    name: "Swarm".into(),
                    description: None,
                    category: None,
                    kind: kage_client::wire::SessionConfigKind::Select,
                    current_value: "on".into(),
                    options: Vec::new(),
                });
        }
        app_actions(Some(&session), plan_on)
    }

    #[test]
    fn app_actions_lead_with_the_ones_that_need_no_session() {
        let rows = sessionless_actions();
        assert_eq!(rows.len(), 5, "no session: the always-on commands only");
        assert_eq!(rows[0].label(), "New session");
        assert_eq!(rows[0].badge(), "command");
        assert_eq!(
            rows[0].detail(),
            "",
            "an app command carries no detail line"
        );
        assert_eq!(
            rows.last().map(PaletteEntry::label).as_deref(),
            Some("Change theme")
        );
        for row in &rows {
            assert!(
                matches!(row, PaletteEntry::Action { .. }),
                "every row is an action without a session"
            );
        }
    }

    #[test]
    fn the_session_commands_join_only_while_a_session_shows() {
        let rows = session_actions(false, false);
        assert_eq!(
            rows.len(),
            10,
            "the app commands lead, the session ones included"
        );
        let labels: Vec<String> = rows.iter().map(PaletteEntry::label).collect();
        assert_eq!(
            labels,
            [
                "New session",
                "Turn swarm mode on",
                "Turn plan mode on",
                "Set goal",
                "Toggle workbench",
                "Toggle sidebar",
                "Show agents",
                "Review changes",
                "Settings",
                "Change theme",
            ]
        );
        assert_eq!(
            session_actions(true, false)[1].label(),
            "Turn swarm mode off"
        );
        assert_eq!(
            session_actions(false, true)[2].label(),
            "Turn plan mode off"
        );
    }

    #[test]
    fn palette_entries_order_commands_then_sessions_and_narrow() {
        let commands = vec![serde_json::json!({
            "name": "review", "description": "review the diff",
        })];
        let sessions = vec![
            ("s1".to_owned(), Some("fix the null check".to_owned()), true),
            ("s2".to_owned(), None, true),
            ("rec1".to_owned(), None, false),
        ];
        let all = palette_entries(&commands, &sessions, "");
        assert_eq!(all.len(), 4);
        assert_eq!(
            all[0],
            PaletteEntry::Command {
                name: "review".into(),
                description: "review the diff".into(),
            }
        );
        assert_eq!(all[0].badge(), "command");
        assert_eq!(all[1].badge(), "session");
        assert_eq!(all[3].badge(), "recorded");
        assert_eq!(all[2].label(), "untitled session");

        let narrowed = palette_entries(&commands, &sessions, "rev");
        assert_eq!(narrowed.len(), 1);
        let by_id = palette_entries(&commands, &sessions, "s2");
        assert_eq!(by_id.len(), 1, "sessions narrow over their id too");
        assert_eq!(by_id[0].badge(), "session");
        assert!(palette_entries(&commands, &sessions, "zzz").is_empty());
    }

    #[test]
    fn a_command_row_narrows_over_its_slash_name() {
        let commands = vec![serde_json::json!({"name": "review"})];
        let found = palette_entries(&commands, &[], "/rev");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].label(), "/review");
        assert_eq!(found[0].detail(), "", "a command with no description");
    }

    #[test]
    fn the_registry_narrows_the_app_commands_with_the_rest() {
        let session = Session::new("s1");
        let registry = super::Registry {
            session: Some(&session),
            plan_on: false,
            commands: &[serde_json::json!({"name": "review", "description": "review the diff"})],
            sessions: &[
                ("s1".to_owned(), Some("fix the null check".to_owned()), true),
                ("s2".to_owned(), None, true),
            ],
        };
        let all = registry.rows("");
        assert_eq!(
            all.len(),
            13,
            "ten app commands, the command and both sessions"
        );
        assert_eq!(all[10].label(), "/review");

        let narrowed = registry.rows("rev");
        assert_eq!(narrowed.len(), 2, "the app command and the agent's");
        assert_eq!(narrowed[0].label(), "Review changes");
        assert_eq!(narrowed[1].label(), "/review");

        let by_id = registry.rows("s2");
        assert_eq!(by_id.len(), 1, "the sessions narrow over their id");
        assert!(registry.rows("zzz").is_empty());
    }

    #[test]
    fn an_empty_query_caps_the_recent_sessions_and_a_query_widens_them() {
        let session = Session::new("s1");
        let sessions: Vec<(String, Option<String>, bool)> = (0..9)
            .map(|index| (format!("s{index}"), None, true))
            .collect();
        let registry = super::Registry {
            session: Some(&session),
            plan_on: false,
            commands: &[],
            sessions: &sessions,
        };
        let recent = registry.rows("");
        assert_eq!(
            recent.len(),
            15,
            "ten app commands and five recent sessions"
        );
        let widened = registry.rows("s");
        let sessions = widened
            .iter()
            .filter(|entry| matches!(entry, PaletteEntry::Session { .. }))
            .count();
        assert_eq!(sessions, 8, "a query widens the sessions to eight");
    }

    #[test]
    fn a_closed_palette_opens_over_a_fresh_query_and_closes() {
        let mut palette = Palette::new();
        assert!(!palette.is_open());
        assert_eq!(palette.query(), "");
        assert_eq!(palette.selected(), 0);

        palette.set_query("stale");
        palette.move_selection(1, 3);
        assert_eq!(palette.selected(), 1);

        palette.open();
        assert!(palette.is_open());
        assert_eq!(palette.query(), "", "the opening query is fresh");
        assert_eq!(palette.selected(), 0, "the highlight starts at the top");

        palette.close();
        assert!(!palette.is_open());
    }

    #[test]
    fn the_highlight_moves_and_wraps_and_stays_without_rows() {
        let mut palette = Palette::new();
        palette.move_selection(1, 0);
        assert_eq!(palette.selected(), 0, "no rows, no move");

        palette.move_selection(1, 3);
        assert_eq!(palette.selected(), 1);
        palette.move_selection(1, 3);
        assert_eq!(palette.selected(), 2);
        palette.move_selection(1, 3);
        assert_eq!(palette.selected(), 0, "down wraps at the end");
        palette.move_selection(-1, 3);
        assert_eq!(palette.selected(), 2, "up wraps at the top");
    }

    #[test]
    fn typing_in_the_query_returns_the_highlight_to_the_top() {
        let mut palette = Palette::new();
        palette.open();
        palette.move_selection(2, 5);
        assert_eq!(palette.selected(), 2);
        palette.set_query("/re");
        assert_eq!(
            palette.selected(),
            0,
            "a new query re-highlights the first row"
        );
        assert_eq!(palette.query(), "/re");
    }

    #[test]
    fn a_run_takes_the_highlighted_row_and_closes() {
        let mut palette = Palette::new();
        palette.open();
        let rows = session_actions(false, false);
        palette.move_selection(1, rows.len());
        assert_eq!(palette.selected(), 1);

        let entry = palette.run(&rows).expect("the highlighted row runs");
        assert_eq!(entry.label(), "Turn swarm mode on");
        assert!(!palette.is_open(), "a run closes the palette");
    }

    #[test]
    fn a_run_over_no_rows_does_nothing() {
        let mut palette = Palette::new();
        palette.open();
        assert_eq!(palette.run(&[]), None);
        assert!(
            palette.selected() == 0,
            "no rows leaves the highlight alone"
        );
    }

    #[test]
    fn a_stale_highlight_drops_back_to_the_first_row() {
        let mut palette = Palette::new();
        palette.open();
        palette.move_selection(4, 5);
        assert_eq!(palette.selected(), 4);
        palette.clamp_selection(2);
        assert_eq!(palette.selected(), 0, "the rows shrank past the highlight");
        palette.clamp_selection(5);
        assert_eq!(palette.selected(), 0, "a clamp never raises the highlight");
    }

    #[test]
    fn every_action_row_names_a_shell_action() {
        for row in session_actions(false, false) {
            let PaletteEntry::Action { action, .. } = row else {
                panic!("every row is an app command here");
            };
            assert!(
                matches!(
                    action,
                    Action::NewSession
                        | Action::Swarm
                        | Action::Plan
                        | Action::Goal
                        | Action::Workbench
                        | Action::Sidebar
                        | Action::Agents
                        | Action::Changes
                        | Action::Settings
                        | Action::Theme
                ),
                "{action:?} is one the shell matches on"
            );
        }
    }
}
