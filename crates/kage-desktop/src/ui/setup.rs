//! The setup screen: what the app shows in place of the chat while it
//! cannot run `kage`.
//!
//! Release builds ship `kage` beside the app and run it. A build
//! without one, such as `cargo install kage-desktop`, spawns the
//! `kage` on the user's machine, so a missing binary gets this screen:
//! point at a `kage` you have, or install one and check again. A chosen binary is probed with
//! `--version` first and refused when it is not `kage` or is older than
//! the minimum the gate checks. The web build runs nothing locally and
//! never shows this screen.

use crate::gate::MINIMUM_KAGE_VERSION;
use crate::theme::{FAMILY_BODY_SEMIBOLD, FAMILY_MONO_REGULAR, FS_SM, FS_XS, Palette, R_LG};
use crate::ui::forms::ShowCrisp as _;
use crate::ui::forms::{self, Tone};

/// The ways to install `kage`, with the command or page each names.
const INSTALL: [(&str, &str); 3] = [
    (
        "Cargo",
        "cargo install --locked --git https://github.com/QaidVoid/kage kage-cli",
    ),
    ("Nix", "nix profile install github:QaidVoid/kage"),
    (
        "Release archive",
        "https://github.com/QaidVoid/kage/releases/latest",
    ),
];

/// What the setup screen asks the shell to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupEvent {
    /// Run the `kage` at this path, which passed the probe.
    Use(String),
    /// Look for `kage` on the PATH again.
    Retry,
    /// Download the newest engine release for this platform.
    Download,
}

/// Probes `program` with `--version`: the version it reports, or why it
/// cannot be used.
#[cfg(not(target_arch = "wasm32"))]
pub fn probe(program: &str) -> Result<String, String> {
    let run = || {
        std::process::Command::new(program)
            .arg("--version")
            .output()
    };
    let mut output = run();
    for _ in 0..4 {
        // A binary written moments ago can still be held open for
        // write by another thread; ETXTBSY clears at once.
        if !matches!(&output, Err(error) if error.raw_os_error() == Some(26)) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        output = run();
    }
    let output = output.map_err(|error| format!("cannot run {program}: {error}"))?;
    let text = String::from_utf8_lossy(&output.stdout);
    let version = text
        .trim()
        .strip_prefix("kage ")
        .ok_or_else(|| format!("{program} is not kage"))?
        .to_owned();
    // A release prints its commit after the version, and a nightly
    // names itself instead of a version; a nightly is never too old.
    let number = version.split_whitespace().next().unwrap_or_default();
    if number != "nightly" && crate::gate::is_below(number, MINIMUM_KAGE_VERSION) {
        return Err(format!(
            "That kage is {version}; this app needs {MINIMUM_KAGE_VERSION} or newer."
        ));
    }
    Ok(version)
}

/// The probe is a local process, which the browser cannot run.
#[cfg(target_arch = "wasm32")]
#[cfg_attr(not(target_arch = "wasm32"), expect(dead_code))]
pub fn probe(program: &str) -> Result<String, String> {
    Err(format!("cannot run {program} in the browser"))
}

/// The setup screen: the path field, what it refused, and the install
/// rows.
#[derive(Debug, Default)]
pub struct SetupView {
    /// The path the user typed.
    path: String,
    /// Why the last chosen path was refused.
    error: Option<String>,
    /// The install commands copied, by index into [`INSTALL`].
    copied: Option<usize>,
    /// What the controls asked for, read once by the shell.
    asked: Option<SetupEvent>,
}

