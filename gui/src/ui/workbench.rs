//! The workbench: the right panel as the kage web client design draws
//! it. A head of tabs over the active session's file changes, its file
//! listing, the announced agents, the shell calls and the fetched
//! pages, read straight from the session transcript.

use std::collections::HashSet;

use kage_client::wire::{ToolCallContent, ToolCallStatus, ToolKind};
use kage_client::{ToolCallItem, TranscriptItem};

use crate::store::Store;
use crate::theme::{
    FAMILY_BODY_SEMIBOLD, FAMILY_MONO_REGULAR, FS_2XS, FS_SM, FS_XS, Palette, R_FULL, R_MD, family,
};
use crate::ui::agents::{self, AgentEvent};
use crate::ui::transcript::model::{DiffLine, diff_lines};
use crate::ui::transcript::tool_rows::RowAction;
use crate::ui::transcript::view::TranscriptView;

/// What a file change row reports, collected from one tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeEntry {
    /// The tool call the entry came from, so the pane can select it.
    pub call_id: String,
    /// The file path the change names.
    pub path: String,
    /// Lines only the new text has.
    pub add: usize,
    /// Lines only the old text has.
    pub del: usize,
    /// Whether the first change made the file: its diff named no old
    /// text.
    pub created: bool,
    /// The subagent that made the change, when one did.
    pub by: Option<String>,
}

/// The path one change names: the diff content's path first, then the
/// tool input's `path`, then the call title.
#[must_use]
pub fn change_path(call: &ToolCallItem) -> String {
    for content in &call.content {
        if let ToolCallContent::Diff(diff) = content {
            return diff.path.clone();
        }
    }
    if let Some(path) = call
        .input
        .as_ref()
        .and_then(|input| input.get("path"))
        .and_then(|path| path.as_str())
    {
        return path.to_owned();
    }
    call.title.clone()
}

/// The changed files the transcript holds: the edit, delete and move
/// calls, in transcript order, folded onto their paths.
#[must_use]
pub fn change_entries(items: &[TranscriptItem]) -> Vec<ChangeEntry> {
    let mut out: Vec<ChangeEntry> = Vec::new();
    for item in items {
        let TranscriptItem::ToolCall(call) = item else {
            continue;
        };
        if !matches!(
            call.kind,
            ToolKind::Edit | ToolKind::Delete | ToolKind::Move
        ) {
            continue;
        }
        let (add, del) = diff_lines(call)
            .map(|lines| {
                lines.iter().fold((0, 0), |(add, del), line| match line {
                    DiffLine::Add(_) => (add + 1, del),
                    DiffLine::Del(_) => (add, del + 1),
                    _ => (add, del),
                })
            })
            .unwrap_or((0, 0));
        let path = change_path(call);
        if let Some(entry) = out.iter_mut().find(|entry| entry.path == path) {
            entry.add += add;
            entry.del += del;
            continue;
        }
        let created = call.content.iter().any(
            |content| matches!(content, ToolCallContent::Diff(diff) if diff.old_text.is_none()),
        );
        out.push(ChangeEntry {
            call_id: call.tool_call_id.clone(),
            path,
            add,
            del,
            created,
            by: None,
        });
    }
    out
}

/// The files `session` and the subagents under it changed: the
/// session's own edits, then each child's, a child's named by its agent.
/// A path two of them touched keeps its first author and sums the lines.
#[must_use]
pub fn session_changes(store: &Store, session: &kage_client::Session) -> Vec<ChangeEntry> {
    let mut out = change_entries(&session.items);
    for (id, agent) in &session.agents {
        let Some(child) = store.state().session(id) else {
            continue;
        };
        for mut entry in change_entries(&child.items) {
            if let Some(known) = out.iter_mut().find(|known| known.path == entry.path) {
                known.add += entry.add;
                known.del += entry.del;
                continue;
            }
            entry.by = agent.name.clone();
            out.push(entry);
        }
    }
    out
}

/// `text` without its ANSI escape sequences: a run of styled output
/// shows as its plain characters, never as markup.
#[must_use]
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.next() {
            // CSI: parameters, then one final byte in @..~.
            Some('[') => {
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            // OSC: up to BEL or ST.
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\u{7}' || (c == '\u{1b}' && chars.peek() == Some(&'\\')) {
                        if c == '\u{1b}' {
                            chars.next();
                        }
                        break;
                    }
                }
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// How many output lines an open run shows, from the end.
const TERM_TAIL: usize = 40;

/// One shell call's facts, collected from the transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermEntry {
    /// The tool call id, which keys the run's open state.
    pub id: String,
    /// The command the call ran.
    pub command: String,
    /// The last [`TERM_TAIL`] lines of output, without the shell
    /// tool's `stdout:` and `exit:` framing.
    pub output: String,
    /// How many output lines the tail leaves out.
    pub hidden: usize,
    /// The exit code, from the raw output or the `exit:` line.
    pub exit: Option<i64>,
    /// Whether the call is still running.
    pub running: bool,
    /// Whether the call failed.
    pub failed: bool,
    /// The subagent that ran it, when one did.
    pub who: Option<String>,
}

/// The shell tool's text without its framing: the output with the
/// `stdout:` header and the `(no output)` stand-in dropped, and the
/// exit code its closing `exit: N` line names.
fn shell_text(text: &str) -> (&str, Option<i64>) {
    let (body, exit) = match text.rsplit_once("\nexit: ") {
        Some((body, code)) if !code.contains('\n') => (body, code.trim().parse().ok()),
        _ => (text, None),
    };
    let body = body.strip_prefix("stdout:\n").unwrap_or(body);
    let body = if body == "(no output)" { "" } else { body };
    (body.trim_end(), exit)
}

