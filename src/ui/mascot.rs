use eframe::egui::{Color32, CornerRadius, Mesh, Painter, Pos2, Rect, Shape, Stroke, Vec2};
use std::f32::consts::TAU;

const BLINK_PERIOD: f64 = 4.2;

pub fn draw(painter: &Painter, center: Pos2, seconds: f64) {
    let phase = (seconds.rem_euclid(2.4) / 2.4) as f32 * TAU;
    aura(painter, center, phase);

    let lift = 0.3 + phase.sin() * 0.90;
    let swish = phase.cos() * 1.35;
    let p = |x: f32, y: f32| center + Vec2::new(x, y + lift);
    let crown = p(0.0, -4.7);
    let roots = [
        p(-19.5, -5.0),
        p(-12.0, -7.4),
        p(-4.5, -8.2),
        p(3.0, -8.4),
        p(11.0, -7.9),
        p(18.0, -6.0),
        p(20.0, -3.8),
    ];
    let shades = [
        Color32::from_rgb(242, 164, 41),
        Color32::from_rgb(255, 198, 56),
        Color32::from_rgb(255, 225, 120),
        Color32::from_rgb(255, 208, 76),
        Color32::from_rgb(255, 215, 95),
        Color32::from_rgb(238, 160, 42),
    ];
    for (i, (x, y)) in [
        (-25.0, -7.3),
        (-17.0, -10.3),
        (-6.0, -10.4),
        (7.0, -10.1),
        (19.0, -9.3),
        (25.0, -5.9),
    ]
    .into_iter()
    .enumerate()
    {
        painter.add(Shape::convex_polygon(
            vec![crown, roots[i], p(x + swish, y), roots[i + 1]],
            shades[i],
            Stroke::NONE,
        ));
    }
    for (x, y, root_x) in [(-16.5, 0.5, -11.5), (-4.8, -0.8, -0.5), (12.0, -0.2, 15.0)] {
        painter.add(Shape::convex_polygon(
            vec![p(x - 3.3, -6.2), p(root_x, -6.0), p(x + swish, y)],
            Color32::from_rgb(250, 185, 49),
            Stroke::NONE,
        ));
    }

    let height = 1.4 + 7.2 * eye_openness(seconds);
    let look = phase.sin() * 1.15;
    for x in [-10.0, 10.0] {
        let at = p(x + look, 4.8);
        let eye = Rect::from_center_size(at, Vec2::new(5.4, height));
        painter.rect_filled(eye, CornerRadius::same(3), Color32::from_rgb(100, 207, 255));
        if height > 4.0 {
            painter.circle_filled(
                at + Vec2::new(-0.55, -height * 0.22),
                0.75,
                Color32::from_rgb(224, 248, 255),
            );
        }
    }
}

fn eye_openness(seconds: f64) -> f32 {
    let delta = ((seconds + BLINK_PERIOD - 1.65).rem_euclid(BLINK_PERIOD) - 0.105).abs();
    let t = (delta / 0.105).clamp(0.0, 1.0) as f32;
    t * t * (3.0 - 2.0 * t)
}

fn aura(painter: &Painter, center: Pos2, phase: f32) {
    let pulse = (phase.sin() + 1.0) * 0.5;
    let glow_center = center + Vec2::new(0.0, 1.0);
    let mut glow = Mesh::default();
    glow.colored_vertex(
        glow_center,
        Color32::from_rgba_unmultiplied(245, 159, 30, 36 + (pulse * 20.0) as u8),
    );
    const STEPS: usize = 22;
    for i in 0..STEPS {
        let angle = i as f32 / STEPS as f32 * TAU;
        glow.colored_vertex(
            glow_center + Vec2::new(angle.cos() * (44.0 + pulse * 5.0), angle.sin() * 11.0),
            Color32::TRANSPARENT,
        );
        glow.add_triangle(0, 1 + i as u32, 1 + ((i + 1) % STEPS) as u32);
    }
    painter.add(Shape::mesh(glow));

    for side in [-1.0_f32, 1.0] {
        let t = phase + side * 0.6;
        let flare = |x: f32, y: f32| center + Vec2::new(side * x, y);
        let reach = 42.0 + t.sin() * 5.5;
        let rise = -4.0 + t.cos() * 3.2;
        let tint = Color32::from_rgba_unmultiplied(255, 190, 45, 116 + (pulse * 64.0) as u8);
        painter.add(Shape::convex_polygon(
            vec![
                flare(24.0, 5.1),
                flare(30.0, 0.8),
                flare(reach, rise),
                flare(34.0, 3.6),
            ],
            tint,
            Stroke::NONE,
        ));
        painter.line_segment(
            [flare(27.0, 8.2), flare(37.0 + t.cos() * 3.0, 6.6)],
            Stroke::new(1.05, tint),
        );
        for i in 0..2 {
            let drift = ((t / TAU) + 2.0 + i as f32 * 0.46).fract();
            let alpha = (210.0 * (std::f32::consts::PI * drift).sin()) as u8;
            let pos = flare(
                29.0 + drift * (16.0 + i as f32 * 4.0),
                4.0 - drift * (9.0 + i as f32 * 3.0),
            );
            painter.circle_filled(
                pos,
                0.85 + i as f32 * 0.25,
                Color32::from_rgba_unmultiplied(255, 219, 105, alpha),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blink_closes_and_reopens_in_both_cycles() {
        for cycle in [0.0, 4.2] {
            assert_eq!(eye_openness(cycle + 1.5), 1.0);
            assert!(eye_openness(cycle + 1.755) < 0.02);
            assert_eq!(eye_openness(cycle + 2.0), 1.0);
        }
    }
}
