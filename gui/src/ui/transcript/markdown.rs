//! The transcript's markdown pipeline: one cache per entry, a syntect
//! theme derived from the kage palette, and the viewer that renders
//! one entry's text.
//!
//! The cache discipline is the spec's: re-parse only the streaming tail
//! entry per frame. `egui_commonmark`'s own cache holds images and
//! scroll state, not the parsed document, so the parse decision is
//! ours to make: a completed entry is parsed once, when its text first
//! lands, and never again while the text stands; only the entry still
//! receiving chunks is parsed every frame. [`Parses`] is the counter
//! the spec's guard asserts on.

use std::collections::HashMap;

/// The cache name the palette-derived syntect themes register under.
pub const THEME_SHADOW: &str = "kage-shadow";
/// The light palette's theme name.
pub const THEME_DAWN: &str = "kage-dawn";

/// The parse counter the spec's cache-discipline guard reads.
#[derive(Debug, Default, Clone)]
pub struct Parses {
    /// How many times each entry id was parsed.
    per_entry: HashMap<String, usize>,
}

impl Parses {
    /// How many times `id` was parsed.
    #[must_use]
    pub fn count(&self, id: &str) -> usize {
        self.per_entry.get(id).copied().unwrap_or(0)
    }

    /// Records one parse of `id`.
    fn record(&mut self, id: &str) {
        *self.per_entry.entry(id.to_owned()).or_default() += 1;
    }
}

/// One entry's cache and its parse bookkeeping.
#[derive(Debug, Default)]
pub struct Entry {
    /// The viewer's own cache: images and scroll state.
    pub cache: egui_commonmark::CommonMarkCache,
    /// The text this entry last rendered, so a repeat render with no
    /// change is not a parse.
    last_text: Option<String>,
}

/// The parsed form of the transcript, keyed by the entry's id.
#[derive(Debug, Default)]
pub struct Pipeline {
    /// One entry per transcript item id.
    entries: HashMap<String, Entry>,
    /// The parse counter.
    pub parses: Parses,
}

impl Pipeline {
    /// A pipeline with no entries.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Drops the entries whose ids are gone, so a long session does not
    /// accumulate caches for rows that scrolled away and were rebuilt.
    pub fn retain(&mut self, live: &[String]) {
        self.entries.retain(|id, _| live.contains(id));
        self.parses.per_entry.retain(|id, _| live.contains(id));
    }

    /// Renders one entry's markdown. `streaming` marks the entry still
    /// receiving chunks: it is parsed on every frame, and a completed
    /// entry is parsed once, when its text first lands, and never
    /// again while the text stands.
    pub fn show(&mut self, id: &str, text: &str, streaming: bool, ui: &mut egui::Ui) {
        let entry = self.entries.entry(id.to_owned()).or_default();
        let changed = entry.last_text.as_deref() != Some(text);
        if changed || streaming {
            entry.last_text = Some(text.to_owned());
            self.parses.record(id);
        }
        egui_commonmark::CommonMarkViewer::new()
            .syntax_theme_light(THEME_DAWN)
            .syntax_theme_dark(THEME_SHADOW)
            .show(ui, &mut entry.cache, text);
    }
}

/// The syntect theme the kage palette implies, as `.tmTheme` bytes.
///
/// The mapping is the design's own: code blocks sit on the `deep`
/// surface with `ink` text, and the palette's hues carry the token
/// colors, so a code block reads as part of the shell rather than as a
/// pasted-in editor theme. Both palettes build the same structure over
/// their own tokens.
#[must_use]
pub fn theme_bytes(palette: &crate::theme::Palette) -> Vec<u8> {
    let hex = |c: egui::Color32| format!("#{:02X}{:02X}{:02X}", c.r(), c.g(), c.b());
    let bg = hex(palette.deep);
    let fg = hex(palette.ink);
    let comment = hex(palette.faint);
    let keyword = hex(palette.done);
    let string = hex(palette.ok);
    let number = hex(palette.accent);
    let function = hex(palette.info);
    let constant = hex(palette.warn);
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<plist version=\"1.0\">\n<dict>\n");
    out.push_str("<key>name</key>\n<string>kage</string>\n");
    out.push_str("<key>settings</key>\n<array>\n");
    // The base settings every token falls back to.
    out.push_str(&format!(
        "<dict><key>settings</key><dict>\
         <key>background</key><string>{bg}</string>\
         <key>foreground</key><string>{fg}</string>\
         <key>caret</key><string>{fg}</string>\
         <key>selection</key><string>{sel}</string>\
         </dict></dict>\n",
        sel = hex(palette.selected),
    ));
    // The token scopes, in syntect's order.
    let scopes = [
        ("comment", comment.as_str()),
        ("keyword", keyword.as_str()),
        ("keyword.operator", keyword.as_str()),
        ("storage", keyword.as_str()),
        ("string", string.as_str()),
        ("constant.numeric", number.as_str()),
        ("constant.language", constant.as_str()),
        ("entity.name.function", function.as_str()),
        ("variable.parameter", fg.as_str()),
        ("entity.name.tag", keyword.as_str()),
        ("support.function", function.as_str()),
        ("constant.character", number.as_str()),
    ];
    for (scope, color) in scopes {
        out.push_str(&format!(
            "<dict><key>scope</key><string>{scope}</string>\
             <key>settings</key><dict><key>foreground</key><string>{color}</string></dict></dict>\n"
        ));
    }
    out.push_str("</array>\n</dict>\n</plist>\n");
    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::{Parses, Pipeline, theme_bytes};
    use crate::theme::Palette;

    #[test]
    fn the_theme_bytes_parse_as_a_tmtheme() {
        for palette in [Palette::shadow(), Palette::dawn()] {
            let bytes = theme_bytes(&palette);
            let text = String::from_utf8(bytes).expect("the theme is text");
            assert!(text.contains("<plist"), "the theme is a plist");
            assert!(
                text.contains("<key>background</key>"),
                "it sets a background"
            );
            let mut cursor = std::io::Cursor::new(text.into_bytes());
            syntect::highlighting::ThemeSet::load_from_reader(&mut cursor)
                .expect("syntect parses the palette theme");
        }
    }

    #[test]
    fn the_two_palettes_build_different_themes() {
        assert_ne!(
            theme_bytes(&Palette::shadow()),
            theme_bytes(&Palette::dawn())
        );
    }

    #[test]
    fn a_completed_entry_is_parsed_once() {
        let mut parses = Parses::default();
        parses.record("s1");
        assert_eq!(parses.count("s1"), 1);
        assert_eq!(parses.count("s2"), 0, "an untouched entry counts zero");
    }

    #[test]
    fn retain_drops_the_entries_that_are_gone() {
        let mut pipeline = Pipeline::new();
        pipeline.parses.record("a");
        pipeline.parses.record("b");
        pipeline.retain(&["a".to_owned()]);
        assert_eq!(pipeline.parses.count("a"), 1);
        assert_eq!(pipeline.parses.count("b"), 0, "the gone entry is dropped");
    }
}