/// The last [`TERM_TAIL`] lines of `text`, and how many come before.
fn tail(text: &str) -> (String, usize) {
    let lines: Vec<&str> = text.lines().collect();
    let hidden = lines.len().saturating_sub(TERM_TAIL);
    (lines[hidden..].join("\n"), hidden)
}

/// The shell calls the transcript holds, in transcript order.
#[must_use]
pub fn term_entries(items: &[TranscriptItem]) -> Vec<TermEntry> {
    let mut out = Vec::new();
    for item in items {
        let TranscriptItem::ToolCall(call) = item else {
            continue;
        };
        if call.kind != ToolKind::Execute {
            continue;
        }
        let command = call
            .input
            .as_ref()
            .and_then(|input| input.get("command"))
            .and_then(|command| command.as_str())
            .unwrap_or(&call.title)
            .to_owned();
        let text = call.text();
        let (body, text_exit) = shell_text(&text);
        let exit = call
            .raw_output
            .as_ref()
            .and_then(|output| {
                output
                    .get("exitCode")
                    .or_else(|| output.get("exit_code"))
                    .and_then(serde_json::Value::as_i64)
            })
            .or(text_exit);
        let (output, hidden) = tail(&strip_ansi(body));
        out.push(TermEntry {
            id: call.tool_call_id.clone(),
            command: command.trim().to_owned(),
            output,
            hidden,
            exit,
            running: matches!(
                call.status,
                ToolCallStatus::Pending | ToolCallStatus::InProgress
            ),
            failed: call.status == ToolCallStatus::Failed,
            who: None,
        });
    }
    out
}

/// One fetched page's facts, collected from the transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchEntry {
    /// The URL the call fetched, as its title carries it.
    pub url: String,
    /// The reader text the call streamed, if any.
    pub text: String,
}

/// The remote fetches the transcript holds, in transcript order.
#[must_use]
pub fn fetch_entries(items: &[TranscriptItem]) -> Vec<FetchEntry> {
    let mut out = Vec::new();
    for item in items {
        let TranscriptItem::ToolCall(call) = item else {
            continue;
        };
        if call.kind != ToolKind::Fetch {
            continue;
        }
        // The final URL after redirects, when the call reported one.
        let url = call
            .raw_output
            .as_ref()
            .and_then(|output| output.get("url"))
            .and_then(serde_json::Value::as_str)
            .map_or_else(|| change_path(call), str::to_owned);
        out.push(FetchEntry {
            url,
            text: call.text(),
        });
    }
    out
}

/// The panes the workbench tabs switch between.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Tab {
    /// The file changes the session made.
    #[default]
    Changes,
    /// The session workdir's listing.
    Files,
    /// The announced subagents.
    Agents,
    /// The shell calls and their output.
    Terminal,
    /// The pages the session fetched.
    Browser,
}

impl Tab {
    /// The tabs in head order.
    pub const ALL: [Tab; 5] = [
        Tab::Changes,
        Tab::Files,
        Tab::Agents,
        Tab::Terminal,
        Tab::Browser,
    ];

    /// The label the tab carries.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Tab::Changes => "Changes",
            Tab::Files => "Files",
            Tab::Agents => "Agents",
            Tab::Terminal => "Terminal",
            Tab::Browser => "Browser",
        }
    }

    /// The icon the tab leads with.
    #[must_use]
    pub fn icon(self) -> crate::icons::Icon {
        match self {
            Tab::Changes => crate::icons::Icon::FileDiff,
            Tab::Files => crate::icons::Icon::Folder,
            Tab::Agents => crate::icons::Icon::Users,
            Tab::Terminal => crate::icons::Icon::Terminal,
            Tab::Browser => crate::icons::Icon::Globe,
        }
    }
}

/// The filter the agents pane narrows its rows with.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Filter {
    /// Every announced agent.
    #[default]
    All,
    /// Only the agents still working.
    Running,
    /// Only the agents that ended.
    Done,
    /// Only the agents that run in the background.
    Background,
}

impl Filter {
    /// The filters in chip order.
    pub const ALL: [Filter; 4] = [
        Filter::All,
        Filter::Running,
        Filter::Done,
        Filter::Background,
    ];

    /// The label the chip carries.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Running => "Running",
            Filter::Done => "Done",
            Filter::Background => "Background",
        }
    }

    /// Whether one agent survives the filter.
    #[must_use]
    pub fn keeps(self, agent: &kage_client::Subagent) -> bool {
        match self {
            Filter::All => true,
            Filter::Running => matches!(
                agent.state,
                None | Some(
                    kage_client::wire::SubagentState::Running
                        | kage_client::wire::SubagentState::Paused
                )
            ),
            Filter::Done => matches!(
                agent.state,
                Some(
                    kage_client::wire::SubagentState::Completed
                        | kage_client::wire::SubagentState::Failed
                        | kage_client::wire::SubagentState::Cancelled
                )
            ),
            Filter::Background => agent.background,
        }
    }
}

/// A byte count as the design writes it: `812 B`, `4.2 KB`, `1.2 MB`.
#[must_use]
pub fn bytes(n: usize) -> String {
    #[allow(clippy::cast_precision_loss)]
    let n = n as f64;
    if n < 1024.0 {
        format!("{n} B")
    } else if n < 1024.0 * 1024.0 {
        format!("{:.1} KB", n / 1024.0)
    } else {
        format!("{:.1} MB", n / (1024.0 * 1024.0))
    }
}

/// The mono count a tab carries beside its label.
fn tab_count(ui: &mut egui::Ui, count: &str, palette: &Palette) {
    ui.label(
        egui::RichText::new(count)
            .size(FS_2XS)
            .family(family(FAMILY_MONO_REGULAR))
            .color(palette.faint),
    );
}

/// The uppercase section label between panes.
fn section_head(ui: &mut egui::Ui, text: &str, palette: &Palette) {
    ui.add_space(12.0);
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .size(FS_XS)
            .family(family(FAMILY_BODY_SEMIBOLD))
            .color(palette.faint),
    );
    ui.add_space(4.0);
}

