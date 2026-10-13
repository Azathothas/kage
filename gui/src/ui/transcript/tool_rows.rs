//! The transcript's tool-bearing rows, drawn with egui widgets.
//!
//! One renderer per row kind that carries a tool call or its output
//! rather than prose: the single tool call with its expandable detail,
//! the collapsed run of same-family calls, the files a run changed, the
//! run's end, the todo list, the `exit_plan` review card and the
//! agent and swarm cards. The text rows live in their own module.
//!
//! Each renderer takes `&mut egui::Ui` and the data its row carries,
//! draws with the design's tokens and hands back an [`egui::Response`]
//! plus the actions the row offers, exactly as [`super::text_rows`]
//! does: the caller owns the store, the composer and the clipboard, so
//! a renderer never reaches past its own rect. The click that opens or
//! closes a detail, a body or a nested list is the one exception the
//! old view kept local, so it is applied here to the [`UiState`] the
//! caller threads in, the way the old element handlers toggled the
//! view's own state.

use std::f32::consts::TAU;
use std::time::Duration;

use kage_client::wire::{ContentBlock, NoticeTone, ToolCallStatus};
use kage_client::{Session, Subagent, ToolCallItem, TranscriptItem};

use super::model::{
    ChangeEntry, ChipTone, DiffLine, Family, Outcome, Row, RowKey, UiState, chips_for, diff_lines,
    plan_decision, tool_verb,
};
use crate::clock;
use crate::store::{PlanChoice, plan_review};
use crate::theme::{
    FAMILY_BODY_MEDIUM, FAMILY_BODY_REGULAR, FAMILY_MONO_BOLD, FAMILY_MONO_REGULAR, FS_2XS, FS_SM,
    FS_XS, Palette, R_FULL, R_LG, R_MD, R_SM, SP_1, SP_2, SP_3, SP_4, SP_5, family,
};
use crate::timing::{RunEnd, SessionTimes};
use crate::ui::agents::Phase;

/// The activity row's minimum height.
const ROW_H: f32 = 28.0;
/// The turn-end row's height: the action buttons it carries.
const TURN_END_H: f32 = 24.0;
/// The design's 12px detail size.
const DETAIL_SIZE: f32 = 12.0;
/// The design's small icon size.
const ICON_SM: f32 = 14.0;
/// The design's extra small icon size.
const ICON_XS: f32 = 12.0;
/// The shell output lines a detail shows before its earlier-lines
/// toggle.
const SHELL_TAIL: usize = 12;
/// The tallest a detail body grows before it scrolls.
const DETAIL_MAX_H: f32 = 320.0;
/// The tallest a plan document grows before it scrolls.
const PLAN_DOC_MAX_H: f32 = 420.0;
/// The left indent of a detail box or an expanded list.
const DETAIL_INDENT: f32 = 22.0;
/// The detail head's vertical padding.
const DETAIL_HEAD_PY: f32 = 7.0;
/// The mono body's vertical padding inside a detail box.
const DETAIL_PRE_PY: f32 = 10.0;
/// One turn of the working spinner.
const SPIN_TURN: f32 = 0.9;

/// What a tool row's actions ask the caller to do.
///
/// The renderer owns the drawing; the caller owns the store, the
/// composer, the clipboard and the workbench, so the actions travel
/// back out rather than reaching into the shell from inside a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowAction {
    /// Show this file in the workbench.
    OpenFile(String),
    /// Show the change this tool call made in the workbench.
    OpenChange(String),
    /// Show the fetched pages in the workbench.
    OpenBrowser,
    /// Show this subagent in the workbench.
    OpenAgent(String),
    /// Show the agents list in the workbench.
    OpenAgents,
    /// Answer the plan review with the given choice and revision text.
    Review(PlanChoice, Option<String>),
    /// Stop the subagent with this session id.
    CancelAgent(String),
    /// Resume the swarm members with these session ids.
    ResumeSwarm(Vec<String>),
    /// Copy this text to the clipboard.
    Copy(String),
    /// Put this message's text in the composer to edit and resend.
    Edit(String),
    /// Retry the run with this prompt.
    Retry(String),
    /// Fork the session through this item, when named.
    Fork(Option<usize>),
    /// Rewind the session to before the prompt at this item.
    Rewind(usize),
}

/// The plan card's revise field: which card's field shows, and the
/// text typed into it. The caller owns one across the transcript.
#[derive(Debug, Default, Clone)]
pub struct PlanRevise {
    /// The plan card whose revise field shows, by item index.
    pub open: Option<usize>,
    /// The text the revise field holds.
    pub text: String,
}

/// The monospace font of the design's paths and commands.
fn mono(size: f32) -> egui::FontId {
    egui::FontId::new(size, family(FAMILY_MONO_REGULAR))
}

/// A trailing right-to-left row inside the head it trails: right
/// aligned pills, vertically centred on the head's own height.
///
/// The height comes from what the head has already laid out (its
/// `set_min_height` at the row's top), and the sub-ui is allocated at
/// that height: a bare `with_layout(right_to_left(Align::Center))`
/// centres its contents in ALL the height the parent still offers, so
/// in the transcript's measure pass, where the offer is the whole
/// row budget, every such head measured the offer tall and the list
/// scrolled past every row but the last.
pub(crate) fn trailing(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    let height = ui.min_rect().height().max(16.0);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), height),
        egui::Layout::right_to_left(egui::Align::Center),
        contents,
    );
}

/// The tinted image one icon draws as.
fn icon_image(icon: crate::icons::Icon, color: egui::Color32) -> egui::Image<'static> {
    egui::Image::new(egui::ImageSource::Bytes {
        uri: icon.uri(),
        bytes: icon.bytes().into(),
    })
    .tint(color)
    .max_size(egui::Vec2::splat(ICON_SM))
}

/// One icon of the design's catalog at its small size.
fn icon(ui: &mut egui::Ui, icon: crate::icons::Icon, color: egui::Color32) -> egui::Response {
    crate::icons::icon(ui, icon, ICON_SM, color)
}

/// One icon of the design's catalog at its extra small size.
fn icon_xs(ui: &mut egui::Ui, icon: crate::icons::Icon, color: egui::Color32) -> egui::Response {
    crate::icons::icon(ui, icon, ICON_XS, color)
}

/// The chevron a collapsible head trails with: pointing right when
/// closed, down when open.
fn chevron(ui: &mut egui::Ui, open: bool, color: egui::Color32) -> egui::Response {
    let glyph = if open {
        crate::icons::Icon::ChevronDown
    } else {
        crate::icons::Icon::ChevronRight
    };
    icon(ui, glyph, color)
}

/// The working spinner: an arc turning once every [`SPIN_TURN`],
/// asking for the frames it turns through.
fn spinner(ui: &mut egui::Ui, color: egui::Color32) -> egui::Response {
    ui.ctx().request_repaint();
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ICON_SM, ICON_SM), egui::Sense::hover());
    let center = rect.center();
    let radius = ICON_SM / 2.0 - 1.0;
    let turn = (ui.ctx().input(|i| i.time) % f64::from(SPIN_TURN)) as f32 / SPIN_TURN;
    let start = turn * TAU;
    let steps = 24;
    let points: Vec<egui::Pos2> = (0..=steps)
        .map(|step| {
            let angle = start + TAU * 0.75 * (step as f32 / steps as f32);
            egui::pos2(
                center.x + radius * angle.cos(),
                center.y + radius * angle.sin(),
            )
        })
        .collect();
    ui.painter()
        .add(egui::Shape::line(points, egui::Stroke::new(2.0, color)));
    response
}

/// A clickable row that fills the available width, painting `hover`
/// behind its contents while hovered and reporting the click.
///
/// The contents draw into a scope so their union rect is known, then
/// that rect is stretched to the full column width and interacted
/// with. The hover fill is painted behind the contents through a shape
/// reserved before they draw, so the text is never tinted.
fn clickable_row(
    ui: &mut egui::Ui,
    hover: egui::Color32,
    radius: egui::CornerRadius,
    contents: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let background = ui.painter().add(egui::Shape::Noop);
    let left = ui.max_rect().left();
    let right = ui.max_rect().right();
    let inner = ui.scope(|ui| {
        ui.set_min_width(ui.available_width());
        contents(ui);
    });
    let mut rect = inner.response.rect;
    rect.min.x = left;
    rect.max.x = right.max(rect.max.x);
    let response = ui.interact(
        rect,
        ui.id().with(("row", ui.next_auto_id())),
        egui::Sense::click(),
    );
    if response.hovered() {
        ui.painter()
            .set(background, egui::Shape::rect_filled(rect, radius, hover));
    }
    response
}

/// The tone a small action button carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BtnTone {
    /// The plain hairline button.
    Plain,
    /// The filled accent button, for the main action.
    Primary,
    /// The outlined danger button, for the refuse answer.
    Danger,
}

