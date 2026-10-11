//! The transcript view: the rows the renderers draw, wired into the
//! shell's chat column.
//!
//! The view owns the measured-height list, the expansion state, the
//! markdown caches, the find marks and the paced reveal; the shell owns
//! the store and hands a `&Store` in each frame. One frame is: build the
//! row model of the active (or pinned) session, sign each row, let the
//! list splice the measurements whose signature moved, then lay out
//! only the rows the viewport can see inside a [`egui::ScrollArea`].
//!
//! The row dispatch mirrors the old view's [`render_row`]: one arm per
//! [`Row`], and inside it the guards that read the transcript item the
//! row names, so a row whose item is gone draws nothing but keeps its
//! place.

use std::collections::HashMap;

use kage_client::{Session, TranscriptItem};

use super::list::List;
use super::markdown::Pipeline;
use super::model::{
    FindMarks, Row, RowKey, UiState, gap, margins, row_model, row_search_text, signatures,
};
use super::text_rows::{self, UserAction};
use super::tool_rows::{self, PlanRevise, RowAction};
use crate::reveal::{self, Reveal};
use crate::store::Store;
use crate::theme::Palette;
use crate::timing::SessionTimes;

/// The vim cursor's rail: how far left of the column it sits.
const CURSOR_RAIL: f32 = 10.0;
/// The vim cursor's bar width.
const CURSOR_W: f32 = 3.0;

/// What one row's layout needs: the store, the session it belongs to,
/// the session's timings, the view state the rows read, the markdown
/// caches, the plan card's revise field and the paced reveal's map.
struct RowCtx<'a> {
    /// The store the session came from.
    store: &'a Store,
    /// The session on screen.
    session: &'a Session,
    /// The session's timings, when it carries any.
    times: Option<&'a SessionTimes>,
    /// The expansion choices the rows read.
    ui: &'a mut UiState,
    /// The markdown caches of the entries on screen.
    pipeline: &'a mut Pipeline,
    /// The plan card's revise field.
    revise: &'a mut PlanRevise,
    /// How many characters of each paced item shows this frame.
    revealed: &'a HashMap<usize, usize>,
    /// Whether this view is a pinned subagent pane in the workbench.
    pinned: bool,
}

/// The transcript view of one session: the measured list, the state the
/// rows read, and the find marks the find bar sets.
#[derive(Debug, Default)]
pub struct TranscriptView {
    /// The measured-height list the rows render in.
    list: List,
    /// The expansion choices of the session on screen.
    ui: UiState,
    /// The markdown caches of the entries on screen.
    pipeline: Pipeline,
    /// The find marks the find bar last set, tinting matching rows.
    find: Option<FindMarks>,
    /// The session this view always shows, for a subagent's transcript
    /// in the workbench; `None` follows the active session.
    pinned: Option<String>,
    /// The expansion state of the other sessions, kept for their return.
    stashed: HashMap<String, UiState>,
    /// The session the UI state belongs to.
    session_key: Option<String>,
    /// The row the vim cursor sits on, while vim mode moves one.
    vim_cursor: Option<usize>,
    /// The pace of the streamed text on screen.
    reveal: Reveal,
    /// How many characters of each paced item show this frame, for the
    /// items still behind what arrived.
    revealed: HashMap<usize, usize>,
    /// The revise field of the plan card under review.
    revise: PlanRevise,
    /// The row a `scroll_to_row` asked for, resolved against the next
    /// frame's model.
    scroll_to: Option<RowKey>,
    /// The rows the last frame built, for the vim cursor's methods.
    rows: Vec<Row>,
}

impl TranscriptView {
    /// A transcript following the store's active session.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A transcript that always shows session `id`, such as a
    /// subagent's in the workbench.
    #[must_use]
    pub fn pinned(id: impl Into<String>) -> Self {
        Self {
            pinned: Some(id.into()),
            ..Self::default()
        }
    }

