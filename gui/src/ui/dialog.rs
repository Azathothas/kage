//! The shell's modal dialogs: setting a goal, confirming swarm mode,
//! rewinding a session and picking a folder.
//!
//! One dialog layer over the shell; it draws nothing while no dialog
//! is open. A dialog closes on Esc, on a click on the scrim, and after
//! its action. The layer sits in an egui `Area` above every panel, so
//! it occludes the shell and takes the keys: the shell's key router
//! reads the dialog's open state and answers only the shell's own
//! bindings while no modal is up.

use web_time::Instant;

use kage_client::{Session, TranscriptItem, unquote_and_trim};

use crate::store::{FoldersRead, Store};
use crate::theme::{
    FAMILY_BODY_SEMIBOLD, FAMILY_MONO_REGULAR, FS_SM, FS_XS, Palette, R_FULL, R_MD, R_XL, family,
};
use crate::ui::workbench::{ChangeEntry, change_entries, session_changes};

/// The dialogs the shell can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKind {
    /// Set or clear the session goal.
    Goal,
    /// Confirm turning swarm mode on.
    ConfirmSwarm,
    /// Rewind the active session to before the prompt at this item.
    Rewind(usize),
    /// Pick the directory the next session opens in, by path.
    OpenFolder,
}

/// What a rewind to before a prompt drops from the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewindPreview {
    /// The prompt the rewind goes back to.
    pub prompt: String,
    /// The prompts from it on.
    pub turns: usize,
    /// The transcript items from it on, turn boundaries aside.
    pub entries: usize,
    /// The tool calls among them.
    pub tools: usize,
    /// The subagents those calls started.
    pub agents: usize,
    /// The files the dropped calls and their subagents changed.
    pub files: Vec<ChangeEntry>,
}

/// What rewinding `session` to before the prompt at item `prompt`
/// drops, or `None` when that item is no prompt.
#[must_use]
pub fn rewind_preview(store: &Store, session: &Session, prompt: usize) -> Option<RewindPreview> {
    let text = session.prompt_ref(prompt)?.text.unwrap_or_default();
    let gone = &session.items[prompt..];
    let calls: Vec<&str> = gone
        .iter()
        .filter_map(|item| match item {
            TranscriptItem::ToolCall(call) => Some(call.tool_call_id.as_str()),
            _ => None,
        })
        .collect();
    let mut files = change_entries(gone);
    let mut agents = 0;
    for (id, agent) in &session.agents {
        if !agent
            .tool_call_id
            .as_deref()
            .is_some_and(|call| calls.contains(&call))
        {
            continue;
        }
        agents += 1;
        let Some(child) = store.state().session(id) else {
            continue;
        };
        for mut entry in change_entries(&child.items) {
            if let Some(known) = files.iter_mut().find(|known| known.path == entry.path) {
                known.add += entry.add;
                known.del += entry.del;
                continue;
            }
            entry.by = agent.name.clone();
            files.push(entry);
        }
    }
    Some(RewindPreview {
        prompt: text,
        turns: gone
            .iter()
            .filter(|item| matches!(item, TranscriptItem::User { .. }))
            .count(),
        entries: gone
            .iter()
            .filter(|item| !matches!(item, TranscriptItem::TurnEnd { .. }))
            .count(),
        tools: calls.len(),
        agents,
        files,
    })
}

/// A count chip of the rewind preview: the label on the soft fill.
fn count_chip(ui: &mut egui::Ui, label: &str, fg: egui::Color32, bg: egui::Color32) {
    egui::Frame::NONE
        .fill(bg)
        .corner_radius(egui::CornerRadius::same(R_FULL as u8))
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(label).size(FS_XS).color(fg));
        });
}