/// One small action button as the design draws it: 26px tall, an 8px
/// radius, medium small text, in one tone, with an optional leading
/// icon. The caller reads the click off the returned response.
fn btn_sm(
    ui: &mut egui::Ui,
    palette: &Palette,
    tone: BtnTone,
    label: &str,
    glyph: Option<crate::icons::Icon>,
) -> egui::Response {
    let (fill, stroke, text) = match tone {
        BtnTone::Plain => (palette.fill, palette.line, palette.ink),
        BtnTone::Primary => (palette.accent, palette.accent, egui::Color32::WHITE),
        BtnTone::Danger => (
            egui::Color32::TRANSPARENT,
            palette.danger_bd,
            palette.danger,
        ),
    };
    let rich = egui::RichText::new(label)
        .size(FS_XS)
        .color(text)
        .font(egui::FontId::new(FS_XS, family(FAMILY_BODY_MEDIUM)));
    let mut button = match glyph {
        Some(glyph) => egui::Button::image_and_text(icon_image(glyph, text), rich),
        None => egui::Button::new(rich),
    };
    button = button
        .fill(fill)
        .stroke(egui::Stroke::new(1.0, stroke))
        .corner_radius(egui::CornerRadius::same(R_MD as u8))
        .min_size(egui::vec2(0.0, 26.0))
        .frame(true);
    ui.add(button)
}

/// The colored chip of the design's chip anatomy: fully round, mono at
/// the smallest size, on the caller's tint.
fn chip(ui: &mut egui::Ui, text: &str, fg: egui::Color32, bg: egui::Color32) {
    egui::Frame::NONE
        .fill(bg)
        .corner_radius(egui::CornerRadius::same(R_FULL as u8))
        .inner_margin(egui::Margin::symmetric(7, 1))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).font(mono(FS_2XS)).color(fg));
        });
}

/// The state pill of the design: fully round, a tinted border and fill,
/// small text in the tint's foreground.
fn pill(
    ui: &mut egui::Ui,
    text: &str,
    fg: egui::Color32,
    bg: egui::Color32,
    border: egui::Color32,
) {
    egui::Frame::NONE
        .fill(bg)
        .stroke(egui::Stroke::new(1.0, border))
        .corner_radius(egui::CornerRadius::same(R_FULL as u8))
        .inner_margin(egui::Margin::symmetric(7, 1))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).size(10.5).color(fg));
        });
}

/// The foreground and background one chip tone renders in.
fn chip_tone(tone: ChipTone, palette: &Palette) -> (egui::Color32, egui::Color32) {
    match tone {
        ChipTone::Neutral => (palette.muted, palette.selected),
        ChipTone::Good => (palette.diff_add, palette.diff_add_bg),
        ChipTone::Bad => (palette.diff_del, palette.diff_del_bg),
    }
}

/// The icon one tool title renders with.
fn tool_icon(title: &str) -> crate::icons::Icon {
    match title {
        "read" => crate::icons::Icon::File,
        "grep" | "find" => crate::icons::Icon::Search,
        "ls" => crate::icons::Icon::ListTree,
        "shell" => crate::icons::Icon::Terminal,
        "edit" => crate::icons::Icon::Pencil,
        "write" => crate::icons::Icon::FilePlus,
        "web_search" | "web_fetch" => crate::icons::Icon::Globe,
        "todo_list" => crate::icons::Icon::ListTodo,
        "ask_user_question" => crate::icons::Icon::MessageSquare,
        "agent" => crate::icons::Icon::Bot,
        "swarm" => crate::icons::Icon::Waypoints,
        _ => crate::icons::Icon::Zap,
    }
}

/// `text` without its ANSI escape sequences: a run of styled output
/// shows as its plain characters, never as markup.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
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
            _ => {}
        }
    }
    out
}

/// A string argument of the tool input.
fn input_str<'a>(input: Option<&'a serde_json::Value>, key: &str) -> &'a str {
    input
        .and_then(|input| input.get(key))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
}

/// One result a `web_search` call returned.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SearchResult {
    title: String,
    url: String,
    snippet: String,
}

/// The results a finished `web_search` call carried as data.
fn search_results(call: &ToolCallItem) -> Option<Vec<SearchResult>> {
    if call.title != "web_search" {
        return None;
    }
    let results = call.raw_output.as_ref()?.get("results")?.as_array()?;
    Some(
        results
            .iter()
            .map(|result| {
                let text = |key: &str| result[key].as_str().unwrap_or_default().to_owned();
                SearchResult {
                    title: text("title"),
                    url: text("url"),
                    snippet: text("snippet"),
                }
            })
            .collect(),
    )
}

/// Where one todo stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoState {
    /// Not started.
    Pending,
    /// The one in progress.
    Running,
    /// Finished.
    Done,
    /// Given up on.
    Failed,
}

/// One todo of a todos row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Todo {
    /// The todo's text.
    pub text: String,
    /// Where the todo stands.
    pub state: TodoState,
}

/// The todos of an ACP plan update: `content` and `status` per entry.
#[must_use]
pub fn todos_of_plan(entries: &[serde_json::Value]) -> Vec<Todo> {
    entries
        .iter()
        .map(|entry| Todo {
            text: entry
                .get("content")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            state: match entry.get("status").and_then(serde_json::Value::as_str) {
                Some("completed") => TodoState::Done,
                Some("in_progress") => TodoState::Running,
                _ => TodoState::Pending,
            },
        })
        .collect()
}

/// The todos a `todo_list` call wrote: `title` and `status` per entry.
#[must_use]
pub fn todos_of_call(call: &ToolCallItem) -> Option<Vec<Todo>> {
    if call.title != "todo_list" {
        return None;
    }
    let todos = call.input.as_ref()?.get("todos")?.as_array()?;
    Some(
        todos
            .iter()
            .map(|todo| Todo {
                text: todo
                    .get("title")
                    .or_else(|| todo.get("content"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                state: match todo.get("status").and_then(serde_json::Value::as_str) {
                    Some("done" | "completed") => TodoState::Done,
                    Some("in_progress") => TodoState::Running,
                    Some("failed") => TodoState::Failed,
                    _ => TodoState::Pending,
                },
            })
            .collect(),
    )
}

/// The text of a prompt: its text blocks in order, a blank line apart.
fn prompt_text(content: &[ContentBlock]) -> String {
    content
        .iter()
        .filter_map(ContentBlock::as_text)
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// The prompt that started the run ending at `end`, and the reply text
/// the run produced, for the turn-end actions.
fn run_texts(session: &Session, end: usize) -> (Option<String>, String) {
    let start = session.items[..end]
        .iter()
        .rposition(|item| matches!(item, TranscriptItem::User { steered: false, .. }));
    let prompt = start.and_then(|ix| match &session.items[ix] {
        TranscriptItem::User { content, .. } => Some(prompt_text(content)),
        _ => None,
    });
    let reply = session.items[start.map_or(0, |ix| ix + 1)..end]
        .iter()
        .filter_map(|item| match item {
            TranscriptItem::Assistant { text } => Some(text.trim()),
            _ => None,
        })
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    // A run woken by agent text has no prompt of the user's to retry.
    let typed = |text: &String| {
        !text.is_empty()
            && kage_client::agent_text::split(text).is_none()
            && kage_client::agent_text::engine_note(text).is_none()
            && kage_client::agent_text::compaction_summary(text).is_none()
    };
    (prompt.filter(typed), reply)
}

/// Opens or closes one row key in the expansion set, the way the old
/// view's `toggle` did.
fn toggle(ui_state: &mut UiState, key: RowKey) {
    if !ui_state.expanded.remove(&key) {
        ui_state.expanded.insert(key);
    }
}

/// One tool call: state icon, verb, target, honest chips, and the
/// detail box when expanded.
pub fn render_tool(
    ui: &mut egui::Ui,
    ix: usize,
    call: &ToolCallItem,
    nested: bool,
    first_nested: bool,
    took: Option<Duration>,
    ui_state: &mut UiState,
) -> (egui::Response, Vec<RowAction>) {
    let palette = Palette::active_of(ui.ctx());
    let mut actions = Vec::new();
    let (verb, target) = tool_verb(call);
    let live_shell = call.title == "shell"
        && call.status == ToolCallStatus::InProgress
        && !call.text().is_empty();
    let expanded = ui_state.expanded.contains(&RowKey::Item(ix)) || live_shell;
    let running = matches!(
        call.status,
        ToolCallStatus::InProgress | ToolCallStatus::Pending
    );
    let failed = call.status == ToolCallStatus::Failed;
    let output = call.text();
    let diff = if call.title == "edit" || call.title == "write" {
        diff_lines(call)
    } else {
        None
    };
    let verb_color = if failed {
        palette.danger
    } else if running {
        palette.muted
    } else {
        palette.ink
    };

    let clickable = diff.is_some() || !output.is_empty();
    let response = clickable_row(
        ui,
        palette.hover,
        egui::CornerRadius::same(R_SM as u8),
        |ui| {
            ui.spacing_mut().item_spacing.x = SP_4;
            ui.set_min_height(ROW_H);
            if nested {
                ui.add_space(SP_5);
            } else {
                ui.add_space(SP_3);
            }
            if failed {
                icon(ui, crate::icons::Icon::CircleX, palette.danger);
            } else if running {
                spinner(ui, palette.accent);
            } else {
                icon(ui, tool_icon(&call.title), palette.muted);
            }
            ui.label(egui::RichText::new(&verb).size(FS_SM).color(verb_color));
            ui.add(
                egui::Label::new(
                    egui::RichText::new(&target)
                        .font(mono(DETAIL_SIZE))
                        .color(palette.muted),
                )
                .truncate(),
            );
            crate::ui::transcript::tool_rows::trailing(ui, |ui| {
                if clickable {
                    chevron(ui, expanded, palette.faint);
                }
                if let Some(took) = took {
                    ui.add_space(SP_4);
                    ui.label(
                        egui::RichText::new(clock::span(took))
                            .font(mono(FS_2XS))
                            .color(palette.faint),
                    );
                }
                if failed {
                    ui.add_space(SP_4);
                    chip(ui, "failed", palette.danger, palette.danger_soft);
                }
                for mark in chips_for(call) {
                    ui.add_space(SP_4);
                    let (fg, bg) = chip_tone(mark.tone, palette);
                    chip(ui, &mark.text, fg, bg);
                }
                if call.status == ToolCallStatus::Pending {
                    ui.add_space(SP_4);
                    chip(ui, "waiting", palette.muted, palette.selected);
                }
            });
        },
    );
    if response.clicked() && clickable {
        toggle(ui_state, RowKey::Item(ix));
    }

    let mut more_click = false;
    if expanded {
        ui.add_space(SP_2);
        ui.horizontal_top(|ui| {
            ui.add_space(DETAIL_INDENT);
            ui.vertical(|ui| {
                let path = input_str(call.input.as_ref(), "path").to_owned();
                if let Some(lines) = &diff {
                    let glyph = if call.title == "write" {
                        crate::icons::Icon::FilePlus
                    } else {
                        crate::icons::Icon::FileDiff
                    };
                    if detail(ui, palette, Some((glyph, path.clone())), |ui| {
                        render_diff(ui, lines, palette);
                    }) {
                        actions.push(RowAction::OpenChange(call.tool_call_id.clone()));
                    }
                } else if call.title == "shell" && !output.is_empty() {
                    let lines: Vec<&str> = output.lines().collect();
                    let all = ui_state.full_output.contains(&ix);
                    let hidden = if all {
                        0
                    } else {
                        lines.len().saturating_sub(SHELL_TAIL)
                    };
                    let command = input_str(call.input.as_ref(), "command").to_owned();
                    let shown = strip_ansi(&lines[hidden..].join("\n"));
                    let _ = detail(
                        ui,
                        palette,
                        Some((crate::icons::Icon::Terminal, format!("$ {command}"))),
                        |ui| render_pre(ui, &shown, palette),
                    );
                    if hidden > 0 {
                        let label = format!("{hidden} earlier lines");
                        let foot = clickable_row(ui, palette.hover, 0.0.into(), |ui| {
                            ui.add_space(SP_5);
                            ui.set_min_height(20.0);
                            ui.label(egui::RichText::new(&label).size(FS_XS).color(palette.faint));
                        });
                        if foot.clicked() {
                            more_click = true;
                        }
                    }
                } else if let Some(results) = search_results(call)
                    && !results.is_empty()
                {
                    let _ = detail(ui, palette, None, |ui| {
                        render_search_results(ui, &results, palette);
                    });
                } else if !output.is_empty() {
                    let head = match call.title.as_str() {
                        "read" => Some((crate::icons::Icon::File, path.clone())),
                        "web_fetch" => Some((
                            crate::icons::Icon::Globe,
                            input_str(call.input.as_ref(), "url").to_owned(),
                        )),
                        _ => None,
                    };
                    let fired = detail(ui, palette, head, |ui| render_pre(ui, &output, palette));
                    if fired {
                        match call.title.as_str() {
                            "read" => actions.push(RowAction::OpenFile(path)),
                            "web_fetch" => actions.push(RowAction::OpenBrowser),
                            _ => {}
                        }
                    }
                }
            });
        });
    }
    if more_click {
        ui_state.full_output.insert(ix);
    }
    let _ = first_nested;
    (response, actions)
}

/// The detail box under a tool row: the head, the body capped at the
/// design's 320px and scrolling past it. The head's action button
/// reports its click, so the caller can turn it into a [`RowAction`].
fn detail(
    ui: &mut egui::Ui,
    palette: &Palette,
    head: Option<(crate::icons::Icon, String)>,
    body: impl FnOnce(&mut egui::Ui),
) -> bool {
    let mut fired = false;
    egui::Frame::NONE
        .fill(palette.well)
        .stroke(egui::Stroke::new(1.0, palette.line))
        .corner_radius(egui::CornerRadius::same(R_LG as u8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            if let Some((glyph, text)) = head {
                ui.horizontal(|ui| {
                    ui.set_min_width(ui.available_width());
                    ui.spacing_mut().item_spacing.x = SP_4;
                    ui.add_space(SP_5);
                    ui.set_min_height(DETAIL_HEAD_PY * 2.0 + ICON_XS);
                    icon_xs(ui, glyph, palette.muted);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&text)
                                .font(mono(FS_XS))
                                .color(palette.ink),
                        )
                        .truncate(),
                    );
                    crate::ui::transcript::tool_rows::trailing(ui, |ui| {
                        if btn_sm(ui, palette, BtnTone::Plain, "Open", None).clicked() {
                            fired = true;
                        }
                    });
                });
                hairline(ui, palette);
            }
            egui::ScrollArea::vertical()
                .max_height(DETAIL_MAX_H)
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    body(ui);
                });
        });
    fired
}

