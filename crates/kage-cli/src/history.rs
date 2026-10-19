//! On-disk prompt history for the TUI.
//!
//! Each entry is JSON-encoded and stored on its own line in
//! `$XDG_STATE_HOME/kage/history.txt` so multi-line prompts round-trip
//! safely. The TUI seeds [`kage_tui::InputState`] with the loaded
//! entries on startup and appends each submission as it happens; load
//! and append are independent so an interrupted process never corrupts
//! prior history.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use kage_tui::HISTORY_MAX;

/// Default path: `$XDG_STATE_HOME/kage/history.txt`.
pub fn default_path() -> Result<PathBuf, String> {
    Ok(crate::state_root()?.join("history.txt"))
}

/// Read all entries from `path` in chronological order. Missing file
/// returns an empty vector. Malformed lines are skipped silently so a
/// partial write never blocks startup; valid entries before and after
/// the bad line are still loaded.
pub fn load_from(path: &Path) -> Result<Vec<String>, String> {
    let raw = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(format!("history: read {}: {err}", path.display())),
    };
    let mut out: Vec<String> = Vec::new();
    for line in raw.lines() {
        if line.is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<String>(line) {
            out.push(entry);
        }
    }
    if out.len() > HISTORY_MAX {
        let drop = out.len() - HISTORY_MAX;
        out.drain(..drop);
    }
    Ok(out)
}

/// Convenience wrapper using [`default_path`]. A read failure other
/// than a missing file is announced on stderr; the session then
/// starts from an empty history instead of failing startup.
pub fn load() -> Vec<String> {
    match default_path().and_then(|p| load_from(&p)) {
        Ok(entries) => entries,
        Err(err) => {
            eprintln!("kage: {err}; starting from empty history");
            Vec::new()
        }
    }
}

/// Append `entry` to the history file, creating it if needed.
pub fn append_to(path: &Path, entry: &str) -> Result<(), String> {
    if entry.is_empty() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("history: mkdir {}: {err}", parent.display()))?;
    }
    let line = serde_json::to_string(entry).map_err(|e| format!("history: encode: {e}"))?;
    let mut options = fs::OpenOptions::new();
    options.create(true).append(true);
    // The file holds the user's prompts verbatim; it is pinned to
    // 0600 like the credential stores in the sibling directories.
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|err| format!("history: open {}: {err}", path.display()))?;
    // `OpenOptions::mode` applies only when the file is created, so a
    // history file from before the hardening keeps the wider mode it
    // was created with; pin it back on every write.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = file
            .metadata()
            .map_err(|err| format!("history: stat {}: {err}", path.display()))?
            .permissions()
            .mode();
        if mode & 0o777 != 0o600 {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))
                .map_err(|err| format!("history: chmod {}: {err}", path.display()))?;
        }
    }
    writeln!(file, "{line}").map_err(|err| format!("history: write {}: {err}", path.display()))
}

/// Convenience wrapper using [`default_path`].
pub fn append(entry: &str) -> Result<(), String> {
    append_to(&default_path()?, entry)
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn round_trip_through_jsonl_preserves_newlines() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("history.txt");
        append_to(&path, "first").unwrap();
        append_to(&path, "second\nwith\nnewlines").unwrap();
        let loaded = load_from(&path).unwrap();
        assert_eq!(loaded, vec!["first", "second\nwith\nnewlines"]);
    }

    #[test]
    fn missing_file_loads_empty() {
        let dir = tempdir().unwrap();
        let loaded = load_from(&dir.path().join("nope.txt")).unwrap();
        assert!(loaded.is_empty());
    }

    #[test]
    fn malformed_lines_are_skipped() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("history.txt");
        fs::write(&path, "\"good\"\nnot-json\n\"also good\"\n").unwrap();
        let loaded = load_from(&path).unwrap();
        assert_eq!(loaded, vec!["good", "also good"]);
    }

    /// Where the platform has modes: the history file holds prompts
    /// verbatim, so it must not be group- or world-readable.
    #[test]
    #[cfg(unix)]
    fn the_history_file_is_private() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempdir().unwrap();
        let path = dir.path().join("history.txt");
        append_to(&path, "secret prompt").unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    /// A file that predates the hardening at a wider mode is pinned
    /// back to 0600 on the next append.
    #[test]
    #[cfg(unix)]
    fn a_prehardening_history_file_is_pinned_back_to_private() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempdir().unwrap();
        let path = dir.path().join("history.txt");
        fs::write(&path, "\"old\"\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        append_to(&path, "new prompt").unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn load_truncates_to_history_max() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("history.txt");
        for i in 0..(HISTORY_MAX + 10) {
            append_to(&path, &format!("entry{i}")).unwrap();
        }
        let loaded = load_from(&path).unwrap();
        assert_eq!(loaded.len(), HISTORY_MAX);
        assert_eq!(loaded.first().unwrap(), "entry10");
    }

    #[test]
    fn empty_entries_are_not_appended() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("history.txt");
        append_to(&path, "").unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn a_directory_path_is_a_read_error_naming_the_path() {
        let dir = tempdir().unwrap();
        let err = load_from(dir.path()).unwrap_err();
        assert!(err.starts_with("history: read "), "{err}");
        assert!(err.contains(&dir.path().display().to_string()), "{err}");
    }
}