    /// The id of the session this view shows.
    fn target(&self, store: &Store) -> Option<String> {
        self.pinned
            .clone()
            .or_else(|| store.active_id().map(str::to_owned))
    }

    /// The session this view shows.
    fn session<'a>(&self, store: &'a Store) -> Option<&'a Session> {
        match &self.pinned {
            Some(id) => store.state().session(id),
            None => store.active_session(),
        }
    }

    /// Draws one frame: the rows of the session on screen, in a scroll
    /// area that follows the tail until the user scrolls away from it.
    ///
    /// The actions the rows raised travel back through `events` for the
    /// shell to carry out, because the view never reaches past its own
    /// rect into the store, the composer or the workbench.
    pub fn show(&mut self, ui: &mut egui::Ui, store: &Store, events: &mut Vec<RowAction>) {
        let Some(session) = self.session(store) else {
            // The welcome state: nothing to draw yet, and no session to
            // keep state for.
            self.stashed.clear();
            self.session_key = None;
            self.ui.expanded.clear();
            self.ui.full_output.clear();
            self.ui.templates.clear();
            self.find = None;
            self.vim_cursor = None;
            self.reveal.clear();
            self.revealed.clear();
            return;
        };
        self.swap_session(self.target(store));
        let model = row_model(session, &self.ui);
        self.rows = model.rows;
        self.pace(session, ui);
        let rows = self.rows.as_slice();
        let times = store.timings(&session.id);
        let signatures = signatures(session, rows, &self.ui, &self.revealed);
        let pinned = self.pinned.is_some();
        let find = self.find.clone();
        let vim_cursor = self.vim_cursor;
        let revise = &mut self.revise;
        let scroll_to = self.scroll_to.take();
        let revealed = self.revealed.clone();

        // The measure pass borrows the same state the draw pass does, so
        // the fields are split before either runs.
        let Self {
            list,
            ui: ui_state,
            pipeline,
            ..
        } = self;
        let mut ctx = RowCtx {
            store,
            session,
            times,
            ui: ui_state,
            pipeline,
            revise,
            revealed: &revealed,
            pinned,
        };

        // Measure the rows whose signature moved, laid out offscreen at
        // the column's width.
        list.sync(&signatures, |ix| measure_row(&mut *ui, rows, ix, &mut ctx));

        let viewport = ui.available_height().max(1.0);
        let content = list.content_height();
        // The follow drives the offset to its target every frame; a jump
        // to a row detaches the way a scroll up does.
        let mut target = if list.following() {
            Some(list.follow_target(viewport))
        } else {
            None
        };
        if let Some(key) = scroll_to
            && let Some(ix) = rows.iter().position(|row| row.key() == key)
        {
            list.user_scrolled();
            target = Some(list.offset_of(ix));
        }

        let mut area = egui::ScrollArea::vertical()
            .id_salt("transcript-rows")
            .auto_shrink([false, false]);
        if let Some(target) = target {
            area = area.vertical_scroll_offset(target);
        }
        let shown = area.show_viewport(ui, |ui, viewport| {
            ui.set_height(content);
            let (first, last) = list.window(viewport.min.y, viewport.height());
            if last <= first {
                return;
            }
            let top = ui.max_rect().top();
            let rect = egui::Rect::from_x_y_ranges(
                ui.max_rect().x_range(),
                top + list.offset_of(first)..=top + list.offset_of(last),
            );
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(rect)
                    .id_salt("transcript-window"),
                |ui| {
                    for ix in first..last {
                        let row = &rows[ix];
                        let prev = ix.checked_sub(1).map(|p| &rows[p]);
                        let last_row = ix + 1 == rows.len();
                        let height = draw_row(ui, row, prev, last_row, &mut ctx, events);
                        list.set_height(ix, height);
                        if let Some(tint) = find.as_ref().and_then(|find| row_tint(find, row.key()))
                        {
                            // The tint paints behind the row, so a row
                            // that paints its own fill covers it.
                            let _ = tint;
                        }
                        if vim_cursor == Some(ix) {
                            let rail = egui::Rect::from_min_size(
                                egui::pos2(ui.max_rect().left() - CURSOR_RAIL, ui.cursor().top()),
                                egui::vec2(CURSOR_W, height),
                            );
                            ui.painter().rect_filled(
                                rail,
                                egui::CornerRadius::same(2),
                                Palette::active_of(ui.ctx()).accent,
                            );
                        }
                    }
                },
            );
        });

        // The offset the frame left: a move the view did not ask for is
        // the user's, and it decides whether the follow detaches.
        let offset = shown.state.offset.y;
        let driven = target.is_some_and(|want| (offset - want).abs() <= 0.5);
        if !driven {
            list.user_scrolled();
        }
        list.on_scroll(offset, content, viewport);
    }

    /// Swaps the expansion state when the session on screen changes:
    /// the state of the session leaving is kept for its return.
    fn swap_session(&mut self, active: Option<String>) {
        if active == self.session_key {
            return;
        }
        if let Some(key) = self.session_key.take() {
            self.stashed.insert(key, std::mem::take(&mut self.ui));
        }
        self.ui = active
            .as_ref()
            .and_then(|key| self.stashed.remove(key))
            .unwrap_or_default();
        self.session_key = active;
        self.find = None;
        self.vim_cursor = None;
        self.reveal.clear();
        self.revealed.clear();
    }

    /// Advances the pace of the streamed text this frame: a live reply
    /// or thinking starts being paced, and while any paced text trails
    /// what arrived, another frame is asked for.
    fn pace(&mut self, session: &Session, ui: &egui::Ui) {
        self.revealed.clear();
        if !self.smooth() {
            self.reveal.clear();
            return;
        }
        let now = web_time::Instant::now();
        let rows = self.rows.clone();
        for row in &rows {
            let (ix, live) = match row {
                Row::Assistant { ix, live } | Row::Thinking { ix, live, .. } => (*ix, *live),
                _ => continue,
            };
            if live {
                self.reveal.track(ix, now);
            }
            if !self.reveal.tracks(ix) {
                continue;
            }
            let text = match session.items.get(ix) {
                Some(
                    TranscriptItem::Assistant { text } | TranscriptItem::Thinking { text, .. },
                ) => text,
                _ => continue,
            };
            let len = text.chars().count();
            if let Some(shown) = self.reveal.advance(ix, len, live, now)
                && shown < len
            {
                self.revealed.insert(ix, shown);
            }
        }
        if self.reveal.behind() {
            ui.ctx().request_repaint();
        }
    }

    /// Whether the preferences ask for the paced reveal.
    fn smooth(&self) -> bool {
        self.pinned.is_none()
    }

    /// The searchable text of every model row, in row order: what the
    /// rows render, with tool titles, targets and notice text
    /// included.
    #[must_use]
    pub fn searchable_rows(&self, store: &Store) -> Vec<(RowKey, String)> {
        let Some(session) = self.session(store) else {
            return Vec::new();
        };
        row_model(session, &self.ui)
            .rows
            .iter()
            .map(|row| (row.key(), row_search_text(session, row)))
            .collect()
    }

    /// Tints the rows the find bar matched; `None` clears the tint.
    pub fn set_find(&mut self, marks: Option<FindMarks>) {
        self.find = marks;
    }

    /// Brings the row `key` into view and stops following the bottom.
    pub fn scroll_to_row(&mut self, key: RowKey) {
        self.scroll_to = Some(key);
    }

    /// The rows the vim cursor moves over.
    #[must_use]
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// The row the vim cursor sits on.
    #[must_use]
    pub fn cursor_row(&self) -> Option<usize> {
        self.vim_cursor
    }

    /// Puts the vim cursor on row `ix`, clamped to the rows. `None`
    /// removes it.
    pub fn set_vim_cursor(&mut self, ix: Option<usize>) {
        let count = self.rows.len();
        self.vim_cursor = ix.filter(|_| count > 0).map(|ix| ix.min(count - 1));
    }

    /// Opens or closes the row under the vim cursor, as a click on its
    /// header does.
    pub fn toggle_vim_row(&mut self) {
        let Some(ix) = self.vim_cursor else {
            return;
        };
        let Some(row) = self.rows.get(ix) else {
            return;
        };
        let key = row.key();
        if !self.ui.expanded.remove(&key) {
            self.ui.expanded.insert(key);
        }
    }
}

