//! The eclipse mark: kage's disc with a shade across it, and the working
//! indicator when the shade orbits.
//!
//! Painted as one crescent, the disc minus the shade circle, so the
//! mark sits on any background. The orbit turns the shade around the
//! disc's center every 2.2 seconds.
//!
//! The crescent and its ring render as SVG through the image loader,
//! so the edges are antialiased the way the reference's paths are, and
//! the orb gradient runs as the reference's shader runs it. The orbit
//! steps through a fixed set of positions, so each one rasterizes once.

use std::f32::consts::TAU;
use std::time::Duration;

use web_time::Instant;

use crate::theme::Palette;

/// The design draws the mark in a 24-unit box.
const BOX: f32 = 24.0;
/// The disc radius.
const DISC_R: f32 = 10.0;
/// The shade radius.
const SHADE_R: f32 = 8.5;
/// Where the shade sits at rest: up and to the right of center.
const SHADE_AT: (f32, f32) = (16.5, 9.0);
/// One orbit of the shade.
const ORBIT: Duration = Duration::from_millis(2200);
/// The ring stroke, in box units.
const RING_W: f32 = 0.8;
/// How many segments the disc's arcs are drawn with.
const ARC_STEPS: usize = 48;

/// The shade's center after `turn` of a full orbit, in box units.
#[must_use]
pub fn shade_center(turn: f32) -> (f32, f32) {
    let (dx, dy) = (SHADE_AT.0 - 12.0, SHADE_AT.1 - 12.0);
    let angle = turn * TAU;
    let (sin, cos) = angle.sin_cos();
    (12.0 + dx * cos - dy * sin, 12.0 + dx * sin + dy * cos)
}

/// The two points where the disc and the shade circles cross, in box
/// units, ordered so the disc's visible arc runs from the first to the
/// second the long way round.
#[must_use]
pub fn crossings(shade: (f32, f32)) -> Option<((f32, f32), (f32, f32))> {
    let (cx, cy) = (12.0, 12.0);
    let (dx, dy) = (shade.0 - cx, shade.1 - cy);
    let d = (dx * dx + dy * dy).sqrt();
    if d >= DISC_R + SHADE_R || d <= (DISC_R - SHADE_R).abs() || d == 0.0 {
        return None;
    }
    let a = (DISC_R * DISC_R - SHADE_R * SHADE_R + d * d) / (2.0 * d);
    let h = (DISC_R * DISC_R - a * a).max(0.0).sqrt();
    let (mx, my) = (cx + a * dx / d, cy + a * dy / d);
    let first = (mx + h * dy / d, my - h * dx / d);
    let second = (mx - h * dy / d, my + h * dx / d);
    Some((first, second))
}

/// The orbit turn of `origin`, or `0.0` at rest.
#[must_use]
pub fn turn_of(origin: Option<Instant>) -> f32 {
    origin.map_or(0.0, |origin| {
        let t = origin.elapsed().as_secs_f32() % ORBIT.as_secs_f32();
        t / ORBIT.as_secs_f32()
    })
}

/// The points of a circle arc from `from` to `to`, both angles in
/// radians, in `steps` segments.
fn arc(cx: f32, cy: f32, r: f32, from: f32, to: f32, steps: usize) -> Vec<(f32, f32)> {
    let mut points = Vec::with_capacity(steps + 1);
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        let angle = from + (to - from) * t;
        points.push((cx + r * angle.cos(), cy + r * angle.sin()));
    }
    points
}

/// The crescent's outline in box units: the disc's arc outside the
/// shade, then the shade's arc inside the disc back to the start.
fn crescent(shade: (f32, f32)) -> Option<Vec<(f32, f32)>> {
    let (first, second) = crossings(shade)?;
    let angle = |(x, y): (f32, f32), (cx, cy): (f32, f32)| (y - cy).atan2(x - cx);
    let (a0, a1) = (angle(first, (12.0, 12.0)), angle(second, (12.0, 12.0)));
    let mut span = a1 - a0;
    while span <= 0.0 {
        span += TAU;
    }
    let mut points = arc(12.0, 12.0, DISC_R, a0, a0 + span, ARC_STEPS);
    let (b0, b1) = (angle(second, shade), angle(first, shade));
    let mut span = b1 - b0;
    while span <= 0.0 {
        span += TAU;
    }
    points.extend(arc(shade.0, shade.1, SHADE_R, b0, b0 + span, ARC_STEPS));
    Some(points)
}

/// How many positions one orbit steps through.
const ORBIT_STEPS: u32 = 120;