/// A one pixel rule in the faintest line token, across the full width.
fn hairline(ui: &mut egui::Ui, palette: &Palette) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::ZERO, palette.subtle);
}

/// The mono body of a detail box: the design's deep well at its 12px
/// detail size.
fn render_pre(ui: &mut egui::Ui, text: &str, palette: &Palette) {
    egui::Frame::NONE
        .fill(palette.deep)
        .inner_margin(egui::Margin::symmetric(SP_5 as i8, DETAIL_PRE_PY as i8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.add(
                egui::Label::new(
                    egui::RichText::new(text)
                        .font(mono(DETAIL_SIZE))
                        .color(palette.ink),
                )
                .selectable(true),
            );
        });
}

/// A unified diff with per-line added and removed coloring, computed
/// from the markers the delivered text carries.
fn render_diff(ui: &mut egui::Ui, lines: &[DiffLine], palette: &Palette) {
    egui::Frame::NONE.fill(palette.deep).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        for line in lines {
            let (color, bg) = match line {
                DiffLine::Add(_) => (palette.diff_add, palette.diff_add_bg),
                DiffLine::Del(_) => (palette.diff_del, palette.diff_del_bg),
                DiffLine::Head(_) => (palette.faint, palette.selected),
                DiffLine::Ctx(_) => (palette.ink, palette.deep),
            };
            egui::Frame::NONE.fill(bg).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.add_space(4.0);
                    ui.add_sized(
                        egui::vec2(16.0, DETAIL_SIZE),
                        egui::Label::new(
                            egui::RichText::new(line.marker())
                                .font(mono(DETAIL_SIZE))
                                .color(palette.ghost),
                        ),
                    );
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(line.body())
                                .font(mono(DETAIL_SIZE))
                                .color(color),
                        )
                        .truncate(),
                    );
                });
            });
        }
    });
}

/// The results of a `web_search` call: title, URL and snippet each.
fn render_search_results(ui: &mut egui::Ui, results: &[SearchResult], palette: &Palette) {
    ui.vertical(|ui| {
        for (n, result) in results.iter().enumerate() {
            if n > 0 {
                hairline(ui, palette);
            }
            ui.vertical(|ui| {
                ui.add_space(SP_4);
                ui.horizontal(|ui| {
                    ui.add_space(SP_5);
                    ui.label(
                        egui::RichText::new(&result.title)
                            .size(FS_SM)
                            .color(palette.ink_strong),
                    );
                });
                ui.horizontal(|ui| {
                    ui.add_space(SP_5);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&result.url)
                                .font(mono(11.0))
                                .color(palette.faint),
                        )
                        .truncate(),
                    );
                });
                if !result.snippet.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        ui.add_space(SP_5);
                        ui.label(
                            egui::RichText::new(&result.snippet)
                                .size(FS_XS)
                                .color(palette.muted),
                        );
                    });
                }
                ui.add_space(SP_4);
            });
        }
    });
}

/// The files a run changed, as the design's changes card: the count
/// and line totals with Review, then one row per file. Any row opens
/// that change in the workbench.
pub fn render_changes(
    ui: &mut egui::Ui,
    entries: &[ChangeEntry],
    end: usize,
) -> (egui::Response, Vec<RowAction>) {
    let palette = Palette::active_of(ui.ctx());
    let mut actions = Vec::new();
    let add: usize = entries.iter().map(|entry| entry.add).sum();
    let del: usize = entries.iter().map(|entry| entry.del).sum();
    let count = entries.len();
    let response = egui::Frame::NONE
        .fill(palette.surface)
        .stroke(egui::Stroke::new(1.0, palette.line))
        .corner_radius(egui::CornerRadius::same(R_LG as u8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = SP_4;
                ui.add_space(SP_5);
                ui.set_min_height(44.0);
                icon(ui, crate::icons::Icon::FileDiff, palette.faint);
                ui.label(
                    egui::RichText::new(format!(
                        "{count} file{} changed",
                        if count == 1 { "" } else { "s" }
                    ))
                    .size(FS_SM)
                    .color(palette.ink_strong),
                );
                crate::ui::transcript::tool_rows::trailing(ui, |ui| {
                    if let Some(first) = entries.first() {
                        ui.add_space(SP_4);
                        if btn_sm(ui, palette, BtnTone::Plain, "Review", None).clicked() {
                            actions.push(RowAction::OpenChange(first.call_id.clone()));
                        }
                    }
                    if del > 0 {
                        ui.add_space(SP_4);
                        chip(
                            ui,
                            &format!("-{del}"),
                            palette.diff_del,
                            palette.diff_del_bg,
                        );
                    }
                    ui.add_space(SP_4);
                    chip(
                        ui,
                        &format!("+{add}"),
                        palette.diff_add,
                        palette.diff_add_bg,
                    );
                });
            });
            for (n, entry) in entries.iter().enumerate() {
                if n > 0 {
                    hairline(ui, palette);
                }
                let clicked = clickable_row(ui, palette.hover, 0.0.into(), |ui| {
                    ui.spacing_mut().item_spacing.x = SP_4;
                    ui.add_space(SP_5);
                    ui.set_min_height(32.0);
                    let glyph = if entry.created {
                        crate::icons::Icon::FilePlus
                    } else {
                        crate::icons::Icon::File
                    };
                    icon(ui, glyph, palette.faint);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&entry.path)
                                .font(mono(DETAIL_SIZE))
                                .color(palette.ink),
                        )
                        .truncate(),
                    );
                    crate::ui::transcript::tool_rows::trailing(ui, |ui| {
                        if entry.del > 0 {
                            ui.add_space(SP_4);
                            chip(
                                ui,
                                &format!("-{}", entry.del),
                                palette.diff_del,
                                palette.diff_del_bg,
                            );
                        }
                        ui.add_space(SP_4);
                        chip(
                            ui,
                            &format!("+{}", entry.add),
                            palette.diff_add,
                            palette.diff_add_bg,
                        );
                        if entry.created {
                            ui.add_space(SP_4);
                            pill(ui, "new", palette.ok, palette.ok_soft, palette.ok_bd);
                        }
                    });
                });
                if clicked.clicked() {
                    actions.push(RowAction::OpenChange(entry.call_id.clone()));
                }
            }
        })
        .response;
    let _ = end;
    (response, actions)
}

