//! The opt-in vim layer: normal-mode motions over the transcript rows,
//! folds, find keys, a `:` command line and a modeline.
//!
//! Off by default. Every binding here lives in the `VimNormal` key
//! context, which only exists while vim mode is on: the shell tracks
//! that focus on the chat column only then, so with the mode off no key
//! reaches these actions and nothing here renders.

use crate::theme::ThemeChoice;

/// The action a normal-mode binding dispatches. The key router turns
/// these onto the shell's behaviour; this module only names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Moves the cursor one row down.
    Down,
    /// Moves the cursor one row up.
    Up,
    /// Jumps to the first row.
    Top,
    /// Jumps to the last row.
    Bottom,
    /// Moves the cursor half a page down.
    PageDown,
    /// Moves the cursor half a page up.
    PageUp,
    /// Opens or closes the row under the cursor.
    Toggle,
    /// Opens every row.
    OpenAll,
    /// Closes every row.
    CloseAll,
    /// Focuses the composer.
    Insert,
    /// Opens find.
    Find,
    /// Steps to the next find match.
    Next,
    /// Steps to the previous find match.
    Prev,
    /// Opens the `:` command line.
    Command,
    /// Interrupts the running turn.
    Escape,
    /// Leaves the composer for normal mode.
    LeaveInsert,
}

/// The key context the normal-mode bindings answer in.
pub const NORMAL: &str = "VimNormal";

/// How many rows Ctrl+D and Ctrl+U move.
pub const PAGE: usize = 6;

/// One normal-mode binding: the key, the context it answers in, and
/// the action the router dispatches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    /// The key, as the router names it.
    pub key: &'static str,
    /// The key context the binding answers in.
    pub context: &'static str,
    /// The action the router dispatches.
    pub action: Action,
}

/// The normal-mode bindings, all in the [`NORMAL`] context.
#[must_use]
pub fn bindings() -> Vec<Binding> {
    let ctx = NORMAL;
    vec![
        Binding {
            key: "j",
            context: ctx,
            action: Action::Down,
        },
        Binding {
            key: "down",
            context: ctx,
            action: Action::Down,
        },
        Binding {
            key: "k",
            context: ctx,
            action: Action::Up,
        },
        Binding {
            key: "up",
            context: ctx,
            action: Action::Up,
        },
        Binding {
            key: "g g",
            context: ctx,
            action: Action::Top,
        },
        Binding {
            key: "shift-g",
            context: ctx,
            action: Action::Bottom,
        },
        Binding {
            key: "ctrl-d",
            context: ctx,
            action: Action::PageDown,
        },
        Binding {
            key: "ctrl-u",
            context: ctx,
            action: Action::PageUp,
        },
        Binding {
            key: "z a",
            context: ctx,
            action: Action::Toggle,
        },
        Binding {
            key: "z o",
            context: ctx,
            action: Action::Toggle,
        },
        Binding {
            key: "z c",
            context: ctx,
            action: Action::Toggle,
        },
        Binding {
            key: "enter",
            context: ctx,
            action: Action::Toggle,
        },
        Binding {
            key: "z shift-r",
            context: ctx,
            action: Action::OpenAll,
        },
        Binding {
            key: "z shift-m",
            context: ctx,
            action: Action::CloseAll,
        },
        Binding {
            key: "i",
            context: ctx,
            action: Action::Insert,
        },
        Binding {
            key: "a",
            context: ctx,
            action: Action::Insert,
        },
        Binding {
            key: "/",
            context: ctx,
            action: Action::Find,
        },
        Binding {
            key: "n",
            context: ctx,
            action: Action::Next,
        },
        Binding {
            key: "shift-n",
            context: ctx,
            action: Action::Prev,
        },
        Binding {
            key: "shift-;",
            context: ctx,
            action: Action::Command,
        },
        Binding {
            key: ":",
            context: ctx,
            action: Action::Command,
        },
        Binding {
            key: "shift-:",
            context: ctx,
            action: Action::Command,
        },
        Binding {
            key: "escape",
            context: ctx,
            action: Action::Escape,
        },
    ]
}

/// One `:` command, parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `theme <name>`: switch themes; `None` names no known theme.
    Theme(Option<ThemeChoice>),
    /// `model <name>`: pick the model whose value or name matches.
    Model(String),
    /// `swarm on|off`.
    Swarm(bool),
    /// `plan on|off`.
    Plan(bool),
    /// `goal <text>` sets the goal; `goal` or `goal clear` clears it.
    Goal(Option<String>),
    /// `new`: the welcome pane.
    New,
    /// `settings` or `set`.
    Settings,
    /// `compact`: summarize older turns now.
    Compact,
    /// `noh`: clear the find highlight.
    NoHighlight,
    /// `q`: close the workbench.
    Quit,
    /// `help` or `h`: the keyboard page.
    Help,
    /// `vim on|off`.
    Vim(bool),
    /// An empty line.
    Nothing,
    /// Anything else, by its first word.
    Unknown(String),
}