impl SetupView {
    /// A setup screen with an empty path field.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The path the field holds.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Why the last probe refused, when it did.
    #[must_use]
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Probes the typed path and hands it to the shell when it passes.
    /// Returns what the screen asks the shell to do.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn use_path(&mut self) -> Option<SetupEvent> {
        let program = self.path.trim().to_owned();
        if program.is_empty() {
            self.error = Some("Enter the path to a kage binary.".to_owned());
            return None;
        }
        match probe(&program) {
            Ok(_) => {
                self.error = None;
                Some(SetupEvent::Use(program))
            }
            Err(why) => {
                self.error = Some(why);
                None
            }
        }
    }

    /// The browser cannot probe, so nothing is handed to the shell.
    #[cfg(target_arch = "wasm32")]
    pub fn use_path(&mut self) -> Option<SetupEvent> {
        self.error = Some(probe(self.path.trim()).unwrap_err());
        None
    }

    /// Draws the screen. What the controls ask for is read once by
    /// [`SetupView::take_asked`], after the area closes.
    pub fn show(&mut self, ui: &mut egui::Ui, installing: bool) {
        #[cfg(target_arch = "wasm32")]
        let _ = installing;
        let palette = Palette::active_of(ui.ctx());
        let mut asked = None;
        ui.vertical(|ui| {
            ui.set_width(ui.available_width().min(620.0));
            ui.add_space(48.0);
            ui.horizontal(|ui| {
                ui.horizontal(|ui| {
                    crate::ui::mark::mark(ui, 44.0);
                });
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new("kage is not installed")
                            .size(22.0)
                            .family(crate::theme::family(FAMILY_BODY_SEMIBOLD))
                            .color(palette.ink_strong),
                    );
                    ui.label(
                        egui::RichText::new(format!(
                            "This app runs kage rpc from your machine, and no kage was found on your PATH. It needs kage {MINIMUM_KAGE_VERSION} or newer."
                        ))
                        .size(FS_SM)
                        .color(palette.muted),
                    );
                });
            });
            ui.add_space(12.0);
            group(ui, "Use a kage you already have", palette);
            boxed(ui, palette, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let path = &mut self.path;
                    forms::text_field(ui, path, "/path/to/kage", true, palette);
                    if ui.input(|i| i.key_pressed(egui::Key::Enter))
                        && let Some(event) = self.use_path()
                    {
                        asked = Some(event);
                    }
                    let browse = forms::button(ui, "Browse\u{2026}", Tone::Plain, palette);
                    if browse.clicked() {
                        self.browse();
                    }
                    let use_it = forms::button(ui, "Use", Tone::Primary, palette);
                    if use_it.clicked()
                        && let Some(event) = self.use_path()
                    {
                        asked = Some(event);
                    }
                });
                if let Some(error) = self.error.clone() {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        crate::icons::icon(
                            ui,
                            crate::icons::Icon::TriangleAlert,
                            12.0,
                            palette.danger,
                        );
                        ui.label(egui::RichText::new(error).size(FS_XS).color(palette.danger));
                    });
                }
            });
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(asset) = crate::update::cli_asset() {
                group(ui, "Or download it here", palette);
                boxed(ui, palette, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(16.0);
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new(asset)
                                    .size(FS_SM)
                                    .family(crate::theme::family(FAMILY_MONO_REGULAR))
                                    .color(palette.ink),
                            );
                            ui.label(
                                egui::RichText::new(crate::update::install_hint())
                                    .size(FS_XS)
                                    .color(palette.muted),
                            );
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(16.0);
                            let label = if installing {
                                "Downloading\u{2026}"
                            } else {
                                "Download"
                            };
                            let tone = Tone::Primary;
                            if forms::button(ui, label, tone, palette).clicked() && !installing {
                                asked = Some(SetupEvent::Download);
                            }
                        });
                    });
                });
            }
            group(ui, "Or install it yourself", palette);
            boxed(ui, palette, |ui| {
                for (ix, (name, command)) in INSTALL.iter().enumerate() {
                    if ix > 0 {
                        hairline(ui, palette);
                    }
                    ui.horizontal(|ui| {
                        ui.add_space(4.0);
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new(*name)
                                    .size(FS_SM)
                                    .color(palette.ink),
                            );
                            ui.label(
                                egui::RichText::new(*command)
                                    .size(FS_XS)
                                    .family(crate::theme::family(FAMILY_MONO_REGULAR))
                                    .color(palette.muted),
                            );
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(4.0);
                            let copy = forms::icon_button(
                                ui,
                                &format!("setup-copy-{ix}"),
                                crate::icons::Icon::Copy,
                                palette,
                            );
                            if copy.clicked() {
                                ui.ctx().copy_text((*command).to_owned());
                                self.copied = Some(ix);
                            }
                        });
                    });
                }
                hairline(ui, palette);
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("Installed it?")
                                .size(FS_SM)
                                .color(palette.ink),
                        );
                        ui.label(
                            egui::RichText::new("Check your PATH again.")
                                .size(FS_XS)
                                .color(palette.muted),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(4.0);
                        let retry = forms::button(ui, "Check again", Tone::Plain, palette);
                        if retry.clicked() {
                            asked = Some(SetupEvent::Retry);
                        }
                    });
                });
            });
            ui.add_space(16.0);
            ui.label(
                egui::RichText::new(
                    "kage runs as its own program and updates on its own, so this app and kage never have to ship together.",
                )
                .size(FS_XS)
                .color(palette.faint),
            );
        });
        if asked.is_some() {
            self.asked = asked;
        }
    }

    /// What the screen asked for this frame, read once. The shell
    /// rebuilds its transport from it.
    pub fn take_asked(&mut self) -> Option<SetupEvent> {
        self.asked.take()
    }

    /// Opens the platform's file picker and fills the field with the
    /// chosen file. The dialog blocks the UI thread while it shows;
    /// it runs on a thread of its own in the original.
    #[cfg(not(target_arch = "wasm32"))]
    fn browse(&mut self) {
        let picked = rfd::FileDialog::new()
            .set_title("Choose kage")
            .add_filter("Program", &["exe", ""])
            .pick_file();
        if let Some(path) = picked {
            self.path = path.to_string_lossy().into_owned();
            self.error = None;
        }
    }

    /// The browser has no local files to pick from.
    #[cfg(target_arch = "wasm32")]
    fn browse(&mut self) {}
}

