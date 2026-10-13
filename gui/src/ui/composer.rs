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
//!
//! The suggestion menu: a leading `/token` lists the commands, and an
//! `@token` before the caret lists the session workdir's entries. A
//! picked row inserts plain text, exactly as the old `pick_slash` and
//! `pick_mention` did, so the stored value stays byte-identical.

use std::time::{Duration, Instant};

use serde_json::Value;

/// How long the first Esc of an interrupt gesture stays armed.
pub const ESC_WINDOW: Duration = Duration::from_millis(1500);

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

/// The built-in command `text` runs, if it names one. A command with
/// no arguments is a command with empty arguments, so a bare `/goal`
/// names the goal command rather than falling through to the agent.
#[must_use]
pub fn builtin(text: &str) -> Option<Builtin<'_>> {
    let rest = text.trim_start().strip_prefix('/')?;
    let (name, args) = match rest.split_once(char::is_whitespace) {
        Some((name, args)) => (name, args.trim()),
        None => (rest, ""),
    };
    BUILTINS
        .iter()
        .find(|(builtin, ..)| *builtin == name)
        .map(|_| Builtin { name, args })
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

/// One mention menu row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MentionItem {
    /// The path relative to the session workdir.
    pub path: String,
    /// Whether the entry is a directory.
    pub directory: bool,
}

