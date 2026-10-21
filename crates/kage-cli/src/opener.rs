//! Handing a URL or a file to the desktop's default application.

use std::ffi::OsStr;
use std::process::Stdio;

/// The program that opens a URL or file with its default application:
/// `open` on macOS, `explorer` on Windows, `xdg-open` when X11 or
/// Wayland is attached, and `None` when no display is attached. The
/// display variables are parameters so the choice stays testable.
pub(crate) fn program(display: Option<&OsStr>, wayland: Option<&OsStr>) -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        return Some("open");
    }
    if cfg!(windows) {
        return Some("explorer");
    }
    if display.is_some() || wayland.is_some() {
        return Some("xdg-open");
    }
    None
}

/// Opens `target` with the default application, without waiting for
/// it and without letting it touch the terminal.
///
/// # Errors
///
/// No display is attached, or the opener cannot be started.
pub(crate) fn open(target: &OsStr) -> Result<(), String> {
    let program = program(
        std::env::var_os("DISPLAY").as_deref(),
        std::env::var_os("WAYLAND_DISPLAY").as_deref(),
    )
    .ok_or("no display is attached")?;
    let mut child = std::process::Command::new(program)
        .arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{program}: {e}"))?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::program;

    #[test]
    fn an_opener_needs_macos_windows_or_a_display() {
        if cfg!(target_os = "macos") || cfg!(windows) {
            assert!(program(None, None).is_some());
        } else {
            assert_eq!(program(None, None), None);
            assert_eq!(program(Some(OsStr::new(":0")), None), Some("xdg-open"));
            assert_eq!(
                program(None, Some(OsStr::new("wayland-0"))),
                Some("xdg-open")
            );
        }
    }
}