/// The mark as SVG in its 24-unit box, the shade `turn` of the way
/// round: the crescent on the orb gradient, then the ring.
fn svg(turn: f32, palette: &Palette) -> String {
    use crate::theme::svg_color;

    let shade = shade_center(turn);
    let mut body = String::new();
    if let (Some((first, second)), Some(points)) = (crossings(shade), crescent(shade)) {
        // The reference's gradient: along 135 degrees over the path's
        // bounds, the short side stretched to the long one.
        let (x0, y0, x1, y1) = points.iter().fold(
            (
                f32::INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
            ),
            |(a, b, c, d), p| (a.min(p.0), b.min(p.1), c.max(p.0), d.max(p.1)),
        );
        let (w, h) = (x1 - x0, y1 - y0);
        let (mut dx, mut dy) = (
            std::f32::consts::FRAC_1_SQRT_2,
            std::f32::consts::FRAC_1_SQRT_2,
        );
        if w > h {
            dy *= h / w;
        } else {
            dx *= w / h;
        }
        let long = if dx.abs() > dy.abs() { w } else { h };
        let len = (dx * dx + dy * dy).sqrt();
        let (nx, ny) = (dx / len * long / 2.0, dy / len * long / 2.0);
        let (cx, cy) = (x0 + w / 2.0, y0 + h / 2.0);
        body = format!(
            concat!(
                "<defs><linearGradient id=\"g\" gradientUnits=\"userSpaceOnUse\" ",
                "x1=\"{:.3}\" y1=\"{:.3}\" x2=\"{:.3}\" y2=\"{:.3}\">",
                "<stop offset=\"0\" {}/><stop offset=\"1\" {}/></linearGradient></defs>",
                "<path d=\"M{:.3} {:.3} A{DISC_R} {DISC_R} 0 1 0 {:.3} {:.3} ",
                "A{SHADE_R} {SHADE_R} 0 0 1 {:.3} {:.3} Z\" fill=\"url(#g)\"/>"
            ),
            cx - nx,
            cy - ny,
            cx + nx,
            cy + ny,
            svg_color("stop-color", "stop-opacity", palette.orb_1),
            svg_color("stop-color", "stop-opacity", palette.orb_2),
            first.0,
            first.1,
            second.0,
            second.1,
            first.0,
            first.1,
            DISC_R = DISC_R,
            SHADE_R = SHADE_R,
        );
    }
    format!(
        concat!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{box}\" height=\"{box}\" viewBox=\"0 0 {box} {box}\">",
            "{}<circle cx=\"12\" cy=\"12\" r=\"{}\" fill=\"none\" stroke-width=\"{}\" {}/></svg>"
        ),
        body,
        DISC_R,
        RING_W,
        svg_color("stroke", "stroke-opacity", palette.line_strong),
        box = BOX,
    )
}

/// The mark at `size` pixels in `rect`. With `orbit` the shade turns on
/// the clock `origin` started.
pub fn paint(
    ui: &egui::Ui,
    rect: egui::Rect,
    size: f32,
    origin: Option<Instant>,
    palette: &Palette,
) {
    use crate::icons::PaintSnapped as _;
    use std::hash::{Hash as _, Hasher as _};

    let step = (turn_of(origin) * ORBIT_STEPS as f32).round() as u32 % ORBIT_STEPS;
    let svg = svg(step as f32 / ORBIT_STEPS as f32, palette);
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    svg.hash(&mut hasher);
    let uri = format!("bytes://kage-eclipse-{:016x}.svg", hasher.finish());
    let mark = egui::Rect::from_center_size(rect.center(), egui::vec2(size, size));
    egui::Image::from_bytes(uri, svg.into_bytes())
        .fit_to_exact_size(mark.size())
        .paint_snapped(ui, mark);
}

/// The mark at `size` pixels, drawn with the active palette. With
/// `orbit` the shade turns on the clock `origin` started, asking for
/// frames while it does.
pub fn eclipse(ui: &mut egui::Ui, size: f32, origin: Option<Instant>) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let palette = Palette::active_of(ui.ctx());
    paint(ui, rect, size, origin, palette);
}

#[cfg(test)]
mod tests {
    use super::{DISC_R, SHADE_AT, SHADE_R, crossings, shade_center, turn_of};

    #[test]
    fn the_shade_rests_where_the_design_draws_it_and_orbits_the_center() {
        let rest = shade_center(0.0);
        assert!((rest.0 - SHADE_AT.0).abs() < 1e-4 && (rest.1 - SHADE_AT.1).abs() < 1e-4);
        let half = shade_center(0.5);
        assert!((half.0 - (24.0 - SHADE_AT.0)).abs() < 1e-3, "{half:?}");
        assert!((half.1 - (24.0 - SHADE_AT.1)).abs() < 1e-3, "{half:?}");
    }

    #[test]
    fn the_crossings_lie_on_both_circles() {
        let shade = shade_center(0.25);
        let (a, b) = crossings(shade).expect("the circles cross");
        for (x, y) in [a, b] {
            let disc = ((x - 12.0).powi(2) + (y - 12.0).powi(2)).sqrt();
            let shaded = ((x - shade.0).powi(2) + (y - shade.1).powi(2)).sqrt();
            assert!((disc - DISC_R).abs() < 1e-3);
            assert!((shaded - SHADE_R).abs() < 1e-3);
        }
    }

    #[test]
    fn a_shade_that_misses_the_disc_has_no_crossings() {
        // A shade centered on the disc center leaves no crescent: the
        // circles are concentric.
        assert!(crossings((12.0, 12.0)).is_none());
    }

    #[test]
    fn the_orbit_turn_wraps_inside_one_period() {
        assert_eq!(turn_of(None), 0.0);
        let origin = web_time::Instant::now();
        let turn = turn_of(Some(origin));
        assert!((0.0..1.0).contains(&turn));
    }
}