/// A collapsed run of same-family tool calls, expandable to list its
/// members.
pub fn render_group(
    ui: &mut egui::Ui,
    session: &Session,
    row: &Row,
    times: Option<&SessionTimes>,
    ui_state: &mut UiState,
) -> (egui::Response, Vec<RowAction>) {
    let mut actions = Vec::new();
    let Row::Group {
        key,
        label,
        family,
        members,
        failed,
        running,
        expanded,
    } = row
    else {
        return (
            ui.allocate_response(egui::Vec2::ZERO, egui::Sense::hover()),
            actions,
        );
    };
    let (key, family, failed, running, expanded) = (*key, *family, *failed, *running, *expanded);
    let palette = Palette::active_of(ui.ctx());
    let group_icon = match family {
        Family::Shell => crate::icons::Icon::Terminal,
        Family::Explore => crate::icons::Icon::Search,
    };

    let response = clickable_row(
        ui,
        palette.hover,
        egui::CornerRadius::same(R_SM as u8),
        |ui| {
            ui.spacing_mut().item_spacing.x = SP_4;
            ui.add_space(SP_3);
            ui.set_min_height(ROW_H);
            if running {
                spinner(ui, palette.accent);
            } else {
                icon(ui, group_icon, palette.muted);
            }
            ui.label(egui::RichText::new(label).size(FS_SM).color(if running {
                palette.ink_strong
            } else {
                palette.ink
            }));
            crate::ui::transcript::tool_rows::trailing(ui, |ui| {
                chevron(ui, expanded, palette.faint);
                if failed > 0 {
                    ui.add_space(SP_4);
                    chip(
                        ui,
                        &format!("{failed} failed"),
                        palette.danger,
                        palette.danger_soft,
                    );
                }
            });
        },
    );
    if response.clicked() {
        toggle(ui_state, key);
    }

    if expanded {
        ui.add_space(SP_2);
        egui::Frame::NONE
            .fill(palette.well)
            .stroke(egui::Stroke::new(1.0, palette.line))
            .corner_radius(egui::CornerRadius::same(R_LG as u8))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                for (n, member) in members.iter().enumerate() {
                    if let Some(TranscriptItem::ToolCall(call)) = session.items.get(*member) {
                        let took = times.and_then(|t| t.tool(&call.tool_call_id));
                        let (_, member_actions) =
                            render_tool(ui, *member, call, true, n == 0, took, ui_state);
                        actions.extend(member_actions);
                    }
                }
            });
    }
    (response, actions)
}

/// The end of a run: when it ended and how long it ran, when this
/// client watched it, the stop outcome, and the run's actions.
pub fn render_turn_end(
    ui: &mut egui::Ui,
    session: &Session,
    ix: usize,
    outcome: Option<Outcome>,
    end: Option<RunEnd>,
    pinned: bool,
) -> (egui::Response, Vec<RowAction>) {
    let palette = Palette::active_of(ui.ctx());
    let mut actions = Vec::new();
    let (prompt, reply) = run_texts(session, ix);
    let mut copy = false;
    let mut retry = false;
    let mut fork = false;
    let mut rewind = false;
    let response = ui
        .horizontal(|ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing.x = SP_4;
            ui.set_min_height(TURN_END_H);
            if let Some(end) = end {
                ui.label(
                    egui::RichText::new(clock::time_of_day(end.at))
                        .size(FS_2XS)
                        .color(palette.faint),
                );
                ui.label(
                    egui::RichText::new("\u{b7}")
                        .size(FS_2XS)
                        .color(palette.faint),
                );
                ui.label(
                    egui::RichText::new(clock::span(end.took))
                        .font(mono(FS_2XS))
                        .color(palette.faint),
                );
            }
            if let Some(outcome) = outcome {
                ui.add_space(SP_4);
                match outcome {
                    Outcome::Interrupted => {
                        chip(ui, "interrupted", palette.muted, palette.selected);
                    }
                    Outcome::Failed => {
                        chip(ui, "failed", palette.danger, palette.danger_soft);
                    }
                }
            }
            crate::ui::transcript::tool_rows::trailing(ui, |ui| {
                if !pinned {
                    ui.add_space(SP_1);
                    if icon_button(ui, crate::icons::Icon::Undo2, "Rewind to here", palette)
                        .clicked()
                    {
                        rewind = true;
                    }
                    ui.add_space(SP_1);
                    if icon_button(ui, crate::icons::Icon::GitFork, "Fork from here", palette)
                        .clicked()
                    {
                        fork = true;
                    }
                }
                if prompt.is_some() {
                    ui.add_space(SP_1);
                    if icon_button(ui, crate::icons::Icon::RefreshCw, "Retry", palette).clicked() {
                        retry = true;
                    }
                }
                if !reply.is_empty() {
                    ui.add_space(SP_1);
                    if icon_button(ui, crate::icons::Icon::Copy, "Copy response", palette).clicked()
                    {
                        copy = true;
                    }
                }
            });
        })
        .response;
    if copy {
        actions.push(RowAction::Copy(reply));
    }
    if retry && let Some(prompt) = prompt {
        actions.push(RowAction::Retry(prompt));
    }
    if fork {
        actions.push(RowAction::Fork(Some(ix)));
    }
    if rewind {
        actions.push(RowAction::Rewind(ix));
    }
    (response, actions)
}

/// A quiet icon button: no frame, the hover fill painted behind it.
fn icon_button(
    ui: &mut egui::Ui,
    glyph: crate::icons::Icon,
    tooltip: &str,
    palette: &Palette,
) -> egui::Response {
    let background = ui.painter().add(egui::Shape::Noop);
    let response = ui.add(egui::Button::image(icon_image(glyph, palette.muted)));
    if response.hovered() {
        ui.painter().set(
            background,
            egui::Shape::rect_filled(
                response.rect,
                egui::CornerRadius::same(4),
                palette.fill_hover,
            ),
        );
    }
    response.on_hover_text(tooltip)
}

/// The todos row: Updated todos, the one in progress or all done, the
/// done count, and the list when expanded.
pub fn render_todos(
    ui: &mut egui::Ui,
    ix: usize,
    todos: &[Todo],
    ui_state: &mut UiState,
) -> (egui::Response, Vec<RowAction>) {
    let palette = Palette::active_of(ui.ctx());
    let expanded = ui_state.expanded.contains(&RowKey::Item(ix));
    let done = todos
        .iter()
        .filter(|todo| todo.state == TodoState::Done)
        .count();
    let complete = done == todos.len() && !todos.is_empty();
    let current = todos
        .iter()
        .find(|todo| todo.state == TodoState::Running)
        .map(|todo| todo.text.clone())
        .unwrap_or_default();

    let response = clickable_row(
        ui,
        palette.hover,
        egui::CornerRadius::same(R_SM as u8),
        |ui| {
            ui.spacing_mut().item_spacing.x = SP_4;
            ui.add_space(SP_3);
            ui.set_min_height(ROW_H);
            icon(ui, crate::icons::Icon::ListTodo, palette.muted);
            ui.label(
                egui::RichText::new("Updated todos")
                    .size(FS_SM)
                    .color(palette.ink),
            );
            ui.add(
                egui::Label::new(
                    egui::RichText::new(if complete {
                        "all done".to_owned()
                    } else {
                        current
                    })
                    .font(mono(DETAIL_SIZE))
                    .color(palette.muted),
                )
                .truncate(),
            );
            crate::ui::transcript::tool_rows::trailing(ui, |ui| {
                chevron(ui, expanded, palette.faint);
                ui.add_space(SP_4);
                let (fg, bg) = if complete {
                    (palette.ok, palette.ok_soft)
                } else {
                    (palette.muted, palette.selected)
                };
                chip(ui, &format!("{done}/{}", todos.len()), fg, bg);
            });
        },
    );
    if response.clicked() {
        toggle(ui_state, RowKey::Item(ix));
    }

    if expanded {
        ui.add_space(SP_2);
        egui::Frame::NONE
            .fill(palette.well)
            .stroke(egui::Stroke::new(1.0, palette.line))
            .corner_radius(egui::CornerRadius::same(R_LG as u8))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal_top(|ui| {
                    ui.add_space(DETAIL_INDENT);
                    ui.vertical(|ui| {
                        ui.add_space(SP_3);
                        for todo in todos {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 9.0;
                                ui.add_space(SP_5);
                                ui.set_min_height(20.0);
                                let text_color = match todo.state {
                                    TodoState::Done => palette.ok,
                                    TodoState::Running => palette.accent,
                                    TodoState::Failed => palette.danger,
                                    TodoState::Pending => palette.ghost,
                                };
                                if todo.state == TodoState::Running {
                                    spinner(ui, palette.accent);
                                } else {
                                    let glyph = match todo.state {
                                        TodoState::Done => crate::icons::Icon::CircleCheck,
                                        TodoState::Failed => crate::icons::Icon::CircleX,
                                        _ => crate::icons::Icon::Circle,
                                    };
                                    icon_xs(ui, glyph, text_color);
                                }
                                let mut text = egui::RichText::new(&todo.text).size(FS_SM).color(
                                    if todo.state == TodoState::Done {
                                        palette.muted
                                    } else {
                                        palette.ink
                                    },
                                );
                                if todo.state == TodoState::Done {
                                    text = text.strikethrough();
                                }
                                ui.add(egui::Label::new(text).wrap_mode(egui::TextWrapMode::Wrap));
                            });
                            ui.add_space(SP_1);
                        }
                        ui.add_space(SP_2);
                    });
                });
            });
    }
    (response, Vec::new())
}