/// The empty-state block of the design: a faint icon over one line.
fn empty_block(ui: &mut egui::Ui, icon: crate::icons::Icon, text: &str, palette: &Palette) {
    ui.add_space(48.0);
    ui.vertical_centered(|ui| {
        crate::icons::icon(ui, icon, 28.0, palette.faint);
        ui.add_space(10.0);
        ui.label(egui::RichText::new(text).size(FS_SM).color(palette.faint));
    });
}

/// The workbench's state: the active tab, its filters and the panes'
/// selections.
#[derive(Debug, Default)]
pub struct Workbench {
    /// The subagent whose detail the workbench shows, when one is open.
    agent: Option<String>,
    /// The subagent's transcript, pinned to `agent`.
    agent_view: Option<TranscriptView>,
    /// The pane the tabs show.
    tab: Tab,
    /// The file the files pane previews.
    file: Option<String>,
    /// The filter the agents pane narrows with.
    filter: Filter,
    /// The change entry the pane shows a diff for, by call id.
    selected: Option<String>,
    /// Whether the workdir listing was asked for this session, so
    /// opening the files pane does not ask twice.
    files_asked: bool,
    /// The session the files ask belongs to.
    loaded: Option<String>,
    /// Shell runs the user opened or closed against their default: the
    /// latest run and running ones start open, the rest folded.
    term_flipped: HashSet<String>,
}

impl Workbench {
    /// A workbench on the changes pane.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The pane the tabs show.
    #[must_use]
    pub const fn tab(&self) -> Tab {
        self.tab
    }

    /// Switches to `tab`.
    pub fn set_tab(&mut self, tab: Tab) {
        self.tab = tab;
    }

    /// The filter the agents pane narrows with.
    #[must_use]
    pub const fn filter(&self) -> Filter {
        self.filter
    }

    /// Sets the agents pane's filter.
    pub fn set_filter(&mut self, filter: Filter) {
        self.filter = filter;
    }

    /// The file the files pane previews.
    #[must_use]
    pub fn file(&self) -> Option<&str> {
        self.file.as_deref()
    }

    /// Previews `file` in the files pane.
    pub fn set_file(&mut self, file: Option<String>) {
        self.file = file;
    }

    /// Whether the workdir listing was asked for `session`.
    #[must_use]
    pub fn listing_asked(&self, session: &str) -> bool {
        self.loaded.as_deref() == Some(session) && self.files_asked
    }

    /// Records that the workdir listing was asked for `session`.
    pub fn mark_listing_asked(&mut self, session: &str) {
        self.files_asked = true;
        self.loaded = Some(session.to_owned());
    }

    /// Whether the shell run `id` shows its output, against the
    /// default: the latest run and running ones start open.
    #[must_use]
    pub fn run_open(&self, id: &str, running: bool, latest: bool) -> bool {
        let open = running || latest;
        if self.term_flipped.contains(id) {
            !open
        } else {
            open
        }
    }

    /// Flips the shell run `id`'s open state against its default.
    pub fn flip_run(&mut self, id: &str) {
        if !self.term_flipped.insert(id.to_owned()) {
            self.term_flipped.remove(id);
        }
    }

    /// The change entry the pane shows a diff for.
    #[must_use]
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    /// Selects the change entry `id`, or clears it when `None`.
    pub fn select(&mut self, id: Option<String>) {
        self.selected = id;
    }

    /// The subagent whose detail is open.
    #[must_use]
    pub fn agent(&self) -> Option<&str> {
        self.agent.as_deref()
    }

    /// Opens subagent `id`'s detail on the agents tab.
    pub fn open_agent(&mut self, id: String) {
        self.tab = Tab::Agents;
        self.agent_view = Some(TranscriptView::pinned(id.clone()));
        self.agent = Some(id);
    }

    /// Returns from a subagent's detail to the agents list.
    pub fn close_agent(&mut self) {
        self.agent = None;
        self.agent_view = None;
    }
}

/// What the workbench asks the shell to carry out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkbenchEvent {
    /// The user picked a file in the files pane; the shell mentions it
    /// in the composer.
    Mention(String),
    /// Open the child session `id` in the detail tab.
    OpenAgent(String),
    /// Stop the child session `id`.
    StopAgent(String),
    /// Ask the engine for the workdir listing.
    AskFiles,
    /// Read the file at `path` for the files pane's preview.
    ReadFile(String),
    /// An action a row of the subagent's transcript raised.
    Row(RowAction),
}

/// Draws the workbench and reports what its panes asked for.
pub fn show(
    ui: &mut egui::Ui,
    workbench: &mut Workbench,
    store: &mut Store,
) -> Vec<WorkbenchEvent> {
    let palette = Palette::active_of(ui.ctx());
    let mut events = Vec::new();
    let session_id = store.active_id().map(str::to_owned);

    ui.vertical(|ui| {
        ui.set_width(ui.available_width());
        tab_strip(ui, workbench, palette);
        let Some(session) = store.active_session() else {
            empty_block(
                ui,
                crate::icons::Icon::Folder,
                "Start a session to see its changes, agents and output here.",
                palette,
            );
            return;
        };
        // The panes read the session by value, so the store stays free
        // for the panes that write to it.
        let items = session.items.clone();
        let agents = session.agents.clone();
        if workbench.agent().is_some() {
            agent_detail(ui, workbench, store, palette, &mut events);
            return;
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                match workbench.tab() {
                    Tab::Changes => {
                        changes_pane(ui, workbench, store, palette, &items, &mut events)
                    }
                    Tab::Files => files_pane(ui, workbench, store, palette, &mut events),
                    Tab::Agents => agents_pane(ui, workbench, store, palette, &agents, &mut events),
                    Tab::Terminal => terminal_pane(ui, workbench, palette, &items),
                    Tab::Browser => browser_pane(ui, palette, &items),
                }
            });
        let _ = session_id;
    });
    events
}

