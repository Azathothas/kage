//! The eclipse mark: kage's disc with a shade across it, and the working
//! indicator when the shade orbits.
//!
//! Painted as one crescent, the disc minus the shade circle, so the
//! mark sits on any background. The orbit turns the shade around the
//! disc's center every 2.2 seconds.
//!
//! The design's mark fills the crescent with the orb gradient. egui
//! fills a path with one flat color, so the gradient is built as a
//! triangle fan with per-vertex colors interpolated along the 135
//! degree axis, which is what the old canvas did with its own
//! gradient.

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

/// The crescent as a mesh with the orb gradient interpolated along the
/// 135 degree axis, which is how the old canvas filled it.
fn gradient_mesh(points: &[(f32, f32)], palette: &Palette, rect: egui::Rect) -> egui::Mesh {
    let scale = rect.width().min(rect.height()) / BOX;
    let to_screen = |(x, y): (f32, f32)| rect.min + egui::vec2(x * scale, y * scale);
    let start = palette.orb_1;
    let end = palette.orb_2;
    let mut mesh = egui::Mesh::default();
    let vertex = |p: (f32, f32), color: egui::Color32| egui::epaint::Vertex {
        pos: to_screen(p),
        uv: egui::epaint::WHITE_UV,
        color,
    };

    // The gradient axis: 135 degrees, the design's diagonal.
    let axis = egui::vec2(-1.0, 1.0);
    let project = |p: (f32, f32)| {
        let v = to_screen(p) - rect.min;
        v.x * axis.x + v.y * axis.y
    };
    let lo = points
        .iter()
        .map(|p| project(*p))
        .fold(f32::INFINITY, f32::min);
    let hi = points
        .iter()
        .map(|p| project(*p))
        .fold(f32::NEG_INFINITY, f32::max);
    let span = (hi - lo).max(f32::EPSILON);
    let mix = |p: (f32, f32)| {
        let t = ((project(p) - lo) / span).clamp(0.0, 1.0);
        egui::Color32::from_rgb(
            (f32::from(start.r()) + (f32::from(end.r()) - f32::from(start.r())) * t) as u8,
            (f32::from(start.g()) + (f32::from(end.g()) - f32::from(start.g())) * t) as u8,
            (f32::from(start.b()) + (f32::from(end.b()) - f32::from(start.b())) * t) as u8,
        )
    };
    let center = {
        let (sx, sy): (f32, f32) = points
            .iter()
            .fold((0.0, 0.0), |(ax, ay), p| (ax + p.0, ay + p.1));
        (sx / points.len() as f32, sy / points.len() as f32)
    };
    mesh.vertices.push(vertex(center, mix(center)));
    let count = points.len();
    for (index, p) in points.iter().enumerate() {
        mesh.vertices.push(vertex(*p, mix(*p)));
        let next = (index + 1) % count;
        mesh.add_triangle(0, (index + 1) as u32, (next + 1) as u32);
    }
    mesh
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
    let turn = turn_of(origin);
    let mark = egui::Rect::from_center_size(rect.center(), egui::vec2(size, size));
    let shade = shade_center(turn);
    let painter = ui.painter();
    if let Some(points) = crescent(shade) {
        painter.add(egui::Shape::mesh(gradient_mesh(&points, palette, mark)));
    }
    // The ring, in the palette's stronger line.
    let ring: Vec<egui::Pos2> = arc(12.0, 12.0, DISC_R, 0.0, TAU, ARC_STEPS)
        .into_iter()
        .map(|(x, y)| {
            let scale = mark.width() / BOX;
            mark.min + egui::vec2(x * scale, y * scale)
        })
        .collect();
    let scale = mark.width() / BOX;
    painter.add(egui::Shape::line(
        ring,
        egui::Stroke::new(RING_W * scale, palette.line_strong),
    ));
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