/// The plan an `exit_plan` call presents, as the design's plan card:
/// the title from the document's first heading, the review state, the
/// markdown document while open, and the three answers while the
/// review is pending.
pub fn render_plan_card(
    ui: &mut egui::Ui,
    pipeline: &mut super::markdown::Pipeline,
    session: &Session,
    ix: usize,
    call: &ToolCallItem,
    revise: &mut PlanRevise,
    ui_state: &mut UiState,
) -> (egui::Response, Vec<RowAction>) {
    let palette = Palette::active_of(ui.ctx());
    let mut actions = Vec::new();
    let pending = plan_review(session).is_some_and(|review| review.call_id == call.tool_call_id);
    let doc = input_str(call.input.as_ref(), "plan").to_owned();
    let title = doc
        .lines()
        .find_map(|line| line.strip_prefix("# "))
        .unwrap_or("Plan")
        .to_owned();
    let state: Option<(&str, egui::Color32, egui::Color32, egui::Color32)> = if pending {
        Some((
            "Pending review",
            palette.accent,
            palette.accent_soft,
            palette.accent_bd,
        ))
    } else {
        plan_decision(session, ix).map(|choice| match choice {
            PlanChoice::Approve => ("Approved", palette.ok, palette.ok_soft, palette.ok_bd),
            PlanChoice::Revise => ("Revising", palette.warn, palette.warn_soft, palette.warn_bd),
            PlanChoice::Reject => (
                "Rejected",
                palette.danger,
                palette.danger_soft,
                palette.danger_bd,
            ),
        })
    };
    let open = pending != ui_state.expanded.contains(&RowKey::Item(ix));

    let card = egui::Frame::NONE
        .fill(palette.surface)
        .stroke(egui::Stroke::new(1.0, palette.accent_bd))
        .corner_radius(egui::CornerRadius::same(R_LG as u8));
    let head = card.show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        clickable_row(
            ui,
            palette.hover,
            egui::CornerRadius::same(R_LG as u8),
            |ui| {
                ui.spacing_mut().item_spacing.x = SP_4;
                ui.add_space(SP_5);
                ui.set_min_height(44.0);
                egui::Frame::NONE
                    .fill(palette.accent_soft)
                    .corner_radius(egui::CornerRadius::same(R_MD as u8))
                    .show(ui, |ui| {
                        ui.set_min_size(egui::vec2(28.0, 28.0));
                        ui.centered_and_justified(|ui| {
                            icon(ui, crate::icons::Icon::PenLine, palette.accent);
                        });
                    });
                ui.add_space(SP_4);
                ui.vertical(|ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&title)
                                .size(FS_SM)
                                .color(palette.ink_strong),
                        )
                        .selectable(false)
                        .truncate(),
                    );
                    ui.label(
                        egui::RichText::new("Plan written in plan mode")
                            .size(FS_XS)
                            .color(palette.faint),
                    );
                });
                crate::ui::transcript::tool_rows::trailing(ui, |ui| {
                    icon(
                        ui,
                        if open {
                            crate::icons::Icon::ChevronUp
                        } else {
                            crate::icons::Icon::ChevronDown
                        },
                        palette.faint,
                    );
                    if let Some((label, fg, bg, border)) = state {
                        ui.add_space(SP_4);
                        pill(ui, label, fg, bg, border);
                    }
                });
            },
        )
    });
    if head.response.clicked() {
        toggle(ui_state, RowKey::Item(ix));
    }
    let response = head.response;

    if open && !doc.is_empty() {
        card.show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            egui::ScrollArea::vertical()
                .max_height(PLAN_DOC_MAX_H)
                .show(ui, |ui| {
                    ui.add_space(SP_5);
                    ui.horizontal(|ui| {
                        ui.add_space(SP_5);
                        ui.vertical(|ui| {
                            let id = format!("plan-{ix}");
                            pipeline.show(&id, &doc, false, ui);
                        });
                    });
                    ui.add_space(SP_5);
                });
        });
    }

    if pending {
        let revising = revise.open == Some(ix);
        let mut approve = false;
        let mut revise_click = false;
        let mut reject = false;
        let mut submit = false;
        card.show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = SP_4;
                ui.add_space(SP_5);
                ui.set_min_height(20.0);
                ui.add_space(SP_3);
                ui.label(
                    egui::RichText::new("Ready to build with this plan?")
                        .size(FS_SM)
                        .color(palette.ink_strong),
                );
                crate::ui::transcript::tool_rows::trailing(ui, |ui| {
                    ui.add_space(SP_5);
                    if btn_sm(
                        ui,
                        palette,
                        BtnTone::Primary,
                        "Approve plan",
                        Some(crate::icons::Icon::Check),
                    )
                    .clicked()
                    {
                        approve = true;
                    }
                    ui.add_space(SP_4);
                    if btn_sm(ui, palette, BtnTone::Plain, "Revise", None).clicked() {
                        revise_click = true;
                    }
                    ui.add_space(SP_4);
                    if btn_sm(ui, palette, BtnTone::Danger, "Reject and exit", None).clicked() {
                        reject = true;
                    }
                });
            });
            if revising {
                ui.add_space(SP_3);
                ui.horizontal(|ui| {
                    ui.add_space(SP_5);
                    let field = egui::TextEdit::singleline(&mut revise.text)
                        .hint_text("What should change?")
                        .desired_width((ui.available_width() - 2.0 * SP_5).max(40.0))
                        .font(egui::FontSelection::FontId(egui::FontId::new(
                            FS_SM,
                            family(FAMILY_BODY_REGULAR),
                        )));
                    let resp = ui.add(field);
                    resp.request_focus();
                    if resp.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        && !revise.text.trim().is_empty()
                    {
                        submit = true;
                    }
                });
            }
        });
        if revise_click {
            revise.open = if revise.open == Some(ix) {
                None
            } else {
                Some(ix)
            };
            revise.text.clear();
        }
        if submit {
            actions.push(RowAction::Review(
                PlanChoice::Revise,
                Some(revise.text.trim().to_owned()),
            ));
            revise.open = None;
            revise.text.clear();
        }
        if approve {
            actions.push(RowAction::Review(PlanChoice::Approve, None));
        }
        if reject {
            actions.push(RowAction::Review(PlanChoice::Reject, None));
        }
    }

    (response, actions)
}

/// What a card shows about one subagent.
#[derive(Debug, Clone, PartialEq)]
struct AgentFacts {
    id: String,
    name: String,
    task: String,
    phase: crate::ui::agents::Phase,
    reason: Option<String>,
    elapsed: Option<Duration>,
    tokens: Option<u64>,
    background: bool,
    last: Option<String>,
    result: Option<String>,
    stoppable: bool,
    item: Option<(String, u32)>,
}

/// The children the call `call_id` started, in launch order.
fn children_of<'a>(parent: &'a Session, call_id: &str) -> Vec<(&'a String, &'a Subagent)> {
    let mut children: Vec<_> = parent
        .agents
        .iter()
        .filter(|(_, agent)| agent.tool_call_id.as_deref() == Some(call_id))
        .collect();
    children.sort_by_key(|(_, agent)| agent.swarm.as_ref().map(|swarm| swarm.index));
    children
}

/// The first non-empty line of `text`, without inline code ticks.
fn first_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.replace('`', ""))
}