/// The tab strip: the five tabs, the active one filled.
fn tab_strip(ui: &mut egui::Ui, workbench: &mut Workbench, palette: &Palette) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for tab in Tab::ALL {
            let active = workbench.tab() == tab;
            let response = ui.add(
                egui::Button::new(
                    egui::RichText::new(tab.label())
                        .size(FS_XS)
                        .color(if active { palette.ink } else { palette.muted }),
                )
                .fill(if active {
                    palette.selected
                } else {
                    egui::Color32::TRANSPARENT
                })
                .corner_radius(egui::CornerRadius::same(R_MD as u8))
                .min_size(egui::vec2(0.0, crate::theme::TAB_H)),
            );
            if response.clicked() {
                workbench.set_tab(tab);
            }
        }
    });
}

/// The changes pane: one row per changed file, with its add and delete
/// counts, opening a diff for the selected one.
fn changes_pane(
    ui: &mut egui::Ui,
    workbench: &mut Workbench,
    store: &mut Store,
    palette: &Palette,
    items: &[TranscriptItem],
    events: &mut Vec<WorkbenchEvent>,
) {
    let Some(session) = store.active_session() else {
        return;
    };
    let changes = session_changes(store, session);
    if changes.is_empty() {
        empty_block(
            ui,
            crate::icons::Icon::FileDiff,
            "No file changes in this session yet.",
            palette,
        );
        return;
    }
    section_head(ui, "Changed files", palette);
    for entry in &changes {
        let selected = workbench.selected() == Some(entry.call_id.as_str());
        let response = ui.add(
            egui::Button::new("")
                .fill(if selected {
                    palette.selected
                } else {
                    egui::Color32::TRANSPARENT
                })
                .corner_radius(egui::CornerRadius::same(R_MD as u8))
                .min_size(egui::vec2(ui.available_width(), 30.0)),
        );
        let rect = response.rect;
        ui.painter().text(
            egui::pos2(rect.left() + 12.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            entry.path.clone(),
            egui::FontId::new(FS_SM, family(crate::theme::FAMILY_BODY_REGULAR)),
            palette.ink,
        );
        let counts = format!("+{} -{}", entry.add, entry.del);
        ui.painter().text(
            egui::pos2(rect.right() - 12.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            counts,
            egui::FontId::new(FS_2XS, family(FAMILY_MONO_REGULAR)),
            palette.faint,
        );
        if selected {
            let open = egui::Rect::from_center_size(
                egui::pos2(rect.right() - 100.0, rect.center().y),
                egui::vec2(84.0, 22.0),
            );
            if ui
                .put(open, egui::Button::new("Open file").small())
                .clicked()
            {
                workbench.set_tab(Tab::Files);
                workbench.set_file(Some(entry.path.clone()));
                events.push(WorkbenchEvent::ReadFile(entry.path.clone()));
            }
        }
        if response.clicked() {
            if selected {
                workbench.select(None);
            } else {
                workbench.select(Some(entry.call_id.clone()));
            }
        }
    }
    // The selected entry's diff, when one is picked.
    if let Some(id) = workbench.selected().map(str::to_owned) {
        let call = items.iter().find_map(|item| match item {
            TranscriptItem::ToolCall(call) if call.tool_call_id == id => Some(call),
            _ => None,
        });
        if let Some(call) = call {
            section_head(ui, &entry_path(call), palette);
            if let Some(lines) = diff_lines(call) {
                for line in lines {
                    let (marker, color) = match line {
                        DiffLine::Add(_) => ("+", palette.diff_add),
                        DiffLine::Del(_) => ("-", palette.diff_del),
                        _ => (" ", palette.muted),
                    };
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(marker)
                                .size(FS_2XS)
                                .family(family(FAMILY_MONO_REGULAR))
                                .color(color),
                        );
                        ui.label(
                            egui::RichText::new(line.body().to_owned())
                                .size(FS_2XS)
                                .family(family(FAMILY_MONO_REGULAR))
                                .color(color),
                        );
                    });
                }
            }
        }
    }
}

/// The path a change entry's diff names.
fn entry_path(call: &ToolCallItem) -> String {
    change_path(call)
}

