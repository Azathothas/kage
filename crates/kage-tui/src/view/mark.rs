//! The kage cat for the start card: the nekomata mark as pixel art in
//! half blocks, two pixels to a cell, colored from the theme so it
//! stays a shadow on a dark terminal and a light one alike.

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use crate::theme::Theme;

/// The mark, one row of pixels a line: `b` body, `r` and `q` the
/// lantern's rim light bright and dim, `I` inner ear, `E` eye, `H`
/// the glint in an eye.
const ART: [&str; 14] = [
    ".r............r.",
    ".br..........rr.",
    ".bIr........rIr.",
    ".bIIr......rIIr.",
    ".bIIbbbbrrrbIIr.",
    "bbbbbbbbbbbbbbbr",
    "bbbbbbbbbbbbbbbr",
    "bbbEHbbbbbbEHbbr",
    "bbbEEbbbbbbEEbbq",
    "bbbEEbbbbbbEEbbq",
    "bbbbbbbbbbbbbbbq",
    ".bbbbbbbbbbbbbq.",
    "..bbbbbbbbbbbq..",
    "....bbbbbbbq....",
];

/// Columns the mark spans.
pub(crate) const WIDTH: usize = 16;

/// Terminal rows the mark spans.
#[cfg(test)]
pub(crate) const HEIGHT: usize = ART.len() / 2;

/// The mark's colors for `theme`. The body is the darker of the raised
/// surface and the strongest text, so the cat stays dark on any
/// background; the eyes take the prompt glyph's lantern and the rim
/// blends it into the body.
fn palette(theme: &Theme) -> [(u8, Color); 6] {
    let body = match (theme.user_bg, theme.focus_color) {
        (raised @ Color::Rgb(..), strong @ Color::Rgb(..)) => {
            if luma(strong) < luma(raised) {
                strong
            } else {
                raised
            }
        }
        (raised, _) => raised,
    };
    let eye = theme.input_glyph_fg;
    [
        (b'b', body),
        (b'r', blend(eye, body, 60)),
        (b'q', blend(eye, body, 28)),
        (b'I', blend(eye, body, 22)),
        (b'E', eye),
        (b'H', blend(Color::Rgb(255, 255, 255), eye, 55)),
    ]
}

fn luma(color: Color) -> u32 {
    match color {
        Color::Rgb(r, g, b) => 299 * u32::from(r) + 587 * u32::from(g) + 114 * u32::from(b),
        _ => 0,
    }
}

/// `fg` over `bg` at `percent`, or `fg` when either is not RGB.
fn blend(fg: Color, bg: Color, percent: u16) -> Color {
    let (Color::Rgb(fr, fg_, fb), Color::Rgb(br, bg_, bb)) = (fg, bg) else {
        return fg;
    };
    let mix = |a: u8, b: u8| {
        let v = (u16::from(a) * percent + u16::from(b) * (100 - percent) + 50) / 100;
        u8::try_from(v).unwrap_or(u8::MAX)
    };
    Color::Rgb(mix(fr, br), mix(fg_, bg_), mix(fb, bb))
}

/// The mark as seven lines for `theme`, each led by `indent` blank
/// columns. A cell paints its upper pixel as the upper half block's
/// foreground and its lower one as its background; a cell with one
/// pixel leaves the other half to the terminal.
pub(crate) fn lines(theme: &Theme, indent: usize) -> Vec<Line<'static>> {
    let colors = palette(theme);
    let at = |row: &str, x: usize| {
        let pixel = row.as_bytes()[x];
        colors
            .iter()
            .find(|(c, _)| *c == pixel)
            .map(|(_, color)| *color)
    };
    ART.chunks(2)
        .map(|pair| {
            let mut spans = vec![Span::raw(" ".repeat(indent))];
            for x in 0..WIDTH {
                let cell = match (at(pair[0], x), at(pair[1], x)) {
                    (Some(top), Some(bottom)) => {
                        Span::styled("\u{2580}", Style::default().fg(top).bg(bottom))
                    }
                    (Some(top), None) => Span::styled("\u{2580}", Style::default().fg(top)),
                    (None, Some(bottom)) => Span::styled("\u{2584}", Style::default().fg(bottom)),
                    (None, None) => Span::raw(" "),
                };
                spans.push(cell);
            }
            Line::from(spans)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_art_is_a_whole_number_of_cells() {
        assert_eq!(ART.len() % 2, 0);
        assert!(ART.iter().all(|row| row.len() == WIDTH));
    }

    #[test]
    fn the_mark_spans_its_rows_and_columns() {
        let lines = lines(&Theme::kage_shadow(), 3);
        assert_eq!(lines.len(), HEIGHT);
        assert!(lines.iter().all(|line| line.width() == 3 + WIDTH));
    }

    #[test]
    fn the_cat_stays_dark_with_lantern_eyes() {
        for theme in [Theme::kage_shadow(), Theme::kage_dawn()] {
            let [(_, body), .., (_, eye), _] = palette(&theme);
            let darkest = luma(theme.user_bg).min(luma(theme.focus_color));
            assert_eq!(luma(body), darkest, "{}", theme.name);
            assert_eq!(eye, theme.input_glyph_fg, "{}", theme.name);
        }
    }
}
