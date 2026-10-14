//! The three-panel shell: the sidebar, the chat column and the
//! workbench, plus the narrow-width overlays.
//!
//! The sidebar takes the left column at [`SIDE_W`], the workbench the
//! right at [`WORKBENCH_W`], and the chat keeps the middle. Both
//! panels are resizable and collapsible: ctrl-\\ toggles the sidebar,
//! ctrl-b the workbench. Under the web client's breakpoints the side
//! panels stop taking a column and float over the content instead
//! (1180px for the workbench, 860px for the sidebar), which is what
//! the old shell did.

use crate::theme::{Palette, SIDE_W, SP_4};

/// The default workbench width.
const WORKBENCH_W: f32 = 460.0;

/// The narrowest the chat pane gets when a side panel is dragged wider.
const CHAT_MIN_W: f32 = 360.0;

/// The web client's breakpoint: at or under it the workbench overlays
/// instead of taking a column.
const MID_W: f32 = 1180.0;

/// The web client's narrow breakpoint: at or under it the sidebar takes
/// no column and floats over the content when shown.
const NARROW_W: f32 = 860.0;

/// The panel layout state the shell keeps.
#[derive(Debug, Clone)]
pub struct Panels {
    /// Whether the sidebar takes its column.
    pub sidebar_visible: bool,
    /// Whether the workbench takes its column.
    pub workbench_visible: bool,
    /// Whether the sidebar floats over the content while narrow.
    pub side_float: bool,
    /// The viewport width, measured each frame. Wide until measured.
    pub viewport_width: f32,
    /// The sidebar's current width, so a re-show keeps the drag.
    pub sidebar_width: f32,
    /// The workbench's current width.
    pub workbench_width: f32,
}

impl Default for Panels {
    fn default() -> Self {
        Self {
            sidebar_visible: true,
            // Closed at first, as the design has it: the workbench is a
            // panel the user asks for with ctrl-b, and opening it by
            // default narrows the transcript on every launch.
            workbench_visible: false,
            side_float: false,
            viewport_width: f32::MAX,
            sidebar_width: SIDE_W,
            workbench_width: WORKBENCH_W,
        }
    }
}

impl Panels {
    /// Measures the viewport, so the overlays follow the window.
    pub fn measure(&mut self, width: f32) {
        self.viewport_width = width;
    }

    /// Toggles the sidebar. Under the narrow breakpoint it floats over
    /// the content instead of taking a column.
    pub fn toggle_sidebar(&mut self) {
        if self.sidebar_floats() {
            self.side_float = !self.side_float;
        } else {
            self.sidebar_visible = !self.sidebar_visible;
        }
    }

    /// Toggles the workbench.
    pub fn toggle_workbench(&mut self) {
        self.workbench_visible = !self.workbench_visible;
    }

    /// Whether the workbench overlays rather than takes a column.
    #[must_use]
    pub fn workbench_overlays(&self) -> bool {
        self.viewport_width <= MID_W
    }

    /// Whether the sidebar floats over the content.
    #[must_use]
    pub fn sidebar_floats(&self) -> bool {
        self.viewport_width <= NARROW_W
    }
}

/// The divider between two columns, its own pixel past the panel.
const DIVIDER_W: f32 = 1.0;

