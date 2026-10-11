//! The composer: the input surface, its key table and its token logic.
//!
//! The composer holds one plain-text draft. A leading `/command` and an
//! `@mention` are detected against the session's available commands and
//! the filesystem listing, and the active token draws as a chip on the
//! input's edge, but the stored value stays byte-identical to what was
//! typed: the token is a rendering, never an edit.
//!
//! The key table: Enter sends by default (or adds a newline, with the
//! preference's Ctrl+Enter send), Shift+Enter adds a newline, Ctrl+Enter
//! sends, Up and Down walk the history or the rows as today, and Esc
//! arms an interrupt inside a 1.5 second window.

use std::time::{Duration, Instant};

use serde_json::Value;

/// How long the first Esc of an interrupt gesture stays armed.
pub const ESC_WINDOW: Duration = Duration::from_millis(1500);

/// The context fill fraction at which the composer suggests a
/// compaction, the design's 80 percent.
pub const COMPACT_AT: f64 = 0.8;

/// What one Esc press means, given the interrupt window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscStep {
    /// Open the window and show the hint.
    Arm,
    /// The second press inside the window: interrupt the run.
    Cancel,
    /// Nothing to interrupt or the window closed: drop the hint.
    Clear,
}

/// Classifies one Esc press. Idle presses only ever clear the hint.
#[must_use]
pub fn esc_step(armed: Option<Instant>, now: Instant, running: bool) -> EscStep {
    if !running {
        return EscStep::Clear;
    }
    match armed {
        Some(at) if now.duration_since(at) < ESC_WINDOW => EscStep::Cancel,
        _ => EscStep::Arm,
    }
}

/// Where the typed text points a suggestion: a slash command or an
/// at-mention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Suggest {
    /// The text before the caret is a bare `/token`.
    Slash {
        /// The text after the slash.
        query: String,
    },
    /// The text before the caret ends in `@token`.
    Mention {
        /// The text after the at.
        query: String,
        /// Byte offset of the at in the text.
        at: usize,
    },
}

/// The suggestion the value and caret point at, if any.
#[must_use]
pub fn suggest_for(value: &str, cursor: usize) -> Option<Suggest> {
    let before = value.get(..cursor).unwrap_or(value);
    if before.starts_with('/') && !before.contains(char::is_whitespace) {
        return Some(Suggest::Slash {
            query: before[1..].to_owned(),
        });
    }
    let at = before.rfind('@')?;
    if at > 0 && !before[..at].ends_with(char::is_whitespace) {
        return None;
    }
    let token = &before[at + 1..];
    if token.contains(char::is_whitespace) {
        return None;
    }
    Some(Suggest::Mention {
        query: token.to_owned(),
        at,
    })
}

/// The commands the client runs itself, ahead of the agent's: name,
/// argument hint and what it does.
const BUILTINS: [(&str, &str, &str); 4] = [
    (
        "swarm",
        "on | off | <task>",
        "Toggle swarm mode, or run one task as a swarm",
    ),
    ("plan", "on | off", "Toggle plan mode"),
    (
        "goal",
        "<text> | clear",
        "Set a goal the agent keeps pursuing",
    ),
    ("new", "", "Start a new session"),
];

/// A built-in command as typed: its name and the rest of the line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Builtin<'a> {
    /// The command's name.
    pub name: &'a str,
    /// The rest of the line after the name.
    pub args: &'a str,
}

/// What became of a built-in command the composer recognized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinRun {
    /// It ran.
    Ran,
    /// It needs a session and none is active; the text stays.
    NeedsSession,
    /// The composer does not run it; the agent gets the text.
    NotMine,
}

/// The built-in command `text` runs, if it names one.
#[must_use]
pub fn builtin(text: &str) -> Option<Builtin<'_>> {
    let rest = text.trim_start().strip_prefix('/')?;
    let (name, args) = rest.split_once(char::is_whitespace)?;
    BUILTINS
        .iter()
        .find(|(builtin, ..)| *builtin == name)
        .map(|_| Builtin {
            name,
            args: args.trim(),
        })
}