/// The mention rows a `_kage/fs` listing delivers for `query`, case
/// insensitively. No listing yields no rows: the menu says so itself.
#[must_use]
pub fn mention_items(
    listing: Option<&kage_client::wire::FsListResult>,
    query: &str,
) -> Vec<MentionItem> {
    let query = query.to_lowercase();
    listing
        .into_iter()
        .flat_map(|listing| listing.entries.iter())
        .filter(|entry| entry.path.to_lowercase().contains(&query))
        .map(|entry| MentionItem {
            path: entry.path.clone(),
            directory: entry.kind == kage_client::wire::FsKind::Directory,
        })
        .collect()
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

    /// The byte offset the mention token under the caret starts at,
    /// or the caret itself when no token is active.
    #[must_use]
    fn token_at(&self) -> usize {
        match self.suggest() {
            Some(Suggest::Mention { at, .. }) => at,
            _ => self.caret,
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

/// The rows the suggestion menu lists: the slash items, the mention
/// items, or `None` when the draft points at no menu. An empty slash
/// list closes the menu, as the old panel did; a mention with no
/// listing yet keeps a single explanatory row.
fn suggestion_rows(
    suggestion: Option<&Suggest>,
    commands: &[Value],
    listing: Option<&kage_client::wire::FsListResult>,
) -> Option<Vec<MenuRow>> {
    match suggestion? {
        Suggest::Slash { query } => {
            let items = slash_items(commands, query);
            if items.is_empty() {
                return None;
            }
            Some(
                items
                    .into_iter()
                    .map(|item| MenuRow {
                        icon: if item.badge.is_none() {
                            crate::icons::Icon::Server
                        } else {
                            crate::icons::Icon::Slash
                        },
                        title: format!("/{}", item.name),
                        detail: item.hint.unwrap_or_default(),
                        description: item.description,
                        badge: item.badge.map(str::to_owned),
                        pick: MenuPick::Slash(item.name),
                    })
                    .collect(),
            )
        }
        Suggest::Mention { query, at } => {
            if listing.is_none() {
                return Some(vec![MenuRow {
                    icon: crate::icons::Icon::AtSign,
                    title: "no file listing yet; the session workdir was asked".to_owned(),
                    detail: String::new(),
                    description: String::new(),
                    badge: None,
                    pick: MenuPick::None,
                }]);
            }
            let items = mention_items(listing, query);
            if items.is_empty() {
                return None;
            }
            Some(
                items
                    .into_iter()
                    .map(|item| MenuRow {
                        icon: if item.directory {
                            crate::icons::Icon::Folder
                        } else {
                            crate::icons::Icon::File
                        },
                        title: item.path.clone(),
                        detail: String::new(),
                        description: String::new(),
                        badge: Some(if item.directory { "dir" } else { "file" }.to_owned()),
                        pick: MenuPick::Mention {
                            at: *at,
                            path: item.path,
                        },
                    })
                    .collect(),
            )
        }
    }
}

/// One suggestion menu row.
struct MenuRow {
    /// The row's leading icon.
    icon: crate::icons::Icon,
    /// The row's title, in the mono family.
    title: String,
    /// The row's second line, when it carries one.
    detail: String,
    /// The row's description, under the title.
    description: String,
    /// The row's trailing badge.
    badge: Option<String>,
    /// What picking the row does.
    pick: MenuPick,
}

/// What one suggestion row does when it is picked.
enum MenuPick {
    /// Insert the named command.
    Slash(String),
    /// Replace the mention at `at` with `path`.
    Mention { at: usize, path: String },
    /// Nothing: the row only reports a state.
    None,
}

/// Draws the suggestion menu over the composer at `at`. A picked row
/// writes plain text into the draft, as the old `pick_slash` and
/// `pick_mention` did.
fn show_suggestion_menu(
    ctx: &egui::Context,
    palette: &crate::theme::Palette,
    rows: Vec<MenuRow>,
    at: egui::Pos2,
    draft: &mut Draft,
) {
    let width = SUGGEST_W;
    let height = (rows.len() as f32 * 44.0 + 10.0).min(SUGGEST_MAX_H);
    // The menu opens above the composer, and a composer near the top
    // of a short window would push it off screen, so the position is
    // clamped to the viewport.
    let mut at = at - egui::vec2(0.0, height + 6.0);
    let screen = ctx.content_rect();
    at.x = at.x.clamp(
        screen.left() + 8.0,
        (screen.right() - width - 8.0).max(screen.left() + 8.0),
    );
    at.y = at.y.max(screen.top() + 8.0);
    egui::Area::new(egui::Id::new("composer-suggest"))
        .order(egui::Order::Foreground)
        .fixed_pos(at)
        .show(ctx, |ui| {
            egui::Frame::NONE
                .fill(palette.menu)
                .stroke(egui::Stroke::new(1.0, palette.line))
                .corner_radius(egui::CornerRadius::same(crate::theme::R_LG as u8))
                .inner_margin(egui::Margin::same(5))
                .shadow(palette.shadow_menu)
                .show(ui, |ui| {
                    ui.set_width(width);
                    ui.set_max_height(SUGGEST_MAX_H);
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.set_width(width - 10.0);
                        for row in &rows {
                            let response = egui::Frame::NONE
                                .fill(egui::Color32::TRANSPARENT)
                                .corner_radius(egui::CornerRadius::same(crate::theme::R_MD as u8))
                                .inner_margin(egui::Margin::symmetric(9, 7))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 10.0;
                                        crate::icons::icon(ui, row.icon, 16.0, palette.muted);
                                        ui.vertical(|ui| {
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = 6.0;
                                                ui.label(
                                                    egui::RichText::new(row.title.clone())
                                                        .size(12.5)
                                                        .family(crate::theme::family(
                                                            crate::theme::FAMILY_MONO_REGULAR,
                                                        ))
                                                        .color(palette.ink),
                                                );
                                                if !row.detail.is_empty() {
                                                    ui.label(
                                                        egui::RichText::new(row.detail.clone())
                                                            .size(11.0)
                                                            .family(crate::theme::family(
                                                                crate::theme::FAMILY_MONO_REGULAR,
                                                            ))
                                                            .color(palette.faint),
                                                    );
                                                }
                                            });
                                            if !row.description.is_empty() {
                                                ui.label(
                                                    egui::RichText::new(row.description.clone())
                                                        .size(crate::theme::FS_XS)
                                                        .color(palette.muted),
                                                );
                                            }
                                        });
                                        if let Some(badge) = &row.badge {
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    ui.label(
                                                        egui::RichText::new(badge.clone())
                                                            .size(crate::theme::FS_2XS)
                                                            .color(palette.faint),
                                                    );
                                                },
                                            );
                                        }
                                    });
                                })
                                .response;
                            if response.hovered() {
                                let rect = response.rect;
                                ui.painter().rect_filled(
                                    rect,
                                    egui::CornerRadius::same(crate::theme::R_MD as u8),
                                    palette.selected,
                                );
                            }
                            if response.clicked() {
                                match &row.pick {
                                    MenuPick::Slash(name) => draft.pick_slash(name),
                                    MenuPick::Mention { at, path } => {
                                        draft.pick_mention(*at, path);
                                    }
                                    MenuPick::None => {}
                                }
                            }
                        }
                    });
                });
        });
}