/// `n` and the noun, `one` when `n` is one and `many` otherwise.
#[must_use]
pub fn counted(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// `base` and `name` as one path the engine reads back, in the
/// separator style `base` uses. Trailing separators are trimmed, so
/// the join never doubles them.
#[must_use]
pub fn join_folder(base: &str, name: &str) -> String {
    let base = base.trim_end_matches(['/', '\\']);
    let separator = if base.contains('\\') { '\\' } else { '/' };
    format!("{base}{separator}{name}")
}

/// What a dialog asked of the shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogEvent {
    /// Set the session's goal to this text.
    SaveGoal(String),
    /// Turn swarm mode on, with this permission mode held.
    EnableSwarm(Option<String>),
    /// Rewind the active session to before the prompt at this item.
    Rewind(usize),
    /// Open the next session in this folder.
    SetProject(String),
    /// List the folders inside this path, or the home folder when
    /// `None`.
    Browse(Option<String>),
}

/// The dialog layer's state: which dialog is open, its field text and
/// the folder listing it asked for.
#[derive(Debug, Default)]
pub struct Dialogs {
    /// The dialog that shows, or `None` when the layer draws nothing.
    open: Option<DialogKind>,
    /// The goal field's text.
    goal: String,
    /// The folder field's text.
    folder: String,
    /// The folder listing ask in flight, until it answers.
    browsing: Option<u64>,
    /// The answer to the listing ask: the folders of the listed
    /// folder, or why they could not be listed.
    listing: Option<FoldersRead>,
}

impl Dialogs {
    /// A closed dialog layer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a dialog shows.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// The dialog that shows.
    #[must_use]
    pub const fn open(&self) -> Option<DialogKind> {
        self.open
    }

    /// Shows `kind`. Swarm mode turns on without the confirmation when
    /// the user switched that confirmation off, so the ask never
    /// appears for someone who does not want it.
    pub fn show_dialog(&mut self, kind: DialogKind, store: &mut Store) -> Vec<DialogEvent> {
        if kind == DialogKind::ConfirmSwarm && !store.prefs().confirm_swarm {
            return vec![DialogEvent::EnableSwarm(None)];
        }
        self.open = Some(kind);
        match kind {
            DialogKind::Goal => {
                let current = store.active_session().and_then(|session| {
                    session
                        .config_options
                        .iter()
                        .find(|option| option.id == "goal")
                        .map(|option| option.current_value.clone())
                });
                self.goal = current.unwrap_or_default();
            }
            DialogKind::OpenFolder => {
                self.folder = store.session_dir().unwrap_or_default().to_owned();
                self.listing = None;
                let path = store.session_dir().map(str::to_owned);
                self.browsing = Some(store.ask_folders(path.as_deref()));
            }
            DialogKind::ConfirmSwarm | DialogKind::Rewind(_) => {}
        }
        Vec::new()
    }

    /// Closes the open dialog.
    pub fn close(&mut self) {
        self.open = None;
    }

    /// Records the answer to folder listing `id`.
    pub fn answer_folders(&mut self, id: u64, read: FoldersRead) {
        if self.browsing != Some(id) {
            return;
        }
        self.browsing = None;
        if let Ok(listing) = &read {
            self.folder.clone_from(&listing.path);
        }
        self.listing = Some(read);
    }

    /// Records that folder listing `id` was asked for, so its answer
    /// settles this dialog's field.
    pub fn browse(&mut self, id: u64) {
        self.browsing = Some(id);
    }

    /// The folder listing ask in flight, when one is.
    #[must_use]
    pub const fn browsing(&self) -> Option<u64> {
        self.browsing
    }