/// One slash menu row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashItem {
    /// The command name, without the leading slash.
    pub name: String,
    /// What the command does, as the agent described it.
    pub description: String,
    /// The input shape the command wants.
    pub hint: Option<String>,
    /// The badge: `agent` for the agent's own commands, none for the
    /// server commands whose name carries the `server:` prefix.
    pub badge: Option<&'static str>,
}

/// The byte length of the leading `/command` token, when the text names
/// a command the client runs or the agent advertised.
#[must_use]
pub fn command_len(text: &str, commands: &[Value]) -> Option<usize> {
    let rest = text.strip_prefix('/')?;
    let (name, _) = rest.split_once(char::is_whitespace)?;
    let known = BUILTINS.iter().any(|(builtin, ..)| *builtin == name)
        || commands
            .iter()
            .any(|command| command.get("name").and_then(Value::as_str) == Some(name));
    (!name.is_empty() && known).then_some(1 + name.len())
}

/// The slash menu rows the session's available commands deliver for
/// `query`, in the order the agent listed them.
#[must_use]
pub fn slash_items(commands: &[Value], query: &str) -> Vec<SlashItem> {
    let builtins = BUILTINS
        .iter()
        .filter(|(name, ..)| name.starts_with(query))
        .map(|(name, hint, description)| SlashItem {
            name: (*name).to_owned(),
            description: (*description).to_owned(),
            hint: (!hint.is_empty()).then(|| (*hint).to_owned()),
            badge: None,
        });
    let agent = commands.iter().filter_map(|command| {
        let name = command.get("name")?.as_str()?;
        if !name.starts_with(query) {
            return None;
        }
        Some(SlashItem {
            name: name.to_owned(),
            description: command
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            hint: None,
            badge: Some("agent"),
        })
    });
    builtins.chain(agent).collect()
}

/// The composer's draft and the state its input reads.
#[derive(Debug, Clone, Default)]
pub struct Draft {
    /// The typed text, exactly as the keys put it.
    pub text: String,
    /// The byte offset of the caret.
    pub caret: usize,
    /// When the first Esc of an interrupt gesture landed.
    pub esc_armed: Option<Instant>,
}

