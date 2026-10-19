//! The session mode helpers the store and the composer share.
//!
//! The mode is one select config option, with plan mode sitting apart
//! from the permission modes. The store reads it to decide whether
//! plan mode is on; the composer that draws the chips returns with
//! E05 and is what cycles through the values.

use kage_client::Session;
use kage_client::wire::{SessionConfigKind, SessionConfigOption, SessionConfigSelectOption};

/// The select config option `id` of a session, when the agent
/// advertised it as a select.
#[must_use]
pub(crate) fn select_option<'a>(session: &'a Session, id: &str) -> Option<&'a SessionConfigOption> {
    session
        .config_options
        .iter()
        .find(|option| option.id == id && option.kind == SessionConfigKind::Select)
}

/// The mode id that is active now: what `current_mode_update` last
/// marked, else what the mode option reports.
#[must_use]
pub(crate) fn active_mode(session: &Session) -> Option<String> {
    session
        .mode
        .clone()
        .or_else(|| select_option(session, "mode").map(|option| option.current_value.clone()))
}

/// The value of the mode option that is plan mode rather than a
/// permission mode.
pub(crate) const PLAN_MODE: &str = "plan";

/// The permission modes the mode option advertises, in its order: every
/// value but plan mode, which the toolbar shows as its own chip.
#[must_use]
#[allow(dead_code)]
pub(crate) fn permission_values(session: &Session) -> Vec<&SessionConfigSelectOption> {
    select_option(session, "mode")
        .map(|option| {
            option
                .options
                .iter()
                .filter(|value| value.value != PLAN_MODE)
                .collect()
        })
        .unwrap_or_default()
}

/// The permission mode Shift+Tab cycles to from `current`: the next
/// advertised one, wrapping around. No option or an empty list cycles
/// nowhere. The composer that cycles lives in E05.
#[must_use]
#[allow(dead_code)]
pub(crate) fn next_mode_value(session: &Session, current: &str) -> Option<String> {
    let values = permission_values(session);
    if values.is_empty() {
        return None;
    }
    let index = values
        .iter()
        .position(|value| value.value == current)
        .map_or(0, |index| (index + 1) % values.len());
    Some(values[index].value.clone())
}

#[cfg(test)]
mod tests {
    use kage_client::Session;
    use kage_client::wire::{SessionConfigKind, SessionConfigOption, SessionConfigSelectOption};

    use super::{active_mode, next_mode_value, select_option};

    fn mode_session(current: &str, marked: Option<&str>, values: &[&str]) -> Session {
        let mut session = Session::new("s1");
        session.config_options = vec![SessionConfigOption {
            id: "mode".into(),
            name: "Mode".into(),
            description: None,
            category: None,
            kind: SessionConfigKind::Select,
            current_value: current.into(),
            options: values
                .iter()
                .map(|value| SessionConfigSelectOption {
                    value: (*value).into(),
                    name: (*value).into(),
                    description: None,
                })
                .collect(),
        }];
        session.mode = marked.map(str::to_owned);
        session
    }

    #[test]
    fn mode_cycles_exactly_the_advertised_values() {
        let values = ["default", "ask", "allow", "deny", "plan"];
        let session = mode_session("default", None, &values);
        assert_eq!(
            next_mode_value(&session, "default").as_deref(),
            Some("ask"),
            "the cycle follows the option's order"
        );
        assert_eq!(
            next_mode_value(&session, "deny").as_deref(),
            Some("default"),
            "plan mode is not a permission mode"
        );
        assert_eq!(
            next_mode_value(&session, "mystery").as_deref(),
            Some("default"),
            "an unknown current starts the cycle"
        );
        let session = mode_session("ask", Some("deny"), &values);
        let bare = Session::new("s1");
        assert_eq!(
            next_mode_value(&bare, "default"),
            None,
            "no option, no cycle"
        );
        let empty = mode_session("default", None, &[]);
        assert_eq!(
            next_mode_value(&empty, "default"),
            None,
            "no values, no cycle"
        );
        assert_eq!(
            active_mode(&session),
            Some("deny".to_owned()),
            "the mark wins over the option value"
        );
        assert!(select_option(&session, "mode").is_some());
        assert!(select_option(&session, "goal").is_none());
    }
}