/// The files pane: the workdir's listing, with a preview of the
/// picked file.
fn files_pane(
    ui: &mut egui::Ui,
    workbench: &mut Workbench,
    store: &mut Store,
    palette: &Palette,
    events: &mut Vec<WorkbenchEvent>,
) {
    let Some(id) = store.active_id().map(str::to_owned) else {
        empty_block(
            ui,
            crate::icons::Icon::Folder,
            "Start a session to see its changes, agents and output here.",
            palette,
        );
        return;
    };
    if !workbench.listing_asked(&id) {
        workbench.mark_listing_asked(&id);
        events.push(WorkbenchEvent::AskFiles);
    }
    let Some(listing) = store.fs_listing(&id) else {
        empty_block(
            ui,
            crate::icons::Icon::Folder,
            "Listing the session workdir\u{2026}",
            palette,
        );
        return;
    };
    let entries: Vec<_> = listing.entries.clone();
    section_head(ui, "Files", palette);
    for entry in &entries {
        let picked = workbench.file() == Some(entry.path.as_str());
        let icon = if entry.kind == kage_client::wire::FsKind::Directory {
            crate::icons::Icon::Folder
        } else {
            crate::icons::Icon::File
        };
        let response = ui.add(
            egui::Button::new("")
                .fill(if picked {
                    palette.selected
                } else {
                    egui::Color32::TRANSPARENT
                })
                .corner_radius(egui::CornerRadius::same(R_MD as u8))
                .min_size(egui::vec2(ui.available_width(), 28.0)),
        );
        let rect = response.rect;
        paint_icon(
            ui,
            icon,
            14.0,
            palette.muted,
            egui::pos2(rect.left() + 13.0, rect.center().y),
        );
        ui.painter().text(
            egui::pos2(rect.left() + 26.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            entry.path.clone(),
            egui::FontId::new(FS_SM, family(crate::theme::FAMILY_BODY_REGULAR)),
            if picked { palette.ink } else { palette.muted },
        );
        if entry.kind == kage_client::wire::FsKind::File && entry.size != 0 {
            ui.painter().text(
                egui::pos2(rect.right() - 12.0, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                bytes(entry.size as usize),
                egui::FontId::new(FS_2XS, family(FAMILY_MONO_REGULAR)),
                palette.faint,
            );
        }
        if response.clicked() {
            if picked {
                workbench.set_file(None);
            } else {
                workbench.set_file(Some(entry.path.clone()));
                events.push(WorkbenchEvent::Mention(entry.path.clone()));
                if entry.kind != kage_client::wire::FsKind::Directory {
                    events.push(WorkbenchEvent::ReadFile(entry.path.clone()));
                }
            }
        }
    }
    file_preview(ui, store, &id, workbench, palette, events);
}

/// The preview of the selected file: its path with a mention button,
/// then the numbered lines, or a note when the file is binary or was
/// cut short.
fn file_preview(
    ui: &mut egui::Ui,
    store: &Store,
    session: &str,
    workbench: &Workbench,
    palette: &Palette,
    events: &mut Vec<WorkbenchEvent>,
) {
    let Some(selected) = workbench.file() else {
        return;
    };
    let Some((path, read)) = store.fs_preview(session) else {
        return;
    };
    if path != selected {
        return;
    }
    ui.add_space(10.0);
    egui::Frame::NONE
        .stroke(egui::Stroke::new(1.0, palette.subtle))
        .inner_margin(egui::Margin::symmetric(14, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                crate::icons::icon(ui, crate::icons::Icon::FilePlus, 14.0, palette.faint);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(path)
                            .size(FS_XS)
                            .family(family(FAMILY_MONO_REGULAR))
                            .color(palette.ink),
                    )
                    .truncate(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::ui::forms::icon_button(
                        ui,
                        "file-mention",
                        crate::icons::Icon::AtSign,
                        palette,
                    )
                    .on_hover_text("Mention in composer")
                    .clicked()
                    {
                        events.push(WorkbenchEvent::Mention(path.to_owned()));
                    }
                });
            });
        });
    if read.binary {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            ui.label(
                egui::RichText::new("Binary file; not shown.")
                    .size(FS_XS)
                    .color(palette.faint),
            );
        });
        return;
    }
    egui::Frame::NONE
        .fill(palette.deep)
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            for (n, line) in read.content.lines().enumerate() {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    ui.add_sized(
                        egui::vec2(28.0, 19.0),
                        egui::Label::new(
                            egui::RichText::new((n + 1).to_string())
                                .size(12.0)
                                .family(family(FAMILY_MONO_REGULAR))
                                .color(palette.ghost),
                        ),
                    );
                    ui.label(
                        egui::RichText::new(line)
                            .size(12.0)
                            .family(family(FAMILY_MONO_REGULAR))
                            .color(palette.ink),
                    );
                });
            }
        });
    if read.truncated {
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            ui.label(
                egui::RichText::new("Truncated: the file is longer than the preview.")
                    .size(FS_XS)
                    .color(palette.faint),
            );
        });
    }
}

/// The subagent detail: back, the name, the kind, the state with Stop
/// while it runs, the task and the measured facts, then its transcript.
fn agent_detail(
    ui: &mut egui::Ui,
    workbench: &mut Workbench,
    store: &Store,
    palette: &Palette,
    events: &mut Vec<WorkbenchEvent>,
) {
    let Some(id) = workbench.agent().map(str::to_owned) else {
        return;
    };
    let found = store
        .active_session()
        .and_then(|parent| parent.agents.get(&id).map(|agent| (parent, agent)));
    let Some((parent, agent)) = found else {
        empty_block(ui, crate::icons::Icon::Bot, "Agent not found.", palette);
        return;
    };
    let facts = agents::agent_facts(store, parent, &id, agent);
    let kind = match &facts.item {
        Some((_, index)) => format!("swarm #{:02}", index + 1),
        None => facts.name.clone(),
    };
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.add_space(8.0);
        if crate::ui::forms::icon_button(ui, "agent-back", crate::icons::Icon::ChevronLeft, palette)
            .on_hover_text("Back to agents")
            .clicked()
        {
            workbench.close_agent();
        }
        ui.label(
            egui::RichText::new(&facts.name)
                .size(FS_SM)
                .family(family(FAMILY_BODY_SEMIBOLD))
                .color(palette.ink_strong),
        );
        ui.label(egui::RichText::new(kind).size(11.0).color(palette.muted));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(8.0);
            if facts.stoppable
                && ui
                    .add(egui::Button::new(
                        egui::RichText::new("Stop")
                            .size(FS_XS)
                            .color(palette.danger),
                    ))
                    .clicked()
            {
                events.push(WorkbenchEvent::StopAgent(id.clone()));
            }
            ui.label(
                egui::RichText::new(facts.phase.label())
                    .size(FS_XS)
                    .color(facts.phase.color(palette)),
            );
        });
    });
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(&facts.task)
                .size(FS_SM)
                .color(palette.ink),
        );
    });
    let calls = store.state().session(&id).map_or(0, |child| {
        child
            .items
            .iter()
            .filter(|item| matches!(item, TranscriptItem::ToolCall(_)))
            .count()
    });
    let line = [
        Some(facts.name.clone()),
        facts.model.clone(),
        facts.elapsed.map(crate::clock::span),
        facts.tokens.map(|n| format!("{} tok", agents::tokens(n))),
        facts.cost.map(|cost| format!("USD {cost:.4}")),
        Some(format!("{calls} tool calls")),
        facts.item.as_ref().map(|(item, _)| format!("item: {item}")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" \u{b7} ");
    ui.horizontal_wrapped(|ui| {
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(line)
                .size(11.0)
                .family(family(FAMILY_MONO_REGULAR))
                .color(palette.faint),
        );
    });
    if let Some(reason) = &facts.reason {
        ui.horizontal_wrapped(|ui| {
            ui.add_space(8.0);
            ui.label(egui::RichText::new(reason).size(FS_XS).color(palette.warn));
        });
    }
    ui.separator();
    let mut rows = Vec::new();
    if let Some(view) = workbench.agent_view.as_mut() {
        view.show(ui, store, &mut rows);
    }
    events.extend(rows.into_iter().map(WorkbenchEvent::Row));
}