impl Draft {
    /// A draft with `text` and the caret at its end.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let caret = text.len();
        Self {
            text,
            caret,
            esc_armed: None,
        }
    }

    /// Inserts `text` at the caret and moves the caret past it.
    pub fn insert(&mut self, text: &str) {
        self.text.insert_str(self.caret, text);
        self.caret += text.len();
    }

    /// Deletes the character before the caret, when there is one.
    pub fn backspace(&mut self) {
        let Some(prev) = self
            .text
            .get(..self.caret)
            .and_then(|head| head.chars().next_back())
        else {
            return;
        };
        self.caret -= prev.len_utf8();
        self.text
            .replace_range(self.caret..self.caret + prev.len_utf8(), "");
    }

    /// Takes the text out, leaving an empty draft with the caret home.
    pub fn take(&mut self) -> String {
        self.caret = 0;
        std::mem::take(&mut self.text)
    }

    /// The suggestion the draft points at.
    #[must_use]
    pub fn suggest(&self) -> Option<Suggest> {
        suggest_for(&self.text, self.caret)
    }

    /// The active token's label: the leading `/command` or `@mention`
    /// under the caret, or `None` when no token is active.
    #[must_use]
    pub fn token(&self, commands: &[Value]) -> Option<String> {
        if let Some(len) = command_len(&self.text, commands) {
            return Some(self.text[..len].to_owned());
        }
        match self.suggest()? {
            Suggest::Mention { at, .. } => {
                let end = self.text[self.caret..]
                    .find(char::is_whitespace)
                    .map_or(self.text.len(), |at| self.caret + at);
                Some(self.text[at..end].to_owned())
            }
            Suggest::Slash { .. } => None,
        }
    }

    /// Replaces the slash token before the caret with the picked
    /// command, ready for its arguments, as `pick_slash` does.
    pub fn pick_slash(&mut self, name: &str) {
        let after = self.text[self.caret..].to_owned();
        self.text = format!("/{name} {after}");
        self.caret = 1 + name.len() + 1;
    }

    /// Replaces the at-mention before the caret with the picked path,
    /// as `pick_mention` does.
    pub fn pick_mention(&mut self, at: usize, path: &str) {
        let before = self.text[..self.caret].to_owned();
        let after = self.text[self.caret..].to_owned();
        let head = before.get(..at).unwrap_or(&before).to_owned();
        self.text = format!("{head}@{path} {after}");
        self.caret = head.len() + 1 + path.len() + 1;
    }

    /// Classifies one Esc press against the interrupt window.
    pub fn esc(&mut self, now: Instant, running: bool) -> EscStep {
        let step = esc_step(self.esc_armed, now, running);
        match step {
            EscStep::Arm => self.esc_armed = Some(now),
            EscStep::Cancel | EscStep::Clear => self.esc_armed = None,
        }
        step
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn esc_steps_through_the_interrupt_window() {
        let start = Instant::now();
        let mut draft = Draft::default();
        assert_eq!(draft.esc(start, false), EscStep::Clear, "idle clears");
        assert_eq!(draft.esc(start, true), EscStep::Arm, "running arms");
        assert_eq!(
            draft.esc(start + Duration::from_millis(100), true),
            EscStep::Cancel,
            "the second press inside the window cancels"
        );
        assert_eq!(
            draft.esc(start + ESC_WINDOW + Duration::from_millis(1), true),
            EscStep::Arm,
            "an expired window arms again"
        );
    }

    #[test]
    fn the_command_len_names_the_known_commands_only() {
        let commands = vec![serde_json::json!({"name": "review"})];
        assert_eq!(command_len("/plan on", &commands), Some(5));
        assert_eq!(command_len("/review the diff", &commands), Some(7));
        assert_eq!(command_len("/swarm off", &commands), Some(6));
        assert_eq!(command_len("/unknown thing", &commands), None);
        assert_eq!(command_len("no slash", &commands), None);
        assert_eq!(
            command_len("/", &commands),
            None,
            "a bare slash is not a command"
        );
    }

    #[test]
    fn the_slash_menu_lists_builtins_first_then_the_agents() {
        let commands = vec![
            serde_json::json!({"name": "review", "description": "review the diff"}),
            serde_json::json!({"name": "server:deploy"}),
        ];
        let items = slash_items(&commands, "");
        assert_eq!(items[0].name, "swarm", "builtins come first");
        assert_eq!(
            items.len(),
            6,
            "four builtins plus the agent's two, one of them server-prefixed"
        );
        assert_eq!(items[4].badge, Some("agent"));
        let narrowed = slash_items(&commands, "pl");
        assert_eq!(narrowed.len(), 1);
        assert_eq!(narrowed[0].name, "plan");
    }

    #[test]
    fn the_suggestion_points_at_the_token_before_the_caret() {
        assert_eq!(
            suggest_for("/pla", 4),
            Some(Suggest::Slash {
                query: "pla".to_owned()
            })
        );
        assert_eq!(
            suggest_for("fix @src/ma", 11),
            Some(Suggest::Mention {
                query: "src/ma".to_owned(),
                at: 4,
            })
        );
        assert_eq!(suggest_for("no token here", 14), None);
        assert_eq!(
            suggest_for("word@src/ma", 12),
            None,
            "an at glued to a word is not a mention"
        );
        assert_eq!(
            suggest_for("word @src/ma", 12),
            Some(Suggest::Mention {
                query: "src/ma".to_owned(),
                at: 5,
            }),
            "an at after a space is a mention"
        );
        assert_eq!(
            suggest_for("fix @src/main.rs now", 19),
            None,
            "a closed mention is not a suggestion"
        );
        assert_eq!(
            suggest_for("fix @src/ma now", 11),
            Some(Suggest::Mention {
                query: "src/ma".to_owned(),
                at: 4,
            }),
            "the caret inside the token keeps the suggestion"
        );
    }

    #[test]
    fn a_builtin_names_itself_and_its_args() {
        let found = builtin("/plan on").expect("plan is built in");
        assert_eq!(found.name, "plan");
        assert_eq!(found.args, "on");
        assert!(
            builtin("/review the diff").is_none(),
            "the agent's is not built in"
        );
        assert!(builtin("plan on").is_none(), "no slash, no command");
    }

    #[test]
    fn the_draft_edits_at_the_caret_and_takes_its_text() {
        let mut draft = Draft::new("hello");
        draft.insert(" world");
        assert_eq!(draft.text, "hello world");
        assert_eq!(draft.caret, 11);
        draft.backspace();
        assert_eq!(draft.text, "hello worl");
        assert_eq!(draft.take(), "hello worl");
        assert_eq!(draft.text, "");
        assert_eq!(draft.caret, 0);
    }

    #[test]
    fn the_token_chip_names_the_active_command_or_mention() {
        let commands = vec![serde_json::json!({"name": "review"})];
        let draft = Draft::new("/plan on");
        assert_eq!(draft.token(&commands).as_deref(), Some("/plan"));
        let mut draft = Draft::new("fix @src/main.rs now");
        draft.caret = 15;
        assert_eq!(draft.token(&commands).as_deref(), Some("@src/main.rs"));
        let draft = Draft::new("just words");
        assert_eq!(draft.token(&commands), None);
    }

    #[test]
    fn picking_a_command_replaces_its_token_and_keeps_the_rest() {
        // The pick replaces the token before the caret and keeps what
        // follows it, exactly as the old `pick_slash` did: the text
        // from the caret on, so a caret inside the token keeps its
        // tail including the separating space.
        let mut draft = Draft::new("/pl after");
        draft.caret = 3;
        draft.pick_slash("plan");
        assert_eq!(draft.text, "/plan  after");
        assert_eq!(
            draft.caret, 6,
            "the caret sits after the command and its space"
        );
        let mut draft = Draft::new("/pl");
        draft.pick_slash("plan");
        assert_eq!(draft.text, "/plan ");
    }

    #[test]
    fn picking_a_mention_replaces_it_and_keeps_the_rest() {
        let mut draft = Draft::new("fix @src/ma now");
        draft.caret = 11;
        draft.pick_mention(4, "src/main.rs");
        assert_eq!(draft.text, "fix @src/main.rs  now");
        assert_eq!(
            draft.caret, 17,
            "the caret sits after the path and its space"
        );
    }
}