/// The facts of child `id` of `parent`.
fn agent_facts(
    store: &crate::store::Store,
    parent: &Session,
    id: &str,
    agent: &Subagent,
) -> AgentFacts {
    let child = store.state().session(id);
    let usage = agent.usage.as_ref();
    let phase = Phase::of(agent);
    let last = child
        .filter(|_| phase.live())
        .and_then(|child| {
            child.items.iter().rev().find_map(|item| match item {
                TranscriptItem::ToolCall(call) => Some(call),
                _ => None,
            })
        })
        .map(|call| {
            let (verb, target) = tool_verb(call);
            format!("{verb} {target}").trim().to_owned()
        });
    let watched = child.filter(|_| !phase.live()).and_then(|child| {
        child.items.iter().rev().find_map(|item| match item {
            TranscriptItem::Assistant { text } => first_line(text),
            TranscriptItem::Notice {
                tone: NoticeTone::Error,
                text,
            } => first_line(text),
            _ => None,
        })
    });
    let result = watched.or_else(|| {
        agent
            .tool_call_id
            .as_deref()
            .filter(|_| !phase.live())
            .and_then(|call| recorded_reply(parent, call, id))
    });
    AgentFacts {
        id: id.to_owned(),
        name: agent.name.clone().unwrap_or_else(|| "agent".to_owned()),
        task: agent.task.clone().unwrap_or_default(),
        phase,
        reason: agent.reason.clone(),
        elapsed: store
            .timings(&parent.id)
            .and_then(|times| times.agent(id))
            .or_else(|| {
                usage
                    .and_then(|usage| usage.run_ms)
                    .map(Duration::from_millis)
            }),
        tokens: usage
            .map(|usage| usage.input + usage.output)
            .or_else(|| child.map(|child| child.usage.used))
            .filter(|used| *used > 0),
        background: agent.background,
        last,
        result,
        stoppable: phase.live() && agent.capabilities.is_some_and(|caps| caps.cancel),
        item: agent
            .swarm
            .as_ref()
            .map(|swarm| (swarm.item.clone(), swarm.index)),
    }
}

/// The first line of the reply the parent recorded for child `id`.
fn recorded_reply(parent: &Session, call_id: &str, id: &str) -> Option<String> {
    let text = parent.items.iter().find_map(|item| match item {
        TranscriptItem::ToolCall(call) if call.tool_call_id == call_id => Some(call.text()),
        _ => None,
    })?;
    let at = text.find(&format!("session=\"{id}\""))?;
    let body = &text[at..];
    let body = &body[body.find('>')? + 1..];
    first_line(body.split("</agent>").next()?)
}

/// How a swarm's members stand, counted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SwarmCounts {
    running: usize,
    paused: usize,
    done: usize,
    failed: usize,
    cancelled: usize,
    total: usize,
}

impl SwarmCounts {
    fn of(members: &[AgentFacts]) -> Self {
        let mut counts = Self {
            total: members.len(),
            ..Self::default()
        };
        for member in members {
            match member.phase {
                Phase::Running => counts.running += 1,
                Phase::Paused => counts.paused += 1,
                Phase::Done => counts.done += 1,
                Phase::Failed => counts.failed += 1,
                Phase::Cancelled => counts.cancelled += 1,
            }
        }
        counts
    }

    fn live(self) -> bool {
        self.running + self.paused > 0
    }
}

/// A short token count: `840`, `2.3k`, `1.2M`.
fn tokens(n: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    match n {
        0..1_000 => n.to_string(),
        1_000..1_000_000 => format!("{:.1}k", n as f64 / 1_000.0),
        _ => format!("{:.1}M", n as f64 / 1_000_000.0),
    }
}

/// The design's string hash, so an avatar hue agrees with the web
/// client.
fn design_hash(text: &str) -> u32 {
    let mut x: i32 = 0;
    for unit in text.encode_utf16() {
        x = x.wrapping_mul(31).wrapping_add(i32::from(unit));
    }
    x.unsigned_abs()
}

/// The avatar: the name's first letter on the fill, in the name's hue.
fn avatar(ui: &mut egui::Ui, name: &str, palette: &Palette) {
    let hues = &palette.avatars;
    let hue = match design_hash(name) % 8 {
        0 => hues.amber,
        1 => hues.rose,
        2 => hues.orange,
        3 => hues.emerald,
        4 => hues.cyan,
        5 => hues.sky,
        6 => hues.violet,
        _ => hues.pink,
    };
    let letter = name
        .trim_start_matches("kage-")
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_default();
    egui::Frame::NONE
        .fill(palette.fill)
        .corner_radius(egui::CornerRadius::same(R_MD as u8))
        .show(ui, |ui| {
            ui.set_min_size(egui::vec2(26.0, 26.0));
            ui.centered_and_justified(|ui| {
                ui.label(
                    egui::RichText::new(letter)
                        .font(egui::FontId::new(12.0, family(FAMILY_MONO_BOLD)))
                        .color(hue.fg),
                );
            });
        });
}

/// The state label with its icon, in the state's color.
fn state_chip(ui: &mut egui::Ui, facts: &AgentFacts, palette: &Palette) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        icon_xs(ui, facts.phase.icon(), facts.phase.color(palette));
        ui.label(
            egui::RichText::new(facts.phase.label())
                .size(FS_XS)
                .color(facts.phase.color(palette)),
        );
    });
}

/// `4s` and `2.3k tok` joined by a middle dot, from what was measured.
fn meta_line(facts: &AgentFacts) -> String {
    [
        facts.elapsed.map(clock::span),
        facts.tokens.map(|n| format!("{} tok", tokens(n))),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" \u{b7} ")
}

/// The card an `agent` or `swarm` call renders as, once the engine
/// announced the children it started; until then the call renders as
/// its plain row. `None` asks the caller to render the plain tool row.
pub fn render_agents(
    ui: &mut egui::Ui,
    store: &crate::store::Store,
    session: &Session,
    ix: usize,
    call: &ToolCallItem,
    origin: web_time::Instant,
    ui_state: &mut UiState,
) -> Option<(egui::Response, Vec<RowAction>)> {
    let members: Vec<AgentFacts> = children_of(session, &call.tool_call_id)
        .into_iter()
        .map(|(id, agent)| agent_facts(store, session, id, agent))
        .collect();
    let palette = Palette::active_of(ui.ctx());
    let mut actions = Vec::new();
    if call.title == "agent" {
        let facts = members.first()?;
        let response = agent_card(ui, ix, facts, palette, &mut actions);
        return Some((response, actions));
    }
    if members.is_empty() {
        return None;
    }
    let counts = SwarmCounts::of(&members);
    let live = counts.live();
    let open = live != ui_state.expanded.contains(&RowKey::Item(ix));
    let took = store
        .timings(&session.id)
        .and_then(|times| times.tool(&call.tool_call_id));
    let template = call
        .swarm
        .as_ref()
        .and_then(|meta| meta.template.clone())
        .or_else(|| {
            let typed = input_str(call.input.as_ref(), "prompt_template").to_owned();
            (!typed.is_empty()).then_some(typed)
        });
    let response = swarm_card(
        ui,
        ix,
        call,
        &members,
        counts,
        open,
        store.constellation().then_some(origin),
        ui_state.templates.contains(&ix),
        template.as_deref(),
        took,
        palette,
        ui_state,
        &mut actions,
    );
    Some((response, actions))
}

/// The card an `agent` call renders as: avatar, name and kind, the
/// task or the latest tool line, the state with elapsed time and
/// tokens, a stop button while it runs, and the result once it ended.
fn agent_card(
    ui: &mut egui::Ui,
    ix: usize,
    facts: &AgentFacts,
    palette: &Palette,
    actions: &mut Vec<RowAction>,
) -> egui::Response {
    let sub = match (&facts.last, facts.phase.live()) {
        (Some(last), true) => last.clone(),
        _ => facts.task.clone(),
    };
    let mut open = false;
    let mut stop = false;
    let response = egui::Frame::NONE
        .fill(palette.surface)
        .stroke(egui::Stroke::new(1.0, palette.line))
        .corner_radius(egui::CornerRadius::same(R_LG as u8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let head = clickable_row(ui, palette.hover, 0.0.into(), |ui| {
                ui.spacing_mut().item_spacing.x = SP_4;
                ui.add_space(SP_5);
                ui.set_min_height(44.0);
                avatar(ui, &facts.name, palette);
                ui.add_space(SP_4);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = SP_4;
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&facts.name)
                                    .size(FS_SM)
                                    .color(palette.ink_strong),
                            )
                            .selectable(false)
                            .truncate(),
                        );
                        let (bg, fg, text) = if facts.background {
                            (palette.accent_soft, palette.accent, "background")
                        } else {
                            (palette.fill, palette.muted, facts.name.as_str())
                        };
                        pill(ui, text, fg, bg, bg);
                    });
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&sub).size(FS_XS).color(palette.faint),
                        )
                        .truncate(),
                    );
                });
                crate::ui::transcript::tool_rows::trailing(ui, |ui| {
                    if facts.stoppable {
                        ui.add_space(SP_4);
                        if icon_button(ui, crate::icons::Icon::Square, "Stop agent", palette)
                            .clicked()
                        {
                            stop = true;
                        }
                    } else {
                        ui.add_space(SP_4);
                        icon(ui, crate::icons::Icon::ChevronRight, palette.faint);
                    }
                    ui.add_space(SP_4);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = SP_1;
                        state_chip(ui, facts, palette);
                        ui.label(
                            egui::RichText::new(meta_line(facts))
                                .font(mono(11.0))
                                .color(palette.faint),
                        );
                    });
                });
            });
            if head.clicked() {
                open = true;
            }
            if let Some(result) = facts.result.as_ref().filter(|_| !facts.phase.live()) {
                hairline(ui, palette);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = SP_4;
                    ui.add_space(SP_5);
                    ui.set_min_height(20.0);
                    let glyph = if facts.phase == Phase::Done {
                        crate::icons::Icon::CircleCheck
                    } else {
                        crate::icons::Icon::CircleX
                    };
                    icon_xs(ui, glyph, facts.phase.color(palette));
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(result).size(FS_XS).color(palette.muted),
                        )
                        .truncate(),
                    );
                });
            }
        })
        .response;
    if open {
        actions.push(RowAction::OpenAgent(facts.id.clone()));
    }
    if stop {
        actions.push(RowAction::CancelAgent(facts.id.clone()));
    }
    let _ = ix;
    response
}