/// The agents pane: the filter chips over the session's agent cards.
fn agents_pane(
    ui: &mut egui::Ui,
    workbench: &mut Workbench,
    store: &Store,
    palette: &Palette,
    agents: &std::collections::BTreeMap<String, kage_client::Subagent>,
    events: &mut Vec<WorkbenchEvent>,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for filter in Filter::ALL {
            let active = workbench.filter() == filter;
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new(filter.label())
                            .size(FS_2XS)
                            .color(if active { palette.ink } else { palette.muted }),
                    )
                    .fill(if active {
                        palette.selected
                    } else {
                        palette.fill
                    })
                    .corner_radius(egui::CornerRadius::same(R_FULL as u8)),
                )
                .clicked()
            {
                workbench.set_filter(filter);
            }
        }
    });
    let Some(parent) = store.active_session() else {
        return;
    };
    let mut cards: Vec<agents::AgentFacts> = agents
        .iter()
        .filter(|(_, agent)| workbench.filter().keeps(agent))
        .map(|(id, agent)| agents::agent_facts(store, parent, id, agent))
        .collect();
    cards.sort_by_key(|facts| facts.item.as_ref().map(|(_, ix)| *ix).unwrap_or(u32::MAX));
    if cards.is_empty() {
        let text = if agents.is_empty() {
            "No agents yet. Subagents and swarm workers show up here while they run."
        } else {
            "No agents match this filter."
        };
        empty_block(ui, crate::icons::Icon::Users, text, palette);
        return;
    }
    for facts in &cards {
        for event in agents::agent_card(ui, palette, facts, parent.running) {
            match event {
                AgentEvent::Open(id) => events.push(WorkbenchEvent::OpenAgent(id)),
                AgentEvent::Stop(id) => events.push(WorkbenchEvent::StopAgent(id)),
                AgentEvent::Steer(_, _) => {}
            }
        }
    }
}