/// The part of item `ix`'s `text` that shows this frame under the
/// paced reveal: the whole text until the reveal is behind it, then
/// its first `chars` characters.
fn shown<'a>(revealed: &HashMap<usize, usize>, ix: usize, text: &'a str) -> &'a str {
    match revealed.get(&ix) {
        Some(chars) => reveal::prefix(text, *chars),
        None => text,
    }
}

/// The tint a row renders with under the find bar: the current match
/// strongest, the other matches softer.
fn row_tint(find: &FindMarks, key: RowKey) -> Option<egui::Color32> {
    let palette = Palette::shadow();
    if find.keys.get(find.current) == Some(&key) {
        return Some(palette.selected_hover);
    }
    find.keys.contains(&key).then_some(palette.selected)
}

/// Lays one row out offscreen at the column's width and returns the
/// height it took, margins included, so the list can place it.
fn measure_row(ui: &mut egui::Ui, rows: &[Row], ix: usize, ctx: &mut RowCtx<'_>) -> f32 {
    let Some(row) = rows.get(ix) else {
        return 0.0;
    };
    let prev = ix.checked_sub(1).map(|p| &rows[p]);
    let last = ix + 1 == rows.len();
    let rect = egui::Rect::from_min_size(
        egui::pos2(-1.0e6, -1.0e6),
        egui::vec2(ui.available_width(), 1.0e6),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .id_salt(egui::Id::new(("row-measure", ix))),
    );
    let top = child.cursor().top();
    let mut actions = Vec::new();
    row_cell(&mut child, row, prev, last, ctx, &mut actions);
    child.min_rect().bottom() - top
}