/// The card a `swarm` call renders as: the batch head with live counts,
/// the progress bar, and while open the member rows and the footer with
/// the prompt template, the summary and Retry for failed members.
#[allow(clippy::too_many_arguments)]
fn swarm_card(
    ui: &mut egui::Ui,
    ix: usize,
    call: &ToolCallItem,
    members: &[AgentFacts],
    counts: SwarmCounts,
    open: bool,
    field: Option<web_time::Instant>,
    template_open: bool,
    template: Option<&str>,
    took: Option<Duration>,
    palette: &Palette,
    ui_state: &mut UiState,
    actions: &mut Vec<RowAction>,
) -> egui::Response {
    let input = call.input.as_ref();
    let text = |key: &str| {
        input
            .and_then(|input| input.get(key))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let description = Some(text("description"))
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "Swarm".to_owned());
    let agent = text("agent");
    let sub = if agent.is_empty() {
        format!("{} agents", counts.total)
    } else {
        format!("{agent} \u{b7} {} agents", counts.total)
    };
    let ended = counts.done + counts.failed + counts.cancelled;

    let mut toggle_click = false;
    let mut open_member: Option<String> = None;
    let response = egui::Frame::NONE
        .fill(palette.surface)
        .stroke(egui::Stroke::new(1.0, palette.done_bd))
        .corner_radius(egui::CornerRadius::same(R_LG as u8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let head = clickable_row(ui, palette.hover, 0.0.into(), |ui| {
                ui.spacing_mut().item_spacing.x = SP_4;
                ui.add_space(SP_5);
                ui.set_min_height(44.0);
                egui::Frame::NONE
                    .fill(palette.done_soft)
                    .corner_radius(egui::CornerRadius::same(R_MD as u8))
                    .show(ui, |ui| {
                        ui.set_min_size(egui::vec2(28.0, 28.0));
                        ui.centered_and_justified(|ui| {
                            icon(ui, crate::icons::Icon::Waypoints, palette.done);
                        });
                    });
                ui.add_space(SP_4);
                ui.vertical(|ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&description)
                                .size(FS_SM)
                                .color(palette.ink_strong),
                        )
                        .selectable(false)
                        .truncate(),
                    );
                    ui.label(egui::RichText::new(&sub).size(FS_XS).color(palette.faint));
                });
                crate::ui::transcript::tool_rows::trailing(ui, |ui| {
                    icon(
                        ui,
                        if open {
                            crate::icons::Icon::ChevronUp
                        } else {
                            crate::icons::Icon::ChevronDown
                        },
                        palette.faint,
                    );
                    ui.add_space(SP_4);
                    ui.label(
                        egui::RichText::new(format!("{ended}"))
                            .font(mono(FS_SM))
                            .color(palette.ink_strong),
                    );
                    ui.label(
                        egui::RichText::new(format!(" / {}", counts.total))
                            .font(mono(FS_SM))
                            .color(palette.faint),
                    );
                    if counts.live() {
                        ui.add_space(SP_4);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 5.0;
                            icon_xs(ui, crate::icons::Icon::LoaderCircle, palette.accent);
                            let mut live = format!("{} running", counts.running);
                            if counts.paused > 0 {
                                live.push_str(&format!(", {} limited", counts.paused));
                            }
                            ui.label(egui::RichText::new(live).size(FS_XS).color(palette.accent));
                        });
                    }
                });
            });
            if head.clicked() {
                toggle_click = true;
            }
            // The constellation while the preference asks for it, else the
            // progress bar: done, failed and live shares of the batch.
            if let Some(origin) = field {
                let stars: Vec<crate::ui::constellation::Star> = members
                    .iter()
                    .map(|member| crate::ui::constellation::Star {
                        id: member.id.clone(),
                        item: member
                            .item
                            .clone()
                            .map_or_else(|| member.name.clone(), |(item, _)| item),
                        phase: member.phase,
                    })
                    .collect();
                let (asked, animate) = crate::ui::constellation::show(ui, &stars, None, origin);
                for event in asked {
                    open_member = match event {
                        crate::ui::constellation::ConstellationEvent::Open(id) => Some(id),
                    };
                }
                if animate {
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(50));
                }
            } else {
                let (track, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 3.0),
                    egui::Sense::hover(),
                );
                let share = |n: usize| {
                    if counts.total == 0 {
                        0.0
                    } else {
                        n as f32 / counts.total as f32
                    }
                };
                let mut x = track.left();
                for (n, color) in [
                    (counts.done, palette.ok),
                    (counts.failed, palette.danger),
                    (counts.running + counts.paused, palette.done),
                ] {
                    let w = track.width() * share(n);
                    if w > 0.0 {
                        ui.painter().rect_filled(
                            egui::Rect::from_min_size(
                                egui::pos2(x, track.top()),
                                egui::vec2(w, 3.0),
                            ),
                            0.0,
                            color,
                        );
                        x += w;
                    }
                }
            }
            if open {
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        for (n, member) in members.iter().enumerate() {
                            if n > 0 {
                                hairline(ui, palette);
                            }
                            let id = member.id.clone();
                            let clicked = clickable_row(ui, palette.hover, 0.0.into(), |ui| {
                                ui.spacing_mut().item_spacing.x = SP_4;
                                ui.add_space(SP_5);
                                ui.set_min_height(36.0);
                                let (item, index) = member.item.clone().unwrap_or_else(|| {
                                    (member.name.clone(), u32::try_from(n).unwrap_or(0))
                                });
                                ui.add_sized(
                                    egui::vec2(18.0, 16.0),
                                    egui::Label::new(
                                        egui::RichText::new(format!("{:02}", index + 1))
                                            .font(mono(11.0))
                                            .color(palette.faint),
                                    ),
                                );
                                ui.add(
                                    egui::Label::new(egui::RichText::new(&item).color(palette.ink))
                                        .truncate(),
                                );
                                let line = if member.phase.live() {
                                    member
                                        .reason
                                        .clone()
                                        .or_else(|| member.last.clone())
                                        .unwrap_or_else(|| "starting".to_owned())
                                } else {
                                    member.result.clone().unwrap_or_default()
                                };
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&line)
                                            .font(mono(11.0))
                                            .color(palette.faint),
                                    )
                                    .truncate(),
                                );
                                crate::ui::transcript::tool_rows::trailing(ui, |ui| {
                                    ui.add_sized(
                                        egui::vec2(44.0, 16.0),
                                        egui::Label::new(
                                            egui::RichText::new(
                                                member.tokens.map(tokens).unwrap_or_default(),
                                            )
                                            .font(mono(11.0))
                                            .color(palette.faint),
                                        ),
                                    );
                                    state_chip(ui, member, palette);
                                });
                            });
                            if clicked.clicked() {
                                open_member = Some(id);
                            }
                        }
                    });
            }
        })
        .response;
    if toggle_click {
        toggle(ui_state, RowKey::Item(ix));
    }
    if let Some(id) = open_member {
        actions.push(RowAction::OpenAgent(id));
    }

    // The footer: the template while asked for, the summary, and the
    // retry and agents buttons.
    let mut template_click = false;
    let mut agents_click = false;
    let retry: Vec<String> = members
        .iter()
        .filter(|member| matches!(member.phase, Phase::Failed | Phase::Cancelled))
        .map(|member| member.id.clone())
        .collect();
    egui::Frame::NONE
        .fill(palette.surface)
        .stroke(egui::Stroke::new(1.0, palette.done_bd))
        .corner_radius(egui::CornerRadius::same(R_LG as u8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            if template_open && let Some(template) = template {
                ui.horizontal_wrapped(|ui| {
                    ui.add_space(SP_5);
                    ui.add_space(SP_4);
                    for (n, part) in template.split("{{item}}").enumerate() {
                        if n > 0 {
                            ui.label(
                                egui::RichText::new("{{item}}")
                                    .font(mono(FS_XS))
                                    .color(palette.done),
                            );
                        }
                        ui.label(
                            egui::RichText::new(part)
                                .font(mono(FS_XS))
                                .color(palette.muted),
                        );
                    }
                });
            }
            ui.horizontal(|ui| {
                ui.set_min_width(ui.available_width());
                ui.spacing_mut().item_spacing.x = SP_4;
                ui.add_space(SP_5);
                ui.set_min_height(20.0);
                ui.add_space(SP_3);
                if template.is_some()
                    && btn_sm(
                        ui,
                        palette,
                        BtnTone::Plain,
                        if template_open {
                            "Hide template"
                        } else {
                            "Prompt template"
                        },
                        Some(crate::icons::Icon::File),
                    )
                    .clicked()
                {
                    template_click = true;
                }
                crate::ui::transcript::tool_rows::trailing(ui, |ui| {
                    ui.add_space(SP_5);
                    if btn_sm(
                        ui,
                        palette,
                        BtnTone::Plain,
                        "Agents",
                        Some(crate::icons::Icon::Users),
                    )
                    .clicked()
                    {
                        agents_click = true;
                    }
                    if !counts.live() && !retry.is_empty() {
                        ui.add_space(SP_4);
                        if btn_sm(
                            ui,
                            palette,
                            BtnTone::Plain,
                            &format!("Retry {}", retry.len()),
                            Some(crate::icons::Icon::RefreshCw),
                        )
                        .clicked()
                        {
                            actions.push(RowAction::ResumeSwarm(retry.clone()));
                        }
                    }
                    let mut summary = format!("{} finished", counts.done);
                    if counts.failed > 0 {
                        summary.push_str(&format!(", {} failed", counts.failed));
                    }
                    if counts.cancelled > 0 {
                        summary.push_str(&format!(", {} aborted", counts.cancelled));
                    }
                    if let Some(took) = took {
                        summary.push_str(&format!(" \u{b7} {}", clock::span(took)));
                    }
                    ui.add_space(SP_4);
                    ui.label(
                        egui::RichText::new(summary)
                            .size(FS_XS)
                            .color(palette.muted),
                    );
                });
            });
        });
    if template_click && !ui_state.templates.remove(&ix) {
        ui_state.templates.insert(ix);
    }
    if agents_click {
        actions.push(RowAction::OpenAgents);
    }
    response
}