/// A group heading above a screen section.
fn group(ui: &mut egui::Ui, text: &str, palette: &Palette) {
    ui.add_space(22.0);
    ui.label(
        egui::RichText::new(text)
            .size(FS_SM)
            .family(crate::theme::family(FAMILY_BODY_SEMIBOLD))
            .color(palette.ink_strong),
    );
    ui.add_space(8.0);
}

/// The rounded box the rows sit in.
fn boxed(ui: &mut egui::Ui, palette: &Palette, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::NONE
        .fill(palette.surface)
        .stroke(egui::Stroke::new(1.0, palette.subtle))
        .corner_radius(egui::CornerRadius::same(R_LG as u8))
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show_crisp(ui, add);
}

/// The hairline between two rows of a box.
fn hairline(ui: &mut egui::Ui, palette: &Palette) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 1.0), egui::Sense::hover());
    ui.painter().line_segment(
        [rect.left_top(), rect.right_top()],
        egui::Stroke::new(1.0, palette.subtle),
    );
    ui.add_space(9.0);
}

#[cfg(test)]
mod tests {
    use super::{SetupView, probe};

    #[test]
    fn a_path_that_does_not_run_is_refused_with_why() {
        let why = probe("/nonexistent/kage").unwrap_err();
        assert!(why.starts_with("cannot run /nonexistent/kage"), "{why}");
        let why = probe("rustc").unwrap_err();
        assert_eq!(why, "rustc is not kage");
    }

    #[test]
    fn an_empty_path_is_refused_without_probing() {
        let mut view = SetupView::new();
        assert_eq!(view.use_path(), None);
        assert_eq!(
            view.error(),
            Some("Enter the path to a kage binary."),
            "the screen says what is missing"
        );
        assert_eq!(view.path(), "");
    }

    #[test]
    fn a_refused_probe_keeps_the_typed_path() {
        let mut view = SetupView::new();
        view.path = "rustc".to_owned();
        assert_eq!(view.use_path(), None);
        assert_eq!(view.error(), Some("rustc is not kage"));
        assert_eq!(view.path(), "rustc", "the user can fix what they typed");
    }

    // The stub is a shell script, which only Unix runs directly.
    #[cfg(unix)]
    #[test]
    fn a_path_that_runs_is_handed_to_the_shell() {
        use super::SetupEvent;
        use std::os::unix::fs::PermissionsExt as _;
        // A stub that answers `--version` as kage does.
        let stub = std::env::temp_dir().join("kage-setup-probe-stub");
        std::fs::write(&stub, "#!/bin/sh\necho 'kage 9.9.9'\n").expect("the stub is written");
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755))
            .expect("the stub runs");
        let mut view = SetupView::new();
        view.path = stub.to_string_lossy().into_owned();
        assert_eq!(view.use_path(), Some(SetupEvent::Use(view.path.clone())));
        assert_eq!(view.error(), None);
        let _ = std::fs::remove_file(&stub);
    }

    #[cfg(unix)]
    #[test]
    fn release_and_nightly_version_lines_pass_the_probe() {
        use std::os::unix::fs::PermissionsExt as _;
        let stub = std::env::temp_dir().join(format!("kage-probe-lines-{}", std::process::id()));
        for (line, accepted) in [
            ("kage 9.9.9 (0123abc)", true),
            ("kage nightly (0123abc)", true),
            ("kage 0.0.1 (0123abc)", false),
        ] {
            std::fs::write(&stub, format!("#!/bin/sh\necho '{line}'\n")).unwrap();
            std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
            let answer = probe(&stub.to_string_lossy());
            assert_eq!(answer.is_ok(), accepted, "{line}: {answer:?}");
        }
        let _ = std::fs::remove_file(&stub);
    }
}