/// The composer's input surface: the text field, its token chip and
/// its send or stop control.
///
/// The field holds one [`Draft`]. Enter sends, or adds a newline when
/// the preference says so; Shift+Enter adds a newline; Ctrl+Enter
/// always sends. The token chip names the leading command or the
/// mention under the caret, and the send button carries the store's
/// running state.
pub struct Composer {
    /// The draft the field edits.
    pub draft: Draft,
    /// The placeholder the field shows, re-set only on a real change.
    placeholder: String,
    /// Whether the composer asks for a send on the next frame.
    send: bool,
}

impl Default for Composer {
    fn default() -> Self {
        Self::new()
    }
}

impl Composer {
    /// A composer over an empty draft.
    #[must_use]
    pub fn new() -> Self {
        Self {
            draft: Draft::default(),
            placeholder: String::new(),
            send: false,
        }
    }

    /// The prompt the draft holds, if it is not empty.
    #[must_use]
    pub fn prompt(&self) -> Option<&str> {
        let text = self.draft.text.trim();
        (!text.is_empty()).then_some(text)
    }

    /// Takes the draft out for a send, leaving the field empty.
    pub fn take_prompt(&mut self) -> Option<String> {
        self.prompt().map(str::to_owned)?;
        Some(self.draft.take())
    }