/// The terminal pane: the session's shell calls, each folded to its
/// command until it is opened.
fn terminal_pane(
    ui: &mut egui::Ui,
    workbench: &mut Workbench,
    palette: &Palette,
    items: &[TranscriptItem],
) {
    let runs = term_entries(items);
    if runs.is_empty() {
        empty_block(
            ui,
            crate::icons::Icon::Terminal,
            "Shell commands and their output collect here.",
            palette,
        );
        return;
    }
    section_head(ui, "Shell runs", palette);
    let latest = runs.last().map(|run| run.id.clone());
    for run in &runs {
        let open = workbench.run_open(&run.id, run.running, latest.as_deref() == Some(&run.id));
        let header = ui.add(
            egui::Button::new("")
                .fill(egui::Color32::TRANSPARENT)
                .corner_radius(egui::CornerRadius::same(R_MD as u8))
                .min_size(egui::vec2(ui.available_width(), 30.0)),
        );
        let rect = header.rect;
        let (icon, color) = if run.running {
            (crate::icons::Icon::LoaderCircle, palette.accent)
        } else if run.failed || run.exit.is_some_and(|code| code != 0) {
            (crate::icons::Icon::CircleX, palette.danger)
        } else {
            (crate::icons::Icon::CircleCheck, palette.ok)
        };
        paint_icon(
            ui,
            icon,
            14.0,
            color,
            egui::pos2(rect.left() + 13.0, rect.center().y),
        );
        ui.painter().text(
            egui::pos2(rect.left() + 26.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            run.command.clone(),
            egui::FontId::new(FS_SM, family(FAMILY_MONO_REGULAR)),
            palette.ink,
        );
        if let Some(who) = &run.who {
            ui.painter().text(
                egui::pos2(rect.right() - 12.0, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                who.clone(),
                egui::FontId::new(FS_2XS, family(FAMILY_MONO_REGULAR)),
                palette.faint,
            );
        }
        if header.clicked() {
            workbench.flip_run(&run.id);
        }
        if open {
            if run.hidden > 0 {
                ui.label(
                    egui::RichText::new(format!("{} earlier lines hidden", run.hidden))
                        .size(FS_2XS)
                        .color(palette.faint),
                );
            }
            ui.add(
                egui::TextEdit::multiline(&mut run.output.clone())
                    .font(egui::FontId::new(FS_2XS, family(FAMILY_MONO_REGULAR)))
                    .desired_width(f32::INFINITY),
            );
        }
    }
}

/// The browser pane: the pages the session fetched.
fn browser_pane(ui: &mut egui::Ui, palette: &Palette, items: &[TranscriptItem]) {
    let pages = fetch_entries(items);
    if pages.is_empty() {
        empty_block(
            ui,
            crate::icons::Icon::Globe,
            "Pages the agent fetches open here as reader text. No scripts, no remote images.",
            palette,
        );
        return;
    }
    section_head(ui, "Fetched pages", palette);
    for page in &pages {
        ui.label(
            egui::RichText::new(page.url.clone())
                .size(FS_SM)
                .family(family(FAMILY_MONO_REGULAR))
                .color(palette.ink),
        );
        if !page.text.is_empty() {
            ui.add(
                egui::TextEdit::multiline(&mut page.text.clone())
                    .font(egui::FontId::new(FS_2XS, family(FAMILY_MONO_REGULAR)))
                    .desired_width(f32::INFINITY),
            );
        }
        ui.add_space(8.0);
    }
    let _ = tab_count;
}

#[cfg(test)]
mod tests {
    use super::Workbench;

    #[test]
    fn opening_an_agent_shows_its_detail_and_closing_returns() {
        let mut workbench = Workbench::new();
        assert_eq!(workbench.agent(), None);
        workbench.open_agent("child-1".to_owned());
        assert_eq!(workbench.agent(), Some("child-1"));
        workbench.close_agent();
        assert_eq!(workbench.agent(), None);
    }

    use super::{
        ChangeEntry, Filter, Tab, TermEntry, bytes, change_entries, change_path, fetch_entries,
        strip_ansi, term_entries,
    };
    use kage_client::wire::{
        DiffContent, FsEntry, FsKind, ToolCallContent, ToolCallStatus, ToolKind,
    };
    use kage_client::{Frame, ToolCallItem, TranscriptItem};
    use serde_json::json;

    fn call(id: &str, kind: ToolKind, title: &str) -> ToolCallItem {
        ToolCallItem {
            tool_call_id: id.to_owned(),
            title: title.to_owned(),
            kind,
            status: ToolCallStatus::Completed,
            content: Vec::new(),
            raw_output: None,
            input: None,
            swarm: None,
            took_ms: None,
        }
    }

    fn edit_call(id: &str, path: &str, old: &str, new: &str) -> ToolCallItem {
        ToolCallItem {
            content: vec![ToolCallContent::Diff(DiffContent {
                path: path.to_owned(),
                old_text: (!old.is_empty()).then(|| old.to_owned()),
                new_text: new.to_owned(),
            })],
            input: Some(json!({"path": path})),
            ..call(id, ToolKind::Edit, "edit")
        }
    }

    #[test]
    fn the_tabs_are_the_five_the_head_draws() {
        assert_eq!(Tab::ALL.len(), 5);
        assert_eq!(Tab::ALL[0], Tab::Changes);
        assert_eq!(Tab::ALL[0].label(), "Changes");
        assert_eq!(Tab::ALL[4].label(), "Browser");
        assert_eq!(Tab::default(), Tab::Changes);
    }

    #[test]
    fn the_filters_keep_the_agents_they_name() {
        let agent = |state, background| kage_client::Subagent {
            name: None,
            task: None,
            capabilities: None,
            state,
            swarm: None,
            reason: None,
            tool_call_id: None,
            usage: None,
            model: None,
            background,
        };
        assert!(Filter::All.keeps(&agent(None, false)));
        assert!(Filter::Running.keeps(&agent(None, false)));
        assert!(!Filter::Running.keeps(&agent(
            Some(kage_client::wire::SubagentState::Completed),
            false
        )));
        assert!(Filter::Done.keeps(&agent(
            Some(kage_client::wire::SubagentState::Completed),
            false
        )));
        assert!(!Filter::Done.keeps(&agent(None, false)));
        assert!(Filter::Background.keeps(&agent(None, true)));
        assert!(!Filter::Background.keeps(&agent(None, false)));
    }

    #[test]
    fn byte_counts_read_like_the_design() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(812), "812 B");
        assert_eq!(bytes(4_301), "4.2 KB");
        assert_eq!(bytes(1_258_291), "1.2 MB");
    }

    #[test]
    fn a_change_names_the_diff_path_first_then_the_input_then_the_title() {
        let diff_call = edit_call("c1", "src/main.rs", "old", "new");
        assert_eq!(change_path(&diff_call), "src/main.rs");
        let input_call = ToolCallItem {
            input: Some(json!({"path": "src/lib.rs"})),
            ..call("c2", ToolKind::Edit, "the title")
        };
        assert_eq!(change_path(&input_call), "src/lib.rs");
        let bare = call("c3", ToolKind::Edit, "the title");
        assert_eq!(change_path(&bare), "the title");
    }

    #[test]
    fn the_changes_fold_onto_their_paths_and_count_both_sides() {
        let items = vec![
            TranscriptItem::ToolCall(edit_call("c1", "src/main.rs", "a\nb\nc", "a\nB\nc\nd")),
            TranscriptItem::ToolCall(edit_call("c2", "src/main.rs", "", "x\ny")),
            TranscriptItem::ToolCall(edit_call("c3", "src/lib.rs", "z", "z\nw")),
            // Not a change kind.
            TranscriptItem::ToolCall(call("c4", ToolKind::Read, "read")),
        ];
        let changes = change_entries(&items);
        assert_eq!(changes.len(), 2, "the two main.rs calls fold onto one");
        let main = changes
            .iter()
            .find(|entry| entry.path == "src/main.rs")
            .expect("main.rs");
        assert_eq!(
            main.add, 5,
            "three adds from the first call, two from the second"
        );
        assert_eq!(
            main.del, 2,
            "two dels from the first call, none from the second"
        );
        assert!(!main.created);
        let lib = changes
            .iter()
            .find(|entry| entry.path == "src/lib.rs")
            .expect("lib.rs");
        assert_eq!(lib.add, 1);
        assert_eq!(lib.del, 0);
    }

    #[test]
    fn a_created_file_names_itself_as_created() {
        let items = vec![TranscriptItem::ToolCall(edit_call("c1", "new.rs", "", "x"))];
        let changes = change_entries(&items);
        assert!(changes[0].created, "no old text means the file was made");
    }

    #[test]
    fn ansi_escapes_drop_from_shell_output() {
        assert_eq!(strip_ansi("plain"), "plain");
        assert_eq!(strip_ansi("\u{1b}[31mred\u{1b}[0m"), "red");
        assert_eq!(strip_ansi("\u{1b}]0;title\u{7}after"), "after");
        assert_eq!(strip_ansi("\u{1b}[2K\u{1b}[1;32mok\u{1b}[m"), "ok");
        assert_eq!(
            strip_ansi("a\u{1b}b"),
            "ab",
            "a lone escape drops with its byte"
        );
    }

    #[test]
    fn the_shell_framing_drops_and_the_exit_code_names_itself() {
        let items = vec![TranscriptItem::ToolCall(ToolCallItem {
            raw_output: Some(json!({"exitCode": 0})),
            input: Some(json!({"command": "ls -la"})),
            ..call("c1", ToolKind::Execute, "shell")
        })];
        let runs = term_entries(&items);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].command, "ls -la");
        assert_eq!(runs[0].exit, Some(0));
        assert!(!runs[0].running);
        assert!(!runs[0].failed);
    }

    #[test]
    fn a_shell_run_names_its_command_from_its_input() {
        let items = vec![TranscriptItem::ToolCall(ToolCallItem {
            input: Some(json!({"command": "  cargo test  "})),
            ..call("c1", ToolKind::Execute, "shell")
        })];
        let runs = term_entries(&items);
        assert_eq!(runs[0].command, "cargo test", "the command is trimmed");
    }

    #[test]
    fn the_shell_tail_keeps_the_last_lines_and_counts_the_hidden() {
        let text: String = (0..50).map(|n| format!("line {n}\n")).collect();
        let (out, hidden) = super::tail(&text);
        assert_eq!(hidden, 10, "fifty lines, forty shown");
        assert!(out.contains("line 49"));
        assert!(!out.contains("line 0\n"));
        let (short, hidden) = super::tail("one\ntwo");
        assert_eq!(hidden, 0);
        assert_eq!(short, "one\ntwo");
    }

    #[test]
    fn the_fetches_name_their_final_url() {
        let items = vec![TranscriptItem::ToolCall(ToolCallItem {
            raw_output: Some(json!({"url": "https://example.com/final"})),
            ..call("c1", ToolKind::Fetch, "https://example.com/start")
        })];
        let pages = fetch_entries(&items);
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].url, "https://example.com/final");
        let bare = vec![TranscriptItem::ToolCall(call(
            "c2",
            ToolKind::Fetch,
            "https://x.test",
        ))];
        assert_eq!(
            fetch_entries(&bare)[0].url,
            "https://x.test",
            "no redirect keeps the title"
        );
    }

    #[test]
    fn the_workbench_opens_on_changes_and_tracks_its_state() {
        let mut workbench = super::Workbench::new();
        assert_eq!(workbench.tab(), Tab::Changes);
        workbench.set_tab(Tab::Agents);
        assert_eq!(workbench.tab(), Tab::Agents);
        workbench.set_filter(Filter::Running);
        assert_eq!(workbench.filter(), Filter::Running);
        workbench.set_file(Some("src/main.rs".to_owned()));
        assert_eq!(workbench.file(), Some("src/main.rs"));
        workbench.set_file(None);
        assert_eq!(workbench.file(), None);
        workbench.select(Some("c1".to_owned()));
        assert_eq!(workbench.selected(), Some("c1"));
        workbench.select(None);
        assert_eq!(workbench.selected(), None);
    }

    #[test]
    fn the_listing_ask_rides_the_session_it_was_made_for() {
        let mut workbench = super::Workbench::new();
        assert!(!workbench.listing_asked("s1"));
        workbench.mark_listing_asked("s1");
        assert!(workbench.listing_asked("s1"));
        assert!(!workbench.listing_asked("s2"), "a new session asks again");
    }

    #[test]
    fn a_shell_run_opens_on_its_default_and_flips_against_it() {
        let mut workbench = super::Workbench::new();
        // Running and the latest run start open; the rest folded.
        assert!(workbench.run_open("r1", true, false));
        assert!(workbench.run_open("r2", false, true));
        assert!(!workbench.run_open("r3", false, false));
        workbench.flip_run("r3");
        assert!(workbench.run_open("r3", false, false), "the flip opens it");
        workbench.flip_run("r3");
        assert!(
            !workbench.run_open("r3", false, false),
            "the second flip closes it"
        );
    }

    #[test]
    fn a_change_entry_carries_its_author() {
        let entry = ChangeEntry {
            call_id: "c1".to_owned(),
            path: "a.rs".to_owned(),
            add: 1,
            del: 0,
            created: false,
            by: Some("explore".to_owned()),
        };
        assert_eq!(entry.by.as_deref(), Some("explore"));
        let run = TermEntry {
            id: "c1".to_owned(),
            command: "ls".to_owned(),
            output: String::new(),
            hidden: 0,
            exit: None,
            running: false,
            failed: false,
            who: None,
        };
        assert_eq!(run.command, "ls");
        let listing = FsEntry {
            path: "src".to_owned(),
            kind: FsKind::Directory,
            size: 0,
        };
        assert_eq!(listing.kind, FsKind::Directory);
        let _ = Frame::Notification {
            method: "session/update".to_owned(),
            params: json!({"sessionId": "s", "update": {}}),
        };
    }
}

/// Paints one icon centred on `center`, inside a row drawn before it.
fn paint_icon(
    ui: &mut egui::Ui,
    icon: crate::icons::Icon,
    size: f32,
    color: egui::Color32,
    center: egui::Pos2,
) {
    crate::icons::themed_image(icon, size, color).paint_at(
        ui,
        egui::Rect::from_center_size(center, egui::vec2(size, size)),
    );
}