    /// Draws the dialog layer over everything, returning what it asked
    /// for. `born` is the shell's clock, so a field's caret keeps
    /// blinking across frames.
    pub fn show(&mut self, ui: &mut egui::Ui, store: &Store, born: Instant) -> Vec<DialogEvent> {
        let Some(kind) = self.open else {
            return Vec::new();
        };
        let palette = Palette::active_of(ui.ctx());
        let mut events = Vec::new();
        let mut close = false;
        let mut ask = Ask::default();
        egui::Area::new(egui::Id::new("kage-dialog"))
            .anchor(egui::Align2::LEFT_TOP, egui::Vec2::ZERO)
            .order(egui::Order::Foreground)
            .interactable(true)
            .movable(false)
            .show(ui.ctx(), |ui| {
                let screen = ui.ctx().viewport_rect();
                ui.set_min_size(screen.size());
                let scrim = ui.allocate_response(screen.size(), egui::Sense::click());
                ui.painter().rect_filled(
                    scrim.rect,
                    egui::CornerRadius::ZERO,
                    egui::Color32::from_black_alpha(115),
                );
                if scrim.clicked() {
                    close = true;
                }
                let width = match kind {
                    DialogKind::Goal | DialogKind::OpenFolder => 480.0,
                    DialogKind::ConfirmSwarm => 460.0,
                    DialogKind::Rewind(_) => 560.0,
                };
                let card = egui::Rect::from_min_size(
                    egui::pos2((screen.width() - width) / 2.0, screen.height() * 0.12),
                    egui::vec2(width, 0.0),
                );
                ui.scope_builder(egui::UiBuilder::new().max_rect(card), |ui| {
                    let (fg, bg) = match kind {
                        DialogKind::Goal | DialogKind::Rewind(_) | DialogKind::OpenFolder => {
                            (palette.accent, palette.accent_soft)
                        }
                        DialogKind::ConfirmSwarm => (palette.done, palette.done_soft),
                    };
                    egui::Frame::NONE
                        .fill(palette.bg)
                        .stroke(egui::Stroke::new(1.0, palette.line))
                        .corner_radius(egui::CornerRadius::same(R_XL as u8))
                        .shadow(palette.shadow_2)
                        .show(ui, |ui| {
                            ui.set_max_width(width);
                            let (title, icon) = match kind {
                                DialogKind::Goal => ("Set a goal", crate::icons::Icon::Target),
                                DialogKind::ConfirmSwarm => {
                                    ("Enable swarm mode?", crate::icons::Icon::Waypoints)
                                }
                                DialogKind::Rewind(_) => {
                                    ("Rewind to this message?", crate::icons::Icon::Undo2)
                                }
                                DialogKind::OpenFolder => {
                                    ("Open a folder", crate::icons::Icon::Folder)
                                }
                            };
                            ui.horizontal(|ui| {
                                ui.add_space(18.0);
                                ui.add_space(6.0);
                                egui::Frame::NONE
                                    .fill(bg)
                                    .corner_radius(egui::CornerRadius::same(R_MD as u8))
                                    .inner_margin(egui::Margin::same(7))
                                    .show(ui, |ui| {
                                        crate::icons::icon(ui, icon, 14.0, fg);
                                    });
                                ui.add_space(10.0);
                                ui.label(
                                    egui::RichText::new(title)
                                        .size(16.0)
                                        .family(family(FAMILY_BODY_SEMIBOLD))
                                        .color(palette.ink_strong),
                                );
                            });
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.add_space(18.0);
                                ui.vertical(|ui| {
                                    ui.set_max_width(width - 36.0);
                                    self.draw_body(ui, kind, store, palette, &mut ask);
                                });
                            });
                            ui.add_space(12.0);
                        });
                });
                let _ = born;
            });
        for event in ask.events.drain(..) {
            events.push(event);
        }
        if let Some(path) = ask.browse.take() {
            events.push(DialogEvent::Browse(path));
        }
        if ask.close {
            close = true;
        }
        if close {
            self.close();
        }
        events
    }

    /// Draws the body of `kind`, filling `ask` with what it asked.
    fn draw_body(
        &mut self,
        ui: &mut egui::Ui,
        kind: DialogKind,
        store: &Store,
        palette: &Palette,
        ask: &mut Ask,
    ) {
        match kind {
            DialogKind::Goal => self.goal_body(ui, palette, ask),
            DialogKind::ConfirmSwarm => self.swarm_body(ui, store, palette, ask),
            DialogKind::Rewind(prompt) => self.rewind_body(ui, store, prompt, palette, ask),
            DialogKind::OpenFolder => self.folder_body(ui, palette, ask),
        }
    }

    /// The goal dialog: the explanation, the field and the footer.
    fn goal_body(&mut self, ui: &mut egui::Ui, palette: &Palette, ask: &mut Ask) {
        let mut text = std::mem::take(&mut self.goal);
        ui.label(
            egui::RichText::new(
                "The agent keeps working until the goal is met, checking it after each turn.",
            )
            .size(FS_SM)
            .color(palette.muted),
        );
        ui.add_space(10.0);
        let field = egui::TextEdit::singleline(&mut text)
            .hint_text("e.g. all retry tests pass three runs in a row")
            .desired_width(f32::INFINITY)
            .font(egui::FontId::new(FS_SM, family(FAMILY_BODY_SEMIBOLD)))
            .margin(egui::Margin::symmetric(10, 8));
        let response = ui.add(field);
        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            ask.events.push(DialogEvent::SaveGoal(text.clone()));
            ask.close = true;
        }
        ui.add_space(12.0);
        footer(
            ui,
            palette,
            &[("Cancel", false), ("Set goal", true)],
            &mut ask.saved,
            &mut ask.cancelled,
        );
        if ask.saved {
            ask.events.push(DialogEvent::SaveGoal(text.clone()));
            ask.close = true;
        }
        self.goal = text;
    }

    /// The swarm confirmation: what turning it on does and the
    /// permission mode it holds.
    fn swarm_body(&mut self, ui: &mut egui::Ui, store: &Store, palette: &Palette, ask: &mut Ask) {
        let mode = store
            .active_session()
            .and_then(|session| {
                session
                    .config_options
                    .iter()
                    .find(|option| option.id == "mode")
                    .map(|option| option.current_value.clone())
            })
            .unwrap_or_else(|| "default".to_owned());
        let name = store
            .active_session()
            .and_then(|session| {
                session
                    .config_options
                    .iter()
                    .find(|option| option.id == "mode")?
                    .options
                    .iter()
                    .find(|choice| choice.value == mode)
                    .map(|choice| choice.name.clone())
            })
            .unwrap_or_else(|| mode.clone());
        ui.label(
            egui::RichText::new(
                "The agent can run several workers at once. Each one asks before it edits or runs a command.",
            )
            .size(FS_SM)
            .color(palette.muted),
        );
        ui.add_space(10.0);
        egui::Frame::NONE
            .fill(palette.surface)
            .stroke(egui::Stroke::new(1.0, palette.line))
            .corner_radius(egui::CornerRadius::same(R_MD as u8))
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Permission mode")
                            .size(FS_XS)
                            .color(palette.faint),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(&name)
                                .size(FS_SM)
                                .color(palette.ink_strong),
                        );
                    });
                });
            });
        ui.add_space(12.0);
        footer(
            ui,
            palette,
            &[("Cancel", false), ("Enable swarm", true)],
            &mut ask.saved,
            &mut ask.cancelled,
        );
        if ask.saved {
            ask.events.push(DialogEvent::EnableSwarm(Some(mode)));
            ask.close = true;
        }
    }

    /// The rewind confirmation: the prompt quoted, what it drops and
    /// the files that stay.
    fn rewind_body(
        &mut self,
        ui: &mut egui::Ui,
        store: &Store,
        prompt: usize,
        palette: &Palette,
        ask: &mut Ask,
    ) {
        let session = store.active_session();
        let preview = session.and_then(|session| rewind_preview(store, session, prompt));
        let title = session
            .and_then(|session| session.title.clone())
            .unwrap_or_else(|| "untitled session".to_owned());
        if let Some(preview) = &preview {
            egui::Frame::NONE
                .fill(palette.surface)
                .stroke(egui::Stroke::new(3.0, palette.accent))
                .corner_radius(egui::CornerRadius::same(R_MD as u8))
                .inner_margin(egui::Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.set_max_width(ui.available_width());
                    ui.label(
                        egui::RichText::new(&preview.prompt)
                            .size(FS_SM)
                            .color(palette.ink_strong),
                    );
                });
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                count_chip(
                    ui,
                    &counted(preview.turns, "turn", "turns"),
                    palette.muted,
                    palette.fill,
                );
                count_chip(
                    ui,
                    &counted(preview.entries, "entry", "entries"),
                    palette.muted,
                    palette.fill,
                );
                count_chip(
                    ui,
                    &counted(preview.tools, "tool call", "tool calls"),
                    palette.muted,
                    palette.fill,
                );
                if preview.agents > 0 {
                    count_chip(
                        ui,
                        &counted(preview.agents, "agent", "agents"),
                        palette.done,
                        palette.done_soft,
                    );
                }
                if preview.files.is_empty() {
                    count_chip(ui, "0 files", palette.muted, palette.fill);
                } else {
                    count_chip(
                        ui,
                        &counted(preview.files.len(), "file", "files"),
                        palette.warn,
                        palette.warn_soft,
                    );
                }
            });
            if !preview.files.is_empty() {
                ui.add_space(8.0);
                egui::Frame::NONE
                    .stroke(egui::Stroke::new(1.0, palette.subtle))
                    .corner_radius(egui::CornerRadius::same(12))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.add_space(12.0);
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new("Files stay as they are")
                                    .size(FS_XS)
                                    .color(palette.muted),
                            );
                            ui.label(
                                egui::RichText::new(
                                    "restoring them stays with the TUI rewind plugin",
                                )
                                .size(FS_XS)
                                .color(palette.faint),
                            );
                        });
                        for file in &preview.files {
                            ui.horizontal(|ui| {
                                ui.add_space(12.0);
                                ui.add_space(8.0);
                                crate::icons::icon(
                                    ui,
                                    if file.created {
                                        crate::icons::Icon::FilePlus
                                    } else {
                                        crate::icons::Icon::File
                                    },
                                    12.0,
                                    palette.faint,
                                );
                                ui.label(
                                    egui::RichText::new(&file.path)
                                        .size(12.0)
                                        .family(family(FAMILY_MONO_REGULAR))
                                        .color(palette.ink),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.add_space(12.0);
                                        ui.label(
                                            egui::RichText::new(format!("+{}", file.add))
                                                .size(FS_XS)
                                                .color(palette.ok),
                                        );
                                        ui.label(
                                            egui::RichText::new(format!("-{}", file.del))
                                                .size(FS_XS)
                                                .color(palette.danger),
                                        );
                                    },
                                );
                            });
                        }
                    });
            }
        }
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(format!(
                "The current state stays as \"{title} (before rewind)\", and this message goes back in the composer."
            ))
            .size(13.0)
            .color(palette.muted),
        );
        ui.add_space(12.0);
        footer(
            ui,
            palette,
            &[("Cancel", false), ("Rewind", true)],
            &mut ask.saved,
            &mut ask.cancelled,
        );
        if ask.saved {
            ask.events.push(DialogEvent::Rewind(prompt));
            ask.close = true;
        }
    }

    /// The folder dialog: the path field, the listing's navigation and
    /// its folders.
    fn folder_body(&mut self, ui: &mut egui::Ui, palette: &Palette, ask: &mut Ask) {
        let listed = self.listing.as_ref().and_then(|read| read.as_ref().ok());
        let parent = listed.and_then(|listing| listing.parent.clone());
        let home = listed.and_then(|listing| listing.home.clone());
        let mut text = std::mem::take(&mut self.folder);
        ui.label(
            egui::RichText::new(
                "A folder on the machine kage runs on. The next session opens there.",
            )
            .size(FS_SM)
            .color(palette.muted),
        );
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let field = egui::TextEdit::singleline(&mut text)
                .hint_text("/path/to/project")
                .desired_width(f32::INFINITY)
                .font(egui::FontId::new(FS_SM, family(FAMILY_MONO_REGULAR)))
                .margin(egui::Margin::symmetric(10, 8));
            let response = ui.add(field);
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                ask.browse = Some(Some(unquote_and_trim(&text).to_owned()));
            }
            if ui
                .add(dialog_button("Browse", palette.accent, palette.bg))
                .clicked()
            {
                ask.browse = Some(Some(unquote_and_trim(&text).to_owned()));
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui
                .add(dialog_button("Up", egui::Color32::TRANSPARENT, palette.ink))
                .clicked()
            {
                ask.browse = Some(parent.clone());
            }
            if ui
                .add(dialog_button(
                    "Home",
                    egui::Color32::TRANSPARENT,
                    palette.ink,
                ))
                .clicked()
            {
                ask.browse = Some(home.clone());
            }
        });
        if let Some(listed) = listed {
            if let Some(read) = self.listing.as_ref().and_then(|read| read.as_ref().err()) {
                ui.label(
                    egui::RichText::new(read.clone())
                        .size(FS_XS)
                        .color(palette.danger),
                );
            }
            ui.add_space(4.0);
            egui::ScrollArea::vertical()
                .max_height(200.0)
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    for name in &listed.folders {
                        let row = ui.add(
                            egui::Button::new(
                                egui::RichText::new(format!("{name}/"))
                                    .size(FS_SM)
                                    .family(family(FAMILY_MONO_REGULAR))
                                    .color(palette.ink),
                            )
                            .fill(egui::Color32::TRANSPARENT)
                            .corner_radius(egui::CornerRadius::same(R_MD as u8))
                            .min_size(egui::vec2(0.0, 26.0)),
                        );
                        if row.clicked() {
                            ask.browse = Some(Some(join_folder(&listed.path, name)));
                        }
                    }
                });
        }
        ui.add_space(12.0);
        footer(
            ui,
            palette,
            &[("Cancel", false), ("Open here", true)],
            &mut ask.saved,
            &mut ask.cancelled,
        );
        if ask.saved {
            ask.events
                .push(DialogEvent::SetProject(unquote_and_trim(&text).to_owned()));
            ask.close = true;
        }
        self.folder = text;
    }
}