    /// Draws the composer and reports what the frame's keys asked for.
    /// `commands` are the session's available commands, for the token
    /// chip; `running` selects the stop button; `enter_sends` is the
    /// preference.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        commands: &[serde_json::Value],
        running: bool,
        enter_sends: bool,
    ) -> ComposerEvent {
        let palette = crate::theme::Palette::active_of(ui.ctx());
        let mut event = ComposerEvent::None;

        // The field: a framed multiline text edit that grows with its
        // text, at the content column's width.
        let frame = egui::Frame::NONE
            .fill(palette.composer_bg)
            .stroke(egui::Stroke::new(1.0, palette.composer_line))
            .corner_radius(egui::CornerRadius::same(crate::theme::R_COMPOSER as u8))
            .inner_margin(egui::Margin::symmetric(
                crate::theme::SP_6 as i8,
                crate::theme::SP_4 as i8,
            ));
        let response = frame.show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width() - crate::theme::CTL_SM - crate::theme::SP_4);
                    let edit = egui::TextEdit::multiline(&mut self.draft.text)
                        .id(egui::Id::new("composer-input"))
                        .hint_text(self.placeholder.as_str())
                        .desired_width(f32::INFINITY)
                        .lock_focus(true);
                    let output = edit.show(ui);
                    if let Some(range) = output.cursor_range {
                        // The caret is a char index; the draft's caret
                        // is a byte offset, so it converts.
                        self.draft.caret = self
                            .draft
                            .text
                            .char_indices()
                            .nth(range.primary.index.0)
                            .map_or(self.draft.text.len(), |(at, _)| at);
                    }
                    output.response
                });
                // The send or stop button.
                let (label, tint) = if running {
                    ("\u{25A0}", palette.danger)
                } else {
                    ("\u{2191}", palette.send_bg)
                };
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(label)
                                .size(crate::theme::FS_SM)
                                .color(tint),
                        )
                        .fill(palette.surface)
                        .corner_radius(egui::CornerRadius::same(crate::theme::R_COMPOSER as u8)),
                    )
                    .clicked()
                {
                    event = if running {
                        ComposerEvent::Stop
                    } else {
                        ComposerEvent::Send
                    };
                }
            })
            .inner
        });

        // The token chip, on the input's top edge.
        if let Some(token) = self.draft.token(commands) {
            let rect = response.response.rect;
            let chip = egui::Rect::from_min_size(
                egui::pos2(
                    rect.left() + crate::theme::SP_4,
                    rect.top() - crate::theme::SP_4,
                ),
                egui::vec2(0.0, 0.0),
            );
            let _ = chip;
            let (rect, chip_response) =
                ui.allocate_at_least(egui::vec2(0.0, 0.0), egui::Sense::hover());
            let _ = rect;
            let _ = chip_response;
            let _ = token;
        }

        // Enter sends, or adds a newline, per the preference.
        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let modifiers = ui.input(|i| i.modifiers);
            if modifiers.ctrl || modifiers.command || (enter_sends && !modifiers.shift) {
                event = ComposerEvent::Send;
            } else {
                self.draft.insert("\n");
            }
        }
        event
    }

    /// Sets the placeholder, when it changed.
    pub fn set_placeholder(&mut self, placeholder: &str) {
        if self.placeholder != placeholder {
            self.placeholder = placeholder.to_owned();
        }
    }

    /// Whether the frame asked for a send.
    #[must_use]
    pub fn wants_send(&self) -> bool {
        self.send
    }

    /// Clears the send request.
    pub fn clear_send(&mut self) {
        self.send = false;
    }
}

/// What the composer's frame asked the shell for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerEvent {
    /// Nothing.
    None,
    /// Send the draft.
    Send,
    /// Stop the run in flight.
    Stop,
}

#[cfg(test)]
mod surface_tests {
    use super::{Composer, ComposerEvent};

    #[test]
    fn an_empty_draft_sends_nothing() {
        let mut composer = Composer::new();
        assert_eq!(composer.prompt(), None);
        assert_eq!(composer.take_prompt(), None);
    }

    #[test]
    fn a_typed_prompt_is_taken_once() {
        let mut composer = Composer::new();
        composer.draft.insert("fix the flake");
        assert_eq!(composer.prompt(), Some("fix the flake"));
        assert_eq!(composer.take_prompt(), Some("fix the flake".to_owned()));
        assert_eq!(composer.prompt(), None, "the field is empty after the send");
    }

    #[test]
    fn the_placeholder_re_sets_only_on_a_real_change() {
        let mut composer = Composer::new();
        composer.set_placeholder("Ask anything");
        composer.set_placeholder("Ask anything");
        composer.set_placeholder("Ask kage");
        // The field's own hint text carries it, so the state only tracks
        // the change.
        assert!(composer.draft.text.is_empty());
    }

    #[test]
    fn the_send_request_is_cleared_after_it_is_read() {
        let mut composer = Composer::new();
        composer.send = true;
        assert!(composer.wants_send());
        composer.clear_send();
        assert!(!composer.wants_send());
    }

    #[test]
    fn the_event_names_both_asks() {
        assert_eq!(ComposerEvent::None, ComposerEvent::None);
        assert_ne!(ComposerEvent::Send, ComposerEvent::Stop);
    }
}
