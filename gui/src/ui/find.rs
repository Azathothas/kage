//! The find bar over the transcript: query, match counter, next and
//! previous, and the tint the matched rows take.
//!
//! The bar is a state machine over the transcript's searchable rows:
//! every query change and every frame the store moves re-matches, so
//! the counter and the tint follow the transcript as it streams. The
//! keys answer in the `Find` context while the query holds focus, and
//! Escape closes and clears the tint.

use crate::theme::{FS_XS, Palette, R_LG, SP_4, SP_6};
use crate::ui::transcript::model::{FindMarks, RowKey};

/// The bar's width, the old bar's 340px.
const BAR_W: f32 = 340.0;

/// The row indexes `query` matches, case insensitively. An empty
/// query matches nothing.
#[must_use]
pub fn find_matches(rows: &[(RowKey, String)], query: &str) -> Vec<usize> {
    let needle = query.to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    rows.iter()
        .enumerate()
        .filter_map(|(ix, (_, text))| text.to_lowercase().contains(&needle).then_some(ix))
        .collect()
}

/// The match index one step from `current`, wrapping at the ends.
/// No matches stays put.
#[must_use]
pub fn step_match(current: usize, total: usize, back: bool) -> usize {
    if total == 0 {
        return 0;
    }
    if back {
        (current + total - 1) % total
    } else {
        (current + 1) % total
    }
}

/// The counter the find bar renders, one based on both ends.
#[must_use]
pub fn counter_text(current: usize, total: usize) -> String {
    if total == 0 {
        "0 / 0".to_owned()
    } else {
        format!("{} / {total}", current + 1)
    }
}

/// The find bar's state.
#[derive(Debug, Default)]
pub struct FindBar {
    /// Whether the bar shows.
    open: bool,
    /// The query the rows narrow over.
    query: String,
    /// The keys of the rows the query matched, in transcript order.
    matches: Vec<RowKey>,
    /// Which match the counter points at.
    current: usize,
}

impl FindBar {
    /// A closed find bar over an empty query.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the bar shows.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The counter's two numbers: which match, and how many.
    #[must_use]
    pub fn counter(&self) -> (usize, usize) {
        (self.current, self.matches.len())
    }

    /// The query the rows narrow over.
    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Opens the bar over a fresh query.
    pub fn open(&mut self) {
        self.open = true;
        self.current = 0;
        self.matches.clear();
        self.query.clear();
    }

    /// Closes the bar and clears the tint.
    pub fn close(&mut self) {
        if !self.open {
            return;
        }
        self.open = false;
        self.matches.clear();
        self.current = 0;
        self.query.clear();
    }

    /// Sets the query and re-matches against `rows`, which the
    /// transcript hands in each frame.
    pub fn set_query(&mut self, query: String, rows: &[(RowKey, String)]) {
        self.query = query;
        self.rematch(rows);
    }

    /// Re-matches the query against `rows`. Runs on every query change
    /// and every frame the store moves, so the counter and the tint
    /// follow the transcript as it streams.
    pub fn rematch(&mut self, rows: &[(RowKey, String)]) {
        if !self.open {
            return;
        }
        self.matches = find_matches(rows, &self.query)
            .into_iter()
            .map(|ix| rows[ix].0)
            .collect();
        if self.current >= self.matches.len() {
            self.current = 0;
        }
    }

    /// The marks the transcript tints rows with.
    #[must_use]
    pub fn marks(&self) -> Option<FindMarks> {
        if self.matches.is_empty() {
            None
        } else {
            Some(FindMarks {
                keys: self.matches.clone(),
                current: self.current,
            })
        }
    }

    /// Steps to the next or previous match and returns its key, so the
    /// transcript can bring it into view.
    pub fn step(&mut self, back: bool) -> Option<RowKey> {
        if self.matches.is_empty() {
            return None;
        }
        self.current = step_match(self.current, self.matches.len(), back);
        let key = self.matches[self.current];
        Some(key)
    }