/// The suggestion surface's width.
const SUGGEST_W: f32 = 460.0;

/// The suggestion surface's max height, before it scrolls.
const SUGGEST_MAX_H: f32 = 300.0;

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
    /// The session workdir listing the mention menu reads, cloned out
    /// of the store each frame it is asked for.
    listing: Option<kage_client::wire::FsListResult>,
    /// The toolbar under the field: pickers, context ring, send.
    toolbar: crate::ui::toolbar::Toolbar,
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
            listing: None,
            toolbar: crate::ui::toolbar::Toolbar::new(),
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
        store: &mut crate::store::Store,
        commands: &[serde_json::Value],
        running: bool,
        enter_sends: bool,
    ) -> ComposerEvent {
        let palette = crate::theme::Palette::active_of(ui.ctx());
        let mut event = ComposerEvent::None;
        // What the toolbar asked the shell for, carried out of the
        // frame's borrow of the store.
        let mut goal = false;
        let mut confirm_swarm = false;
        let has_text = !self.draft.text.trim().is_empty();
        self.sync_placeholder(store, running);

        // The suggestion menu, over the field: the slash rows or the
        // mention rows the draft points at.
        let suggestion = self.draft.suggest();
        let menu_rows = suggestion_rows(suggestion.as_ref(), commands, self.listing.as_ref());
        let menu_top = ui.next_widget_position().y - 6.0;
        let menu_left = ui.next_widget_position().x;

        // The card: the field over the toolbar row, the way the
        // reference composer stacks them. The border names the state:
        // plan, then swarm, then focus, then rest.
        let focused = ui
            .ctx()
            .memory(|memory| memory.has_focus(egui::Id::new("composer-input")));
        let border = if store.plan_on() {
            palette.accent_bd
        } else if store.swarm_on() {
            palette.done_bd
        } else if focused {
            palette.composer_focus_line
        } else {
            palette.composer_line
        };
        let frame = egui::Frame::NONE
            .fill(palette.composer_bg)
            .stroke(egui::Stroke::new(1.0, border))
            .corner_radius(egui::CornerRadius::same(crate::theme::R_COMPOSER as u8))
            .shadow(palette.shadow_input)
            .outer_margin(egui::Margin::ZERO);
        let response = frame.show(ui, |ui| {
            ui.vertical(|ui| {
                // The field: frameless over the card's fill, its own
                // 14/16/16/4 padding, and the reference's 60px floor.
                egui::Frame::NONE
                    .inner_margin(egui::Margin {
                        left: 16,
                        right: 16,
                        top: 14,
                        bottom: 4,
                    })
                    .show(ui, |ui| {
                        let width = ui.available_width();
                        let edit = egui::TextEdit::multiline(&mut self.draft.text)
                            .id(egui::Id::new("composer-input"))
                            .hint_text(self.placeholder.as_str())
                            .desired_width(width)
                            .min_size(egui::vec2(width, 60.0))
                            .frame(egui::Frame::NONE)
                            .margin(egui::Margin::ZERO)
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
                // The toolbar row under the field, the reference's
                // 6/8/8 padding around it.
                egui::Frame::NONE
                    .inner_margin(egui::Margin {
                        left: 8,
                        right: 8,
                        top: 6,
                        bottom: 8,
                    })
                    .show(ui, |ui| {
                        let toolbar_events = crate::ui::toolbar::show(
                            ui,
                            &mut self.toolbar,
                            store,
                            running,
                            has_text,
                        );
                        for asked in toolbar_events {
                            match asked {
                                crate::ui::toolbar::ToolbarEvent::Submit => {
                                    event = ComposerEvent::Send { steer: false };
                                }
                                crate::ui::toolbar::ToolbarEvent::Cancel => {
                                    event = ComposerEvent::Stop;
                                }
                                crate::ui::toolbar::ToolbarEvent::OpenGoal => {
                                    goal = true;
                                }
                                crate::ui::toolbar::ToolbarEvent::TogglePlan => {
                                    if store.plan_on() {
                                        store.exit_plan();
                                    } else {
                                        store.enter_plan();
                                    }
                                }
                                crate::ui::toolbar::ToolbarEvent::ConfirmSwarm => {
                                    confirm_swarm = true;
                                }
                                crate::ui::toolbar::ToolbarEvent::SwarmOff => {
                                    store.set_option("swarm", "off");
                                }
                                crate::ui::toolbar::ToolbarEvent::SeedMention => {
                                    self.draft.insert(" @");
                                }
                                crate::ui::toolbar::ToolbarEvent::SeedSlash => {
                                    self.draft.text = "/".to_owned();
                                    self.draft.caret = 1;
                                }
                                crate::ui::toolbar::ToolbarEvent::Compact => {
                                    store.compact();
                                }
                                crate::ui::toolbar::ToolbarEvent::OpenProviders => {
                                    event = ComposerEvent::OpenProviders;
                                }
                            }
                        }
                    });
            })
        });

        // The hint line under the card: the key table on the left, the
        // session's cost on the right, as the reference draws it.
        hint_line(ui, store, running, enter_sends);

        // The token chip: the active `/command` or `@mention` drawn as
        // a badge on the input's top edge. The stored text stays plain;
        // the chip is a rendering.
        if let Some(token) = self.draft.token(commands) {
            let rect = response.response.rect;
            let chip_font = egui::FontId::new(
                crate::theme::FS_2XS,
                crate::theme::family(crate::theme::FAMILY_MONO_REGULAR),
            );
            let galley = ui
                .painter()
                .layout_no_wrap(token.clone(), chip_font, palette.accent);
            let width = galley.size().x + 12.0;
            let height = galley.size().y + 5.0;
            let chip_rect = egui::Rect::from_min_size(
                egui::pos2(rect.left() + crate::theme::SP_4, rect.top() - height / 2.0),
                egui::vec2(width, height),
            );
            ui.painter().rect_filled(
                chip_rect,
                egui::CornerRadius::same(crate::theme::R_FULL as u8),
                palette.accent_soft,
            );
            ui.painter().rect_stroke(
                chip_rect,
                egui::CornerRadius::same(crate::theme::R_FULL as u8),
                egui::Stroke::new(1.0, palette.accent_bd),
                egui::StrokeKind::Inside,
            );
            ui.painter().galley(
                chip_rect.center() - galley.size() / 2.0,
                galley,
                palette.accent,
            );
        }

        if let Some(rows) = menu_rows {
            show_suggestion_menu(
                ui.ctx(),
                palette,
                rows,
                egui::pos2(menu_left, menu_top),
                &mut self.draft,
            );
        }

        // The goal and swarm asks outrank a send, so the frame reports
        // the dialog rather than the prompt.
        if goal {
            event = ComposerEvent::OpenGoal;
        } else if confirm_swarm {
            event = ComposerEvent::ConfirmSwarm;
        }

        // Enter sends, or adds a newline, per the preference. The
        // toolbar's own ask wins, so a click and a keypress in one
        // frame do not race.
        if event == ComposerEvent::None && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let modifiers = ui.input(|i| i.modifiers);
            if modifiers.ctrl || modifiers.command || (enter_sends && !modifiers.shift) {
                event = ComposerEvent::Send {
                    steer: modifiers.ctrl || modifiers.command,
                };
            } else {
                self.draft.insert("\n");
            }
        }
        // In vim mode Escape leaves the composer for normal mode,
        // where a second Escape interrupts a running turn. A
        // suggestion menu takes the key first, as it does a click.
        if event == ComposerEvent::None
            && store.prefs().vim
            && ui.input(|i| i.key_pressed(egui::Key::Escape))
            && self.draft.suggest().is_none()
        {
            event = ComposerEvent::LeaveInsert;
        }
        event
    }

    /// Keeps the field's placeholder on the per-state phrasing: queueing
    /// while a run is in flight, then swarm, then plan, then an open
    /// ask, then the default.
    fn sync_placeholder(&mut self, store: &crate::store::Store, running: bool) {
        let wanted = if running {
            "Queue a follow-up, or Ctrl+Enter to steer the running turn"
        } else if store.swarm_on() {
            "Describe work to split across parallel agents..."
        } else if store.plan_on() {
            "Describe what to plan..."
        } else if store
            .active_session()
            .is_some_and(|session| !session.permissions.is_empty())
        {
            "Answer the request above first, or type to queue"
        } else {
            "Ask kage anything, @ to mention, / for commands"
        };
        self.set_placeholder(wanted);
    }

    /// Replaces the draft with `text`, the caret at its end. The dock's
    /// edit action uses this, so the caret stays a valid byte offset.
    pub fn set_draft(&mut self, text: String) {
        self.draft.text = text;
        self.draft.caret = self.draft.text.len();
    }

    /// Inserts a `@path` mention of `path` at the caret, replacing any
    /// mention token already under it, so a file picked in the
    /// workbench lands as a typed one would.
    pub fn mention(&mut self, path: &str) {
        let at = self.draft.token_at();
        self.draft.pick_mention(at, path);
    }

    /// Sets the placeholder, when it changed.
    pub fn set_placeholder(&mut self, placeholder: &str) {
        if self.placeholder != placeholder {
            self.placeholder = placeholder.to_owned();
        }
    }

    /// The placeholder the field shows.
    #[must_use]
    pub fn placeholder(&self) -> &str {
        &self.placeholder
    }

    /// Sets the session workdir listing the mention menu reads. The
    /// shell hands in the active session's listing each frame, so the
    /// menu follows the session.
    pub fn set_listing(&mut self, listing: Option<kage_client::wire::FsListResult>) {
        self.listing = listing;
    }
}