/// Lays out the three panels and calls `chat` for the middle column,
/// `sidebar` for the left panel's contents and `workbench` for the
/// right's. Each callback draws inside the panel's frame.
pub fn show(
    ui: &mut egui::Ui,
    panels: &mut Panels,
    sidebar: impl Fn(&mut egui::Ui),
    chat: impl FnOnce(&mut egui::Ui),
    workbench: impl Fn(&mut egui::Ui),
) {
    panels.measure(ui.available_width());
    let narrow = panels.sidebar_floats();
    let mid = panels.workbench_overlays();

    // Each panel paints its own palette token, the way the old shell
    // drew them: the sidebar the `sidebar` surface, the workbench the
    // `well`, and the chat keeps the app background.
    let palette = Palette::active_of(ui.ctx());
    if panels.sidebar_visible && !narrow {
        // The reference's sidebar keeps its full width and the divider
        // takes its own pixel past it, crisp, where egui would draw a
        // half-pixel line inside the panel.
        let panel = egui::Panel::left("sidebar")
            .resizable(true)
            .show_separator_line(false)
            .default_size(panels.sidebar_width + DIVIDER_W)
            .size_range(CHAT_MIN_W.min(SIDE_W) + DIVIDER_W..=f32::MAX)
            .frame(
                egui::Frame::side_top_panel(ui.style())
                    .fill(palette.sidebar)
                    .stroke(egui::Stroke::NONE)
                    .inner_margin(egui::Margin {
                        left: SP_4 as i8,
                        right: SP_4 as i8 + DIVIDER_W as i8,
                        top: SP_4 as i8,
                        bottom: SP_4 as i8,
                    }),
            )
            .show(ui, |ui| {
                sidebar(ui);
            });
        let rect = panel.response.rect;
        ui.painter().rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(rect.right().round() - DIVIDER_W, rect.top()),
                egui::pos2(rect.right().round(), rect.bottom()),
            ),
            0.0,
            palette.line,
        );
        panels.sidebar_width = (rect.width() - DIVIDER_W).max(CHAT_MIN_W);
    }

    if panels.workbench_visible && !mid {
        let panel = egui::Panel::right("workbench")
            .resizable(true)
            .show_separator_line(false)
            .default_size(panels.workbench_width)
            .size_range(200.0..=f32::MAX)
            .frame(
                egui::Frame::NONE
                    .fill(palette.sidebar)
                    .inner_margin(egui::Margin {
                        left: DIVIDER_W as i8,
                        ..Default::default()
                    }),
            )
            .show(ui, |ui| {
                workbench(ui);
            });
        let rect = panel.response.rect;
        ui.painter().rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(rect.left().round(), rect.top()),
                egui::pos2(rect.left().round() + DIVIDER_W, rect.bottom()),
            ),
            0.0,
            palette.line,
        );
        // Here the divider is the panel's own first pixel, as the
        // reference's handle belongs to the panel after it.
        panels.workbench_width = rect.width().max(200.0);
    }

    // The chat column keeps what is left.
    egui::CentralPanel::default()
        .frame(egui::Frame::central_panel(ui.style()).inner_margin(0))
        .show(ui, |ui| {
            chat(ui);
        });

    // The narrow overlays, over the chat column.
    if narrow && panels.side_float && panels.sidebar_visible {
        let palette = Palette::active_of(ui.ctx());
        let width = panels.sidebar_width;
        let clicked = ui.ctx().input(|i| i.pointer.any_pressed());
        egui::Area::new(egui::Id::new("side-scrim"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(0.0, 0.0))
            .show(ui.ctx(), |ui| {
                let screen = ui.max_rect();
                ui.painter().rect_filled(
                    screen,
                    0.0,
                    egui::Color32::from_black_alpha((0.3 * 255.0) as u8),
                );
                egui::Panel::left("side-float")
                    .resizable(false)
                    .exact_size(width)
                    .frame(egui::Frame::NONE.fill(palette.sidebar))
                    .show(ui, |ui| {
                        sidebar(ui);
                    });
            });
        if clicked {
            panels.side_float = false;
        }
    }

    if mid && panels.workbench_visible {
        let palette = Palette::active_of(ui.ctx());
        let width = panels.workbench_width;
        egui::Area::new(egui::Id::new("workbench-float"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(0.0, 0.0))
            .show(ui.ctx(), |ui| {
                egui::Panel::right("workbench-float-panel")
                    .resizable(false)
                    .exact_size(width)
                    .frame(egui::Frame::NONE.fill(palette.sidebar))
                    .show(ui, |ui| {
                        workbench(ui);
                    });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::{CHAT_MIN_W, MID_W, NARROW_W, Panels, SIDE_W, WORKBENCH_W};

    #[test]
    fn the_defaults_match_the_design() {
        let panels = Panels::default();
        assert!(panels.sidebar_visible);
        assert!(!panels.workbench_visible, "closed until asked for");
        assert_eq!(panels.sidebar_width, SIDE_W);
        assert_eq!(panels.workbench_width, WORKBENCH_W);
    }

    #[test]
    fn the_sidebar_toggles_its_column_while_wide() {
        let mut panels = Panels::default();
        panels.measure(1200.0);
        panels.toggle_sidebar();
        assert!(!panels.sidebar_visible);
        assert!(!panels.side_float, "wide takes no float");
        panels.toggle_sidebar();
        assert!(panels.sidebar_visible);
    }

    #[test]
    fn the_sidebar_floats_while_narrow() {
        let mut panels = Panels::default();
        panels.measure(NARROW_W - 1.0);
        panels.toggle_sidebar();
        assert!(panels.sidebar_visible, "the column stays, it floats over");
        assert!(panels.side_float);
    }

    #[test]
    fn the_workbench_takes_a_column_only_while_wide() {
        let mut panels = Panels::default();
        panels.measure(MID_W + 1.0);
        panels.toggle_workbench();
        assert!(panels.workbench_visible);
        assert!(!panels.workbench_overlays());
        panels.measure(MID_W - 1.0);
        assert!(panels.workbench_overlays());
    }

    /// The design's floor sits above the sidebar's default, so a
    /// dragged-wide sidebar cannot squeeze the chat to nothing, and
    /// below both panels together.
    const _: () = {
        assert!(CHAT_MIN_W > SIDE_W);
        assert!(CHAT_MIN_W < WORKBENCH_W + SIDE_W);
    };
}
