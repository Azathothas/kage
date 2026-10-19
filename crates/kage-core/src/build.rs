//! What this build of kage calls itself.
//!
//! A CI build names the commit it was made from, and a nightly names
//! itself instead of a release number: `0.3.1`, `0.3.1 (abc1234)` or
//! `nightly (abc1234)`. Every binary, the CLI, the TUI and the desktop
//! client, reports this one string, on `--version` and over the
//! protocols alike.

/// The version this build reports.
pub const VERSION: &str = env!("KAGE_VERSION");

/// The commit a CI build was made from.
pub const COMMIT: Option<&str> = option_env!("KAGE_BUILD_SHA");

/// Whether this is a nightly build.
pub const NIGHTLY: bool = is_nightly(VERSION);

/// Whether a reported version names a nightly build.
#[must_use]
pub const fn is_nightly(version: &str) -> bool {
    matches!(
        version.as_bytes(),
        [b'n', b'i', b'g', b'h', b't', b'l', b'y', ..]
    )
}

/// The release number a reported version carries: `0.3.1` of
/// `0.3.1 (abc1234)`. `None` for a nightly, which has none, or for an
/// empty report.
#[must_use]
pub fn release(version: &str) -> Option<&str> {
    let number = version.split_whitespace().next()?;
    (!is_nightly(number)).then_some(number)
}

#[cfg(test)]
mod tests {
    use super::{is_nightly, release};

    #[test]
    fn a_release_number_is_read_past_the_commit() {
        assert_eq!(release("0.3.1"), Some("0.3.1"));
        assert_eq!(release("0.3.1 (abc1234)"), Some("0.3.1"));
        assert_eq!(release("nightly (abc1234)"), None);
        assert_eq!(release(""), None);
    }

    #[test]
    fn a_nightly_is_known_by_its_name() {
        assert!(is_nightly("nightly (abc1234)"));
        assert!(is_nightly("nightly"));
        assert!(!is_nightly("0.3.1 (abc1234)"));
        assert!(!is_nightly("night"));
    }
}