/// The hint line under the card: the key table on the left, the
/// session's cost on the right. The phrasing follows the run state and
/// the Enter-sends preference, as the reference's `hint_line` does.
fn hint_line(ui: &mut egui::Ui, store: &crate::store::Store, running: bool, enter_sends: bool) {
    let palette = crate::theme::Palette::active_of(ui.ctx());
    let chip = |ui: &mut egui::Ui, keys: &str| crate::ui::palette::kbd_chip(ui, keys, palette);
    let word = |ui: &mut egui::Ui, text: &str| {
        ui.label(
            egui::RichText::new(text)
                .size(crate::theme::FS_2XS)
                .color(palette.faint),
        )
    };
    egui::Frame::NONE
        .inner_margin(egui::Margin {
            left: 6,
            right: 6,
            top: 6,
            bottom: 0,
        })
        .show(ui, |ui| {
            ui.set_min_height(16.0);
            ui.horizontal(|ui| {
                if !enter_sends {
                    chip(ui, "Ctrl Enter");
                    word(ui, if running { "queue" } else { "send" });
                    ui.add_space(6.0);
                    chip(ui, "Enter");
                    word(ui, "newline");
                } else if running {
                    let steer = store.state().steer_available();
                    chip(ui, "Enter");
                    word(ui, "queue");
                    if steer {
                        ui.add_space(6.0);
                        chip(ui, "Ctrl Enter");
                        word(ui, "steer");
                    }
                    ui.add_space(6.0);
                    chip(ui, "Esc Esc");
                    word(ui, "interrupt");
                    if !steer {
                        ui.add_space(6.0);
                        word(ui, "steering not advertised");
                    }
                } else {
                    chip(ui, "Enter");
                    word(ui, "send");
                    ui.add_space(6.0);
                    chip(ui, "Shift Enter");
                    word(ui, "newline");
                }
                // The cost at the row's right edge, as the reference's
                // justify-between does.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let cost = store
                        .active_id()
                        .and_then(|id| store.tree_cost(id))
                        .map(|cost| format!("{} {:.4}", cost.currency, cost.amount))
                        .unwrap_or_default();
                    if !cost.is_empty() {
                        word(ui, &cost);
                    }
                });
            });
        });
}