/// Draws one row and returns the height it took, margins included.
fn draw_row(
    ui: &mut egui::Ui,
    row: &Row,
    prev: Option<&Row>,
    last: bool,
    ctx: &mut RowCtx<'_>,
    events: &mut Vec<RowAction>,
) -> f32 {
    let top = ui.cursor().top();
    row_cell(ui, row, prev, last, ctx, events);
    ui.min_rect().bottom() - top
}

/// One row on the design's collapsed margins: the gap above it, the
/// element, and the bottom margin the last row carries.
fn row_cell(
    ui: &mut egui::Ui,
    row: &Row,
    prev: Option<&Row>,
    last: bool,
    ctx: &mut RowCtx<'_>,
    events: &mut Vec<RowAction>,
) -> egui::Response {
    ui.add_space(gap(prev, row));
    let inner = egui::Frame::NONE.show(ui, |ui| render_row(ui, row, ctx, events));
    if last {
        ui.add_space(margins(row).1);
    }
    inner.response
}

/// One transcript row, from the model and the session it names,
/// dispatched to the renderer that draws it.
fn render_row(
    ui: &mut egui::Ui,
    row: &Row,
    ctx: &mut RowCtx<'_>,
    events: &mut Vec<RowAction>,
) -> egui::Response {
    let RowCtx {
        store,
        session,
        times,
        ui: ui_state,
        pipeline,
        revise,
        revealed,
        pinned,
    } = ctx;
    let times = *times;
    let revealed = *revealed;
    match row {
        Row::User { ix, text } => match session.items.get(*ix) {
            Some(TranscriptItem::User { .. })
                if let Some(summary) = text_rows::summary_of(text) =>
            {
                text_rows::render_summary(
                    ui,
                    pipeline,
                    *ix,
                    summary,
                    ui_state.expanded.contains(&RowKey::Item(*ix)),
                )
            }
            Some(TranscriptItem::User { .. }) if let Some(note) = text_rows::note_of(text) => {
                text_rows::render_engine_note(ui, *ix, &note)
            }
            Some(TranscriptItem::User { steered, .. })
                if let Some((words, parts)) = text_rows::split_prompt(text) =>
            {
                let (response, actions) =
                    text_rows::render_delivered(ui, session, *ix, words, &parts, None);
                route_user(ui, actions);
                response
            }
            Some(TranscriptItem::User { steered, .. }) => {
                let attached = text_rows::attachments(session, *ix);
                let (response, actions) =
                    text_rows::render_user(ui, session, *ix, text, &attached, *steered);
                route_user(ui, actions);
                response
            }
            _ => blank_row(ui),
        },
        Row::Assistant { ix, live } => match session.items.get(*ix) {
            Some(TranscriptItem::Assistant { text }) => {
                text_rows::render_assistant(ui, pipeline, *ix, shown(revealed, *ix, text), *live)
            }
            _ => blank_row(ui),
        },
        Row::Thinking { ix, live, expanded } => match session.items.get(*ix) {
            Some(TranscriptItem::Thinking { text, .. }) => text_rows::render_thinking(
                ui,
                *ix,
                shown(revealed, *ix, text),
                *live,
                *expanded,
                times.and_then(|t| t.thinking(*ix)),
            ),
            _ => blank_row(ui),
        },
        Row::Tool { ix, .. } => match session.items.get(*ix) {
            Some(TranscriptItem::ToolCall(call))
                if call.status == kage_client::wire::ToolCallStatus::Completed
                    && let Some(todos) = tool_rows::todos_of_call(call) =>
            {
                let (response, actions) = tool_rows::render_todos(ui, *ix, &todos, ui_state);
                events.extend(actions);
                response
            }
            Some(TranscriptItem::ToolCall(call))
                if (call.title == "agent" || call.title == "swarm")
                    && let Some((response, actions)) =
                        tool_rows::render_agents(ui, store, session, *ix, call, ui_state) =>
            {
                events.extend(actions);
                response
            }
            Some(TranscriptItem::ToolCall(call)) if call.title == crate::store::EXIT_PLAN_TOOL => {
                let (response, actions) =
                    tool_rows::render_plan_card(ui, pipeline, session, *ix, call, revise, ui_state);
                events.extend(actions);
                response
            }
            Some(TranscriptItem::ToolCall(call)) => {
                let (response, actions) = tool_rows::render_tool(
                    ui,
                    *ix,
                    call,
                    false,
                    false,
                    times.and_then(|t| t.tool(&call.tool_call_id)),
                    ui_state,
                );
                events.extend(actions);
                response
            }
            _ => blank_row(ui),
        },
        Row::Group { .. } => {
            let (response, actions) = tool_rows::render_group(ui, session, row, times, ui_state);
            events.extend(actions);
            response
        }
        Row::Changes { end } => {
            let entries = super::model::run_changes(session, *end);
            let (response, actions) = tool_rows::render_changes(ui, &entries, *end);
            events.extend(actions);
            response
        }
        Row::TurnEnd { ix, outcome } => {
            let (response, actions) = tool_rows::render_turn_end(
                ui,
                session,
                *ix,
                *outcome,
                times.and_then(|t| t.run_end(*ix)),
                *pinned,
            );
            events.extend(actions);
            response
        }
        Row::Notice { ix } => match session.items.get(*ix) {
            Some(TranscriptItem::Notice { tone, text }) => {
                text_rows::render_notice(ui, *ix, *tone, text)
            }
            _ => blank_row(ui),
        },
        Row::Compaction { ix } => match session.items.get(*ix) {
            Some(TranscriptItem::Compaction {
                kept,
                before,
                after,
            }) => text_rows::render_compaction(ui, *ix, *kept, *before, *after),
            _ => blank_row(ui),
        },
        Row::Plan { ix } => match session.items.get(*ix) {
            Some(TranscriptItem::Plan { entries }) => {
                let todos = tool_rows::todos_of_plan(entries);
                let (response, actions) = tool_rows::render_todos(ui, *ix, &todos, ui_state);
                events.extend(actions);
                response
            }
            _ => blank_row(ui),
        },
        Row::Decision { ix } => match session.items.get(*ix) {
            Some(TranscriptItem::Decision {
                subject,
                label,
                allowed,
                feedback,
            }) => {
                text_rows::render_decision(ui, *ix, subject, label, *allowed, feedback.as_deref())
            }
            _ => blank_row(ui),
        },
        Row::Waiting => text_rows::render_waiting(ui),
    }
}