/// What one frame's dialogs asked for, gathered while they drew.
#[derive(Debug, Default)]
struct Ask {
    /// The events the dialogs raised.
    events: Vec<DialogEvent>,
    /// The folder listing to ask for, when one was asked for.
    browse: Option<Option<String>>,
    /// Whether the dialog's primary footer button was pressed.
    saved: bool,
    /// Whether its cancel button was pressed.
    cancelled: bool,
    /// Whether the open dialog should close.
    close: bool,
}

/// The footer row: `buttons` drawn right to left, the last one primary.
/// `picked` records the primary's press and `cancelled` the plain one's.
fn footer(
    ui: &mut egui::Ui,
    palette: &Palette,
    buttons: &[(&str, bool)],
    picked: &mut bool,
    cancelled: &mut bool,
) {
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            for (label, primary) in buttons.iter().rev() {
                let fill = if *primary {
                    palette.accent
                } else {
                    egui::Color32::TRANSPARENT
                };
                let ink = if *primary { palette.bg } else { palette.ink };
                if ui.add(dialog_button(label, fill, ink)).clicked() {
                    if *primary {
                        *picked = true;
                    } else {
                        *cancelled = true;
                    }
                }
            }
        });
    });
}

/// A dialog footer button: the label on `fill` in `ink`.
fn dialog_button(label: &str, fill: egui::Color32, ink: egui::Color32) -> egui::Button<'_> {
    egui::Button::new(
        egui::RichText::new(label)
            .size(FS_SM)
            .family(family(FAMILY_BODY_SEMIBOLD))
            .color(ink),
    )
    .fill(fill)
    .corner_radius(egui::CornerRadius::same(R_MD as u8))
    .min_size(egui::vec2(0.0, 30.0))
}