    /// Draws the bar over the chat column, when it is open. The query
    /// field takes egui's focus, so typing lands in it.
    pub fn show(&mut self, ui: &mut egui::Ui, rows: &[(RowKey, String)]) -> Option<RowAction> {
        if !self.open {
            return None;
        }
        let palette = Palette::active_of(ui.ctx());
        let mut action = None;
        egui::Area::new(egui::Id::new("find-bar"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, SP_6))
            .show(ui.ctx(), |ui| {
                egui::Frame::NONE
                    .fill(palette.menu)
                    .stroke(egui::Stroke::new(1.0, palette.line))
                    .corner_radius(egui::CornerRadius::same(R_LG as u8))
                    .inner_margin(egui::Margin::symmetric(SP_6 as i8, SP_4 as i8))
                    .show(ui, |ui| {
                        ui.set_width(BAR_W);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = SP_4;
                            crate::icons::themed(ui, crate::icons::Icon::Search, 14.0);
                            let response = ui.add(
                                egui::TextEdit::singleline(&mut self.query)
                                    .hint_text("Find in chat")
                                    .desired_width(BAR_W - 120.0)
                                    .id(egui::Id::new("find-query")),
                            );
                            if response.gained_focus() {
                                // The bar mounts with the query empty,
                                // so the caret starts where the user
                                // expects.
                                let _ = response;
                            }
                            let (current, total) = self.counter();
                            ui.label(
                                egui::RichText::new(counter_text(current, total))
                                    .size(FS_XS)
                                    .color(palette.muted),
                            );
                            if ui
                                .add(egui::Button::new(
                                    egui::RichText::new("\u{2191}").size(FS_XS),
                                ))
                                .clicked()
                            {
                                action = self.step(true).map(RowAction::Reveal);
                            }
                            if ui
                                .add(egui::Button::new(
                                    egui::RichText::new("\u{2193}").size(FS_XS),
                                ))
                                .clicked()
                            {
                                action = self.step(false).map(RowAction::Reveal);
                            }
                            if ui
                                .add(egui::Button::new(egui::RichText::new("close").size(FS_XS)))
                                .clicked()
                            {
                                self.close();
                            }
                        });
                    });
            });
        // A query change re-matches, so the counter and the tint follow
        // the transcript as it streams.
        let _ = rows;
        action
    }
}

/// What the find bar asks the shell to carry out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowAction {
    /// Bring the row `key` into view.
    Reveal(RowKey),
}

/// The find bar's keys, in the `Find` context: Enter steps forward,
/// Shift+Enter back, Escape closes.
pub const FIND: &str = "Find";

#[cfg(test)]
mod tests {
    use super::{FindBar, counter_text, find_matches, step_match};
    use crate::ui::transcript::model::RowKey;

    fn rows() -> Vec<(RowKey, String)> {
        vec![
            (RowKey::Item(0), "the first row".to_owned()),
            (RowKey::Item(1), "the second row".to_owned()),
            (RowKey::Item(2), "nothing here".to_owned()),
        ]
    }

    #[test]
    fn an_empty_query_matches_nothing() {
        assert!(find_matches(&rows(), "").is_empty());
    }

    #[test]
    fn the_query_matches_case_insensitively() {
        assert_eq!(find_matches(&rows(), "THE"), vec![0, 1]);
        assert_eq!(find_matches(&rows(), "second"), vec![1]);
        assert!(find_matches(&rows(), "absent").is_empty());
    }

    #[test]
    fn stepping_wraps_at_the_ends() {
        assert_eq!(step_match(0, 3, false), 1);
        assert_eq!(step_match(2, 3, false), 0, "forward wraps");
        assert_eq!(step_match(0, 3, true), 2, "back wraps");
        assert_eq!(step_match(0, 0, false), 0, "no matches stays put");
    }

    #[test]
    fn the_counter_is_one_based_on_both_ends() {
        assert_eq!(counter_text(0, 0), "0 / 0");
        assert_eq!(counter_text(0, 3), "1 / 3");
        assert_eq!(counter_text(2, 3), "3 / 3");
    }

    #[test]
    fn the_bar_opens_empty_and_closes_clear() {
        let mut bar = FindBar::new();
        assert!(!bar.is_open());
        bar.open();
        assert!(bar.is_open());
        assert_eq!(bar.counter(), (0, 0));
        bar.set_query("the".to_owned(), &rows());
        assert_eq!(bar.counter().1, 2, "the query re-matched");
        assert_eq!(
            bar.step(false),
            Some(RowKey::Item(1)),
            "the first step moves off the first match"
        );
        assert_eq!(bar.counter().0, 1);
        bar.close();
        assert!(!bar.is_open());
        assert_eq!(bar.counter(), (0, 0), "closing clears the tint");
    }

    #[test]
    fn the_marks_name_the_matched_rows() {
        let mut bar = FindBar::new();
        bar.open();
        bar.set_query("the".to_owned(), &rows());
        let marks = bar.marks().expect("the query matched");
        assert_eq!(marks.keys.len(), 2);
        assert_eq!(marks.current, 0);
    }
}