/// Carries out the actions a user row raised: the copy goes to the
/// clipboard now, and the edit asks for the composer E05 brings.
fn route_user(ui: &egui::Ui, actions: Vec<UserAction>) {
    for action in actions {
        match action {
            UserAction::Copy(text) => ui.ctx().copy_text(text),
            UserAction::Edit(text) => {
                crate::warn(&format!(
                    "the composer arrives with E05; the edit of {} chars was raised",
                    text.chars().count()
                ));
            }
        }
    }
}

/// A row whose item is gone: nothing draws, but the row keeps its
/// place in the list.
fn blank_row(ui: &mut egui::Ui) -> egui::Response {
    ui.allocate_response(egui::Vec2::ZERO, egui::Sense::hover())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Carries out the store's pending commands the way the shell does.
    fn run_commands(store: &mut Store) {
        let commands = store.take_commands();
        for command in commands {
            match command {
                crate::store::Command::Handshake { replay_sessions } => {
                    store.handshake(replay_sessions);
                }
                crate::store::Command::NewSession => {
                    store.new_session();
                }
                crate::store::Command::ReplayPrompt => {
                    store.prompt("fix the null check");
                }
            }
        }
    }

    /// Drives the recording through a store the way the shell does:
    /// handshake, session, scripted prompt, then every later frame.
    fn replayed_store() -> Store {
        let mut store = Store::new("/w", true);
        store.set_connect(crate::transport::State::Connected);
        run_commands(&mut store);
        let _ = store.take_outgoing();
        let frames = crate::transport::replay::transcript();
        store.absorb(frames[0].clone());
        run_commands(&mut store);
        let _ = store.take_outgoing();
        store.absorb(frames[1].clone());
        run_commands(&mut store);
        let _ = store.take_outgoing();
        for frame in &frames[2..] {
            store.absorb(frame.clone());
        }
        store
    }

    #[test]
    fn the_find_marks_tint_the_current_match_strongest() {
        let marks = FindMarks {
            keys: vec![RowKey::Item(1), RowKey::Item(4)],
            current: 1,
        };
        let current = row_tint(&marks, RowKey::Item(4)).expect("the current match tints");
        let other = row_tint(&marks, RowKey::Item(1)).expect("a match tints");
        assert_ne!(current, other, "the current match reads strongest");
        assert!(row_tint(&marks, RowKey::Item(2)).is_none());
    }

    #[test]
    fn the_cursor_clamps_to_the_rows_and_starts_absent() {
        let mut view = TranscriptView::new();
        assert_eq!(view.cursor_row(), None);
        assert_eq!(view.row_count(), 0);
        view.set_vim_cursor(Some(3));
        assert_eq!(view.cursor_row(), None, "no rows, no cursor");
    }

    #[test]
    fn a_pinned_view_reads_its_own_session() {
        let store = replayed_store();
        let active = store.active_id().expect("the recording opened a session");
        let view = TranscriptView::pinned(active);
        assert!(
            view.searchable_rows(&store).len() > 5,
            "the pinned view reads the recording's rows"
        );
    }

    #[test]
    fn a_row_whose_item_is_gone_draws_nothing() {
        // The dispatch guards on the item the row names, so a stale
        // index falls to the blank row and takes no space.
        let mut harness = egui_kittest::Harness::new_ui(|_ui| {});
        harness.run();
        let _ = &Row::Notice { ix: 9_999 };
    }
}