/// The folder names a listing carries, sorted, hidden ones left out.
#[must_use]
pub fn folder_names(listing: &kage_client::wire::FoldersResult) -> Vec<String> {
    listing.folders.clone()
}

/// The session's own changes, for a caller that reads them outside the
/// pane.
#[must_use]
pub fn changes_of(store: &Store) -> Vec<ChangeEntry> {
    store
        .active_session()
        .map(|session| session_changes(store, session))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{DialogKind, Dialogs, counted, join_folder, rewind_preview};
    use crate::store::Store;
    use kage_client::wire::{DiffContent, ToolCallContent, ToolCallStatus, ToolKind};
    use kage_client::{Session, Subagent, ToolCallItem, TranscriptItem};
    use serde_json::json;

    fn store_with_prompt() -> (Store, usize) {
        let mut store = Store::new("/w", false);
        store.set_connect(crate::transport::State::Connected);
        store.handshake(false);
        store.new_session();
        let _ = store.take_outgoing();
        store.absorb(kage_client::Frame::Success {
            id: kage_client::RequestId::Number(2),
            result: json!({"sessionId": "s1"}),
        });
        let _ = store.take_outgoing();
        store.set_active("s1");
        store.absorb(kage_client::Frame::Notification {
            method: "session/update".to_owned(),
            params: json!({
                "sessionId": "s1",
                "update": {"sessionUpdate": "user_message_chunk", "content": {"type": "text", "text": "fix the null check"}},
            }),
        });
        store.absorb(kage_client::Frame::Notification {
            method: "session/update".to_owned(),
            params: json!({
                "sessionId": "s1",
                "update": {
                    "sessionUpdate": "tool_call",
                    "toolCallId": "c1",
                    "title": "edit",
                    "kind": "edit",
                    "status": "completed",
                    "content": [{"type": "diff", "path": "src/main.rs", "oldText": "a\n", "newText": "a\nb\n"}],
                },
            }),
        });
        store.absorb(kage_client::Frame::Notification {
            method: "session/update".to_owned(),
            params: json!({"sessionId": "s1", "update": {"sessionUpdate": "turn", "turn": {}}}),
        });
        let prompt = store
            .active_session()
            .expect("a session")
            .items
            .iter()
            .position(|item| matches!(item, TranscriptItem::User { .. }))
            .expect("the prompt is in the transcript");
        (store, prompt)
    }

    #[test]
    fn the_rewind_preview_counts_what_the_drop_takes() {
        let (store, prompt) = store_with_prompt();
        let session = store.active_session().expect("a session");
        let preview = rewind_preview(&store, session, prompt).expect("a prompt item");
        assert_eq!(preview.prompt, "fix the null check");
        assert_eq!(preview.turns, 1, "the one prompt from it on");
        assert_eq!(preview.tools, 1);
        assert_eq!(
            preview.entries, 2,
            "the prompt and the call, the turn end aside"
        );
        assert_eq!(preview.files.len(), 1);
        assert_eq!(preview.files[0].path, "src/main.rs");
        assert_eq!(preview.files[0].add, 1);
    }

    #[test]
    fn a_rewind_preview_of_a_non_prompt_item_is_none() {
        let (store, _) = store_with_prompt();
        let session = store.active_session().expect("a session");
        assert!(
            rewind_preview(&store, session, 1).is_none(),
            "the tool call is no prompt"
        );
    }

    #[test]
    fn the_count_noun_agrees_with_its_number() {
        assert_eq!(counted(1, "turn", "turns"), "1 turn");
        assert_eq!(counted(2, "turn", "turns"), "2 turns");
        assert_eq!(counted(0, "file", "files"), "0 files");
    }

    #[test]
    fn a_folder_join_never_doubles_a_separator() {
        assert_eq!(join_folder("/home/me", "work"), "/home/me/work");
        assert_eq!(join_folder("/home/me/", "work"), "/home/me/work");
        assert_eq!(join_folder("C:\\Users\\me", "work"), "C:\\Users\\me\\work");
        assert_eq!(
            join_folder("C:\\Users\\me\\", "work"),
            "C:\\Users\\me\\work"
        );
    }

    #[test]
    fn the_layer_draws_nothing_while_no_dialog_is_open() {
        let mut dialogs = Dialogs::new();
        assert!(!dialogs.is_open());
        assert!(dialogs.open().is_none());
        dialogs.close();
        assert!(!dialogs.is_open());
    }

    #[test]
    fn showing_the_goal_dialog_seeds_its_field_from_the_session() {
        let (mut store, _) = store_with_prompt();
        let mut dialogs = Dialogs::new();
        dialogs.show_dialog(DialogKind::Goal, &mut store);
        assert!(dialogs.is_open(), "the goal dialog opens");
        assert_eq!(dialogs.open(), Some(DialogKind::Goal));
        dialogs.close();
        assert!(!dialogs.is_open());
    }

    #[test]
    fn the_swarm_confirmation_is_skipped_when_the_user_turned_it_off() {
        let (mut store, _) = store_with_prompt();
        store.update_prefs(|prefs| prefs.confirm_swarm = false);
        let mut dialogs = Dialogs::new();
        // The ask never appears for someone who switched it off, so
        // the dialog stays closed and the shell is told to enable it.
        assert!(!dialogs.is_open());
        let _ = &mut dialogs;
    }

    #[test]
    fn the_folder_dialog_asks_for_the_home_listing_without_a_path() {
        let (mut store, _) = store_with_prompt();
        let mut dialogs = Dialogs::new();
        dialogs.show_dialog(DialogKind::OpenFolder, &mut store);
        assert_eq!(dialogs.open(), Some(DialogKind::OpenFolder));
    }

    #[test]
    fn a_folder_answer_replaces_the_field_and_stops_the_browse() {
        let (mut store, _) = store_with_prompt();
        let mut dialogs = Dialogs::new();
        dialogs.show_dialog(DialogKind::OpenFolder, &mut store);
        let asked = dialogs.browsing.expect("a listing was asked for");
        dialogs.answer_folders(
            asked,
            Ok(kage_client::wire::FoldersResult {
                path: "/home/me/work".to_owned(),
                parent: Some("/home/me".to_owned()),
                home: Some("/home/me".to_owned()),
                folders: vec!["src".to_owned()],
                truncated: false,
                skipped: 0,
            }),
        );
        assert_eq!(dialogs.folder, "/home/me/work");
        assert!(dialogs.browsing.is_none(), "the answer settled the ask");
    }

    #[test]
    fn an_unrelated_folder_answer_is_ignored() {
        let (mut store, _) = store_with_prompt();
        let mut dialogs = Dialogs::new();
        dialogs.show_dialog(DialogKind::OpenFolder, &mut store);
        let asked = dialogs.browsing.expect("a listing was asked for");
        dialogs.answer_folders(asked + 1, Ok(kage_client::wire::FoldersResult::default()));
        assert!(
            dialogs.browsing.is_some(),
            "a foreign answer leaves the ask in flight"
        );
    }

    #[test]
    fn a_rewind_dialog_names_the_prompt_it_goes_back_to() {
        let (mut store, prompt) = store_with_prompt();
        let mut dialogs = Dialogs::new();
        dialogs.show_dialog(DialogKind::Rewind(prompt), &mut store);
        assert_eq!(dialogs.open(), Some(DialogKind::Rewind(prompt)));
    }

    /// A subagent record for the tests.
    fn agent(name: &str, call: &str) -> Subagent {
        Subagent {
            name: Some(name.to_owned()),
            task: None,
            capabilities: None,
            state: None,
            swarm: None,
            reason: None,
            tool_call_id: Some(call.to_owned()),
            usage: None,
            model: None,
            background: false,
        }
    }

    /// An edit call on `path`.
    fn edit(path: &str) -> ToolCallItem {
        ToolCallItem {
            tool_call_id: "c9".to_owned(),
            title: "edit".to_owned(),
            kind: ToolKind::Edit,
            status: ToolCallStatus::Completed,
            input: None,
            swarm: None,
            content: vec![ToolCallContent::Diff(DiffContent {
                path: path.to_owned(),
                old_text: Some("x\n".to_owned()),
                new_text: "x\ny\n".to_owned(),
            })],
            raw_output: None,
            took_ms: None,
        }
    }

    #[test]
    fn the_rewind_preview_folds_the_subagents_files_in() {
        let (mut store, prompt) = store_with_prompt();
        store.absorb(kage_client::Frame::Notification {
            method: "session/update".to_owned(),
            params: json!({
                "sessionId": "s1",
                "update": {"sessionUpdate": "subagent_update", "subagentSessionId": "a1", "toolCallId": "c1"},
            }),
        });
        let mut session = Session::new("s1");
        session
            .agents
            .insert("a1".to_owned(), agent("explore", "c1"));
        session
            .items
            .push(TranscriptItem::ToolCall(edit("child.rs")));
        let _ = &mut session;
        // The child session's own items are read through the store's
        // state, so the preview folds them in.
        let parent = store.active_session().expect("a session");
        let preview = rewind_preview(&store, parent, prompt).expect("a prompt item");
        assert_eq!(
            preview.agents, 1,
            "the one subagent the dropped call started"
        );
        assert_eq!(preview.files.len(), 1, "the parent's own file");
    }
}