/// Parses one `:` line.
#[must_use]
pub fn parse(line: &str) -> Command {
    let line = line.trim();
    let (word, arg) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
    let arg = arg.trim();
    let on = arg != "off";
    match word {
        "" => Command::Nothing,
        "theme" => Command::Theme(match arg.to_lowercase().as_str() {
            "" => None,
            "system" => Some(ThemeChoice::System),
            "shadow" | "kage" | "kage shadow" | "dark" => {
                Some(ThemeChoice::Named("kage-shadow".to_owned()))
            }
            "dawn" | "kage dawn" | "light" => Some(ThemeChoice::Named("kage-dawn".to_owned())),
            _ => Some(ThemeChoice::Named(arg.to_owned())),
        }),
        "model" => Command::Model(arg.to_owned()),
        "swarm" => Command::Swarm(on),
        "plan" => Command::Plan(on),
        "goal" => Command::Goal((!arg.is_empty() && arg != "clear").then(|| arg.to_owned())),
        "new" => Command::New,
        "settings" | "set" => Command::Settings,
        "compact" => Command::Compact,
        "noh" | "nohlsearch" => Command::NoHighlight,
        "q" | "quit" => Command::Quit,
        "help" | "h" => Command::Help,
        "vim" => Command::Vim(on),
        other => Command::Unknown(other.to_owned()),
    }
}

/// Where the keys go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Keys move over the transcript.
    Normal,
    /// The composer has the keys.
    Insert,
    /// An approval waits; the digits answer it.
    Approval,
}

impl Mode {
    fn label(self) -> &'static str {
        match self {
            Mode::Normal => "-- NORMAL --",
            Mode::Insert => "-- INSERT --",
            Mode::Approval => "-- APPROVAL --",
        }
    }
}

/// What the modeline shows.
#[derive(Debug, Clone, Default)]
pub struct Modeline {
    /// The mode slot.
    pub mode: Option<Mode>,
    /// The model and thinking level, `model@level`.
    pub model: Option<String>,
    /// Whether swarm mode is on.
    pub swarm: bool,
    /// Whether plan mode is on.
    pub plan: bool,
    /// Todos done and total.
    pub todos: Option<(usize, usize)>,
    /// The session's working directory.
    pub path: Option<String>,
    /// The cursor row and the row total, one based.
    pub cursor: Option<(usize, usize)>,
    /// Context percent, tokens used and the window.
    pub usage: Option<String>,
    /// The session cost.
    pub cost: Option<String>,
    /// The link state.
    pub link: String,
}

/// The cursor row after one motion from `at` over `total` rows. With no
/// cursor yet, moving down starts at the top and moving up at the
/// bottom.
#[must_use]
pub fn step(at: Option<usize>, total: usize, delta: isize) -> Option<usize> {
    if total == 0 {
        return None;
    }
    let last = total - 1;
    let next = match at {
        Some(at) => at.saturating_add_signed(delta).min(last),
        None if delta < 0 => last,
        None => 0,
    };
    Some(next)
}

#[cfg(test)]
mod tests {
    use super::{Action, Command, NORMAL, bindings, parse, step};
    use crate::theme::ThemeChoice;

    /// The action the table answers for `key` while `context` is the
    /// active key context.
    fn action_for(key: &str, context: &str) -> Option<Action> {
        bindings()
            .into_iter()
            .find(|binding| binding.context == context && binding.key == key)
            .map(|binding| binding.action)
    }

    #[test]
    fn the_command_table_parses_like_the_prototype() {
        assert_eq!(
            parse("theme dawn"),
            Command::Theme(Some(ThemeChoice::Named("kage-dawn".into())))
        );
        assert_eq!(
            parse("theme kimi-dark"),
            Command::Theme(Some(ThemeChoice::Named("kimi-dark".into())))
        );
        assert_eq!(parse("theme"), Command::Theme(None));
        assert_eq!(parse("swarm off"), Command::Swarm(false));
        assert_eq!(parse("swarm"), Command::Swarm(true));
        assert_eq!(parse("plan on"), Command::Plan(true));
        assert_eq!(
            parse("goal tests pass"),
            Command::Goal(Some("tests pass".into()))
        );
        assert_eq!(parse("goal clear"), Command::Goal(None));
        assert_eq!(parse("  "), Command::Nothing);
        assert_eq!(parse("q"), Command::Quit);
        assert_eq!(parse("wq"), Command::Unknown("wq".into()));
    }

    #[test]
    fn motions_clamp_and_start_from_the_ends() {
        assert_eq!(step(None, 5, 1), Some(0));
        assert_eq!(step(None, 5, -1), Some(4));
        assert_eq!(step(Some(4), 5, 6), Some(4));
        assert_eq!(step(Some(2), 5, -6), Some(0));
        assert_eq!(step(Some(0), 0, 1), None);
    }

    #[test]
    fn with_vim_off_no_binding_answers_outside_the_normal_context() {
        assert!(
            bindings().iter().all(|binding| binding.context == NORMAL),
            "every binding waits for the vim context, which only exists with the mode on"
        );
        assert_eq!(action_for("j", ""), None);
        assert_eq!(action_for(":", ""), None);
    }

    #[test]
    fn with_vim_on_the_colon_key_opens_the_line_and_q_quits() {
        assert_eq!(action_for(":", NORMAL), Some(Action::Command));
        assert_eq!(action_for("shift-;", NORMAL), Some(Action::Command));
        assert_eq!(parse("q"), Command::Quit);
        assert_eq!(parse(""), Command::Nothing);
    }
}