/// What the composer's frame asked the shell for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerEvent {
    /// Nothing.
    None,
    /// Send the draft, or steer the running turn when `steer` is set.
    Send {
        /// The ask came from Ctrl+Enter, so a running turn is steered
        /// rather than queued.
        steer: bool,
    },
    /// Stop the run in flight.
    Stop,
    /// Open the goal dialog.
    OpenGoal,
    /// Confirm swarm mode before it turns on.
    ConfirmSwarm,
    /// Leave the composer for vim normal mode: vim mode's Escape in
    /// the field.
    LeaveInsert,
    /// Open the providers section of settings, from the model picker.
    OpenProviders,
}

#[cfg(test)]
mod surface_tests {
    use super::{Composer, ComposerEvent, Suggest, mention_items, suggestion_rows};

    #[test]
    fn an_empty_draft_sends_nothing() {
        let mut composer = Composer::new();
        assert_eq!(composer.prompt(), None);
        assert_eq!(composer.take_prompt(), None);
    }

    #[test]
    fn the_mention_rows_come_from_the_listing_only() {
        let listing = kage_client::wire::FsListResult {
            entries: vec![
                kage_client::wire::FsEntry {
                    path: "src/main.rs".to_owned(),
                    kind: kage_client::wire::FsKind::File,
                    size: 12,
                },
                kage_client::wire::FsEntry {
                    path: "src/lib.rs".to_owned(),
                    kind: kage_client::wire::FsKind::File,
                    size: 9,
                },
                kage_client::wire::FsEntry {
                    path: "docs".to_owned(),
                    kind: kage_client::wire::FsKind::Directory,
                    size: 0,
                },
            ],
            truncated: false,
        };
        let items = mention_items(Some(&listing), "main");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].path, "src/main.rs");
        assert!(!items[0].directory);
        let dirs = mention_items(Some(&listing), "docs");
        assert_eq!(dirs.len(), 1);
        assert!(dirs[0].directory, "a directory row says so");
        assert!(
            mention_items(Some(&listing), "MAIN").len() == 1,
            "the query is case insensitive"
        );
        assert!(
            mention_items(None, "").is_empty(),
            "no listing yields no rows"
        );
    }

    #[test]
    fn the_suggestion_menu_names_the_slash_and_mention_rows() {
        let commands = vec![serde_json::json!({"name": "review"})];
        let rows = suggestion_rows(
            Some(&Suggest::Slash {
                query: "pl".to_owned(),
            }),
            &commands,
            None,
        )
        .expect("the slash menu shows");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "/plan");
        assert!(
            suggestion_rows(
                Some(&Suggest::Slash {
                    query: "zzz".to_owned(),
                }),
                &commands,
                None,
            )
            .is_none(),
            "no matching command closes the menu"
        );
        let listing = kage_client::wire::FsListResult {
            entries: vec![kage_client::wire::FsEntry {
                path: "src/main.rs".to_owned(),
                kind: kage_client::wire::FsKind::File,
                size: 0,
            }],
            truncated: false,
        };
        let rows = suggestion_rows(
            Some(&Suggest::Mention {
                query: "main".to_owned(),
                at: 4,
            }),
            &commands,
            Some(&listing),
        )
        .expect("the mention menu shows");
        assert_eq!(rows[0].title, "src/main.rs");
        assert_eq!(rows[0].badge.as_deref(), Some("file"));
        let rows = suggestion_rows(
            Some(&Suggest::Mention {
                query: "main".to_owned(),
                at: 4,
            }),
            &commands,
            None,
        )
        .expect("the menu says the listing is coming");
        assert!(rows[0].title.starts_with("no file listing yet"));
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
        assert_eq!(composer.placeholder(), "Ask anything");
        composer.set_placeholder("Ask kage");
        assert_eq!(composer.placeholder(), "Ask kage", "a change re-sets it");
    }

    #[test]
    fn the_send_event_carries_whether_it_steers() {
        assert_eq!(
            ComposerEvent::Send { steer: false },
            ComposerEvent::Send { steer: false }
        );
        assert_ne!(
            ComposerEvent::Send { steer: true },
            ComposerEvent::Send { steer: false }
        );
        assert_ne!(ComposerEvent::Send { steer: false }, ComposerEvent::Stop);
        assert_ne!(ComposerEvent::None, ComposerEvent::Stop);
    }
}