#[cfg(test)]
mod tests {
    use kage_client::wire::{
        ContentBlock, MessageChunk, ToolCallContent, ToolCallStatus, ToolKind,
    };
    use kage_client::{Session, ToolCallItem, TranscriptItem};

    use super::{
        ChipTone, RowAction, TodoState, chip_tone, design_hash, first_line, recorded_reply,
        run_texts, search_results, strip_ansi, todos_of_call, todos_of_plan, tokens,
    };
    use crate::theme::Palette;

    /// A completed tool call with the given fields.
    fn call(
        id: &str,
        title: &str,
        kind: ToolKind,
        status: ToolCallStatus,
        content: Vec<ToolCallContent>,
        raw_output: Option<serde_json::Value>,
        input: Option<serde_json::Value>,
    ) -> ToolCallItem {
        ToolCallItem {
            tool_call_id: id.into(),
            title: title.into(),
            kind,
            status,
            input,
            swarm: None,
            content,
            raw_output,
            took_ms: None,
        }
    }

    /// Text content holding one chunk.
    fn text_chunk(text: &str) -> ToolCallContent {
        ToolCallContent::Content(MessageChunk {
            content: ContentBlock::text(text),
            meta: None,
        })
    }

    #[test]
    fn strip_ansi_drops_csi_and_osc_escapes_only() {
        assert_eq!(strip_ansi("plain"), "plain");
        assert_eq!(strip_ansi("\u{1b}[31mred\u{1b}[0m"), "red");
        assert_eq!(strip_ansi("\u{1b}]0;title\u{7}body"), "body");
        assert_eq!(
            strip_ansi("a\u{1b}b"),
            "a",
            "a lone escape eats its next byte"
        );
    }

    #[test]
    fn todos_of_call_reads_title_and_status_and_refuses_other_tools() {
        let todo_list = call(
            "t",
            "todo_list",
            ToolKind::Other,
            ToolCallStatus::Completed,
            Vec::new(),
            None,
            Some(serde_json::json!({"todos": [
                {"title": "read", "status": "done"},
                {"title": "write", "status": "in_progress"},
                {"title": "test", "status": "failed"},
                {"title": "ship", "status": "pending"},
            ]})),
        );
        let todos = todos_of_call(&todo_list).expect("the todos parse");
        let states: Vec<TodoState> = todos.iter().map(|todo| todo.state).collect();
        assert_eq!(todos[0].text, "read");
        assert_eq!(
            states,
            [
                TodoState::Done,
                TodoState::Running,
                TodoState::Failed,
                TodoState::Pending
            ]
        );
        let other = call(
            "r",
            "read",
            ToolKind::Read,
            ToolCallStatus::Completed,
            Vec::new(),
            None,
            None,
        );
        assert!(todos_of_call(&other).is_none());
    }

    #[test]
    fn todos_of_call_falls_back_to_content_when_title_is_absent() {
        let todo_list = call(
            "t",
            "todo_list",
            ToolKind::Other,
            ToolCallStatus::Completed,
            Vec::new(),
            None,
            Some(
                serde_json::json!({"todos": [{"content": "from content", "status": "completed"}]}),
            ),
        );
        let todos = todos_of_call(&todo_list).expect("the todos parse");
        assert_eq!(todos[0].text, "from content");
        assert_eq!(todos[0].state, TodoState::Done);
    }

    #[test]
    fn todos_of_plan_reads_content_and_status_per_entry() {
        let entries = vec![
            serde_json::json!({"content": "one", "status": "completed"}),
            serde_json::json!({"content": "two", "status": "in_progress"}),
            serde_json::json!({"content": "three"}),
        ];
        let todos = todos_of_plan(&entries);
        let states: Vec<TodoState> = todos.iter().map(|todo| todo.state).collect();
        assert_eq!(todos[1].text, "two");
        assert_eq!(
            states,
            [TodoState::Done, TodoState::Running, TodoState::Pending]
        );
    }

    #[test]
    fn run_texts_split_the_prompt_and_the_reply_of_a_run() {
        let session = Session {
            items: vec![
                TranscriptItem::User {
                    content: vec![ContentBlock::text("fix the bug")],
                    steered: false,
                },
                TranscriptItem::Assistant {
                    text: "looking".into(),
                },
                TranscriptItem::Assistant {
                    text: "fixed".into(),
                },
                TranscriptItem::TurnEnd {
                    reason: None,
                    at: None,
                    took_ms: None,
                },
            ],
            ..Session::new("s1")
        };
        let (prompt, reply) = run_texts(&session, 3);
        assert_eq!(prompt.as_deref(), Some("fix the bug"));
        assert_eq!(reply, "looking\n\nfixed");
    }

    #[test]
    fn run_texts_hide_a_prompt_the_engine_built_from_agent_text() {
        let session = Session {
            items: vec![
                TranscriptItem::User {
                    content: vec![ContentBlock::text(
                        "<agent name=\"general\" session=\"c1\" state=\"completed\">\nreport body\n</agent>",
                    )],
                    steered: false,
                },
                TranscriptItem::Assistant {
                    text: "done".into(),
                },
                TranscriptItem::TurnEnd {
                    reason: None,
                    at: None,
                    took_ms: None,
                },
            ],
            ..Session::new("s1")
        };
        let (prompt, reply) = run_texts(&session, 2);
        assert!(prompt.is_none(), "an engine-built prompt cannot be retried");
        assert_eq!(reply, "done");
    }

    #[test]
    fn search_results_read_the_delivered_results_of_a_web_search() {
        let search = call(
            "w",
            "web_search",
            ToolKind::Other,
            ToolCallStatus::Completed,
            Vec::new(),
            Some(serde_json::json!({"results": [
                {"title": "A", "url": "https://a", "snippet": "first"},
            ]})),
            None,
        );
        let results = search_results(&search).expect("results parse");
        assert_eq!(results[0].title, "A");
        assert_eq!(results[0].url, "https://a");
        assert_eq!(results[0].snippet, "first");
        let other = call(
            "r",
            "read",
            ToolKind::Read,
            ToolCallStatus::Completed,
            Vec::new(),
            None,
            None,
        );
        assert!(search_results(&other).is_none());
    }

    #[test]
    fn token_counts_shorten_like_the_design() {
        assert_eq!(tokens(840), "840");
        assert_eq!(tokens(2_300), "2.3k");
        assert_eq!(tokens(1_200_000), "1.2M");
    }

    #[test]
    fn the_hash_matches_the_design_so_hues_agree() {
        assert_eq!(design_hash("explore"), 1_309_148_525);
        assert_eq!(design_hash(""), 0);
    }

    #[test]
    fn first_line_trims_and_drops_inline_code_ticks() {
        assert_eq!(first_line("\n  `answer`\nmore").as_deref(), Some("answer"));
        assert_eq!(first_line("  \n  "), None);
    }

    #[test]
    fn recorded_reply_reads_the_child_report_a_call_recorded() {
        let output = "<agent name=\"general\" session=\"c1\" state=\"completed\">\nfirst `answer`\nmore\n</agent>";
        let parent = Session {
            items: vec![TranscriptItem::ToolCall(call(
                "call_s",
                "swarm",
                ToolKind::Other,
                ToolCallStatus::Completed,
                vec![text_chunk(output)],
                None,
                None,
            ))],
            ..Session::new("p")
        };
        assert_eq!(
            recorded_reply(&parent, "call_s", "c1").as_deref(),
            Some("first answer")
        );
        assert_eq!(recorded_reply(&parent, "call_s", "c3"), None);
        assert_eq!(recorded_reply(&parent, "other", "c1"), None);
    }

    #[test]
    fn chip_tones_map_to_the_design_fills() {
        let palette = Palette::shadow();
        let (good_fg, good_bg) = chip_tone(ChipTone::Good, &palette);
        assert_eq!(good_fg, palette.diff_add);
        assert_eq!(good_bg, palette.diff_add_bg);
        let (bad_fg, bad_bg) = chip_tone(ChipTone::Bad, &palette);
        assert_eq!(bad_fg, palette.diff_del);
        assert_eq!(bad_bg, palette.diff_del_bg);
    }

    #[test]
    fn row_actions_carry_their_payloads() {
        assert_eq!(
            RowAction::OpenChange("c1".to_owned()),
            RowAction::OpenChange("c1".to_owned())
        );
        assert_ne!(
            RowAction::OpenChange("c1".to_owned()),
            RowAction::OpenFile("c1".to_owned())
        );
        assert_eq!(
            RowAction::ResumeSwarm(vec!["a".to_owned()]),
            RowAction::ResumeSwarm(vec!["a".to_owned()])
        );
    }
}
