use eframe::egui::{Color32, CornerRadius, Mesh, Painter, Pos2, Rect, Shape, Stroke, Vec2};
use glacier::config::MascotForm;
use std::f32::consts::{PI, TAU};

const FLOAT_PERIOD: f64 = 2.8;
const BLINK_PERIOD: f64 = 4.2;
const FLASH_PERIOD: f64 = 3.6;

// Normal / Toad Sage Minato palette.
const HAIR_BACK: Color32 = Color32::from_rgb(212, 148, 30);
const HAIR_MAIN: Color32 = Color32::from_rgb(250, 210, 82);
const HAIR_TIP: Color32 = Color32::from_rgb(255, 234, 142);
const HAIR_EDGE: Color32 = Color32::from_rgb(178, 116, 24);

const SKIN_LIGHT: Color32 = Color32::from_rgb(246, 214, 184);
const SKIN_SHADE: Color32 = Color32::from_rgb(220, 169, 134);
const SKIN_JAW: Color32 = Color32::from_rgb(148, 98, 76);

const ROBE_WHITE: Color32 = Color32::from_rgb(244, 247, 254);
const ROBE_SHADE: Color32 = Color32::from_rgb(198, 208, 226);
const ROBE_EDGE: Color32 = Color32::from_rgb(138, 152, 178);

const BAND_NAVY: Color32 = Color32::from_rgb(24, 40, 82);
const PLATE_STEEL: Color32 = Color32::from_rgb(192, 206, 224);
const PLATE_HI: Color32 = Color32::from_rgb(238, 246, 255);
const PLATE_ETCH: Color32 = Color32::from_rgb(62, 76, 102);

const FLAME_CRIMSON: Color32 = Color32::from_rgb(224, 38, 42);
const FLAME_ORANGE: Color32 = Color32::from_rgb(255, 112, 38);

const EYE_LASH: Color32 = Color32::from_rgb(22, 28, 46);
const EYE_SCLERA: Color32 = Color32::from_rgb(248, 251, 255);
const EYE_BLUE: Color32 = Color32::from_rgb(24, 136, 246);
const EYE_SKY: Color32 = Color32::from_rgb(106, 218, 255);
const EYE_PUPIL: Color32 = Color32::from_rgb(14, 42, 106);

const SAGE_PIGMENT: Color32 = Color32::from_rgb(234, 72, 32);
const SAGE_PIGMENT_DEEP: Color32 = Color32::from_rgb(198, 48, 24);
const SAGE_GOLD_OUTER: Color32 = Color32::from_rgb(252, 174, 24);
const SAGE_GOLD_CORE: Color32 = Color32::from_rgb(255, 226, 74);
const SAGE_PUPIL: Color32 = Color32::from_rgb(22, 14, 8);

// Kurama Chakra Mode (KCM) blazing golden-yellow & jet-black seal palette.
const KCM_HAIR_BACK: Color32 = Color32::from_rgb(242, 136, 12);
const KCM_HAIR_MAIN: Color32 = Color32::from_rgb(255, 214, 48);
const KCM_HAIR_TIP: Color32 = Color32::from_rgb(255, 246, 166);
const KCM_HAIR_EDGE: Color32 = Color32::from_rgb(196, 88, 10);

const KCM_FACE: Color32 = Color32::from_rgb(255, 234, 126);
const KCM_FACE_SHADE: Color32 = Color32::from_rgb(244, 164, 26);
const KCM_FACE_JAW: Color32 = Color32::from_rgb(164, 72, 10);

const KCM_CLOAK_GOLD: Color32 = Color32::from_rgb(255, 206, 40);
const KCM_CLOAK_SHADE: Color32 = Color32::from_rgb(240, 126, 14);
const KCM_INK: Color32 = Color32::from_rgb(18, 14, 12);

const KCM_EYE_OUTER: Color32 = Color32::from_rgb(226, 48, 20);
const KCM_EYE_CORE: Color32 = Color32::from_rgb(255, 146, 26);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Sage,
    Kurama,
}

impl From<MascotForm> for Mode {
    fn from(form: MascotForm) -> Self {
        match form {
            MascotForm::Kurama => Mode::Kurama,
            MascotForm::Normal => Mode::Normal,
            MascotForm::Sage => Mode::Sage,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MascotOptions {
    pub mode: Mode,
    pub show_aura: bool,
    pub animate: bool,
    pub blink: bool,
}

impl Default for MascotOptions {
    fn default() -> Self {
        Self {
            mode: Mode::Kurama,
            show_aura: true,
            animate: true,
            blink: true,
        }
    }
}

#[allow(dead_code)]
pub fn draw(painter: &Painter, center: Pos2, seconds: f64) {
    draw_mode(painter, center, seconds, Mode::Kurama);
}

#[allow(dead_code)]
pub fn draw_mode(painter: &Painter, center: Pos2, seconds: f64, mode: Mode) {
    draw_with_options(
        painter,
        center,
        seconds,
        MascotOptions {
            mode,
            ..MascotOptions::default()
        },
    );
}

pub fn draw_with_options(painter: &Painter, center: Pos2, seconds: f64, options: MascotOptions) {
    let mode = options.mode;
    let motion_seconds = if options.animate { seconds } else { 0.0 };
    let phase = (motion_seconds.rem_euclid(FLOAT_PERIOD) / FLOAT_PERIOD) as f32 * TAU;
    let breath = if options.animate { phase.sin() } else { 0.0 };
    let sway = if options.animate {
        (phase - 0.55).sin()
    } else {
        0.0
    };
    let flash = if options.animate {
        flash_pulse(motion_seconds)
    } else {
        0.0
    };

    let lift = breath * 0.50;
    let hair_sway = sway * 0.85;
    let bang_sway = if options.animate {
        (phase - 0.85).sin() * 0.65
    } else {
        0.0
    };

    if options.show_aura {
        atmospheric_backdrop(painter, center, phase, flash, mode);
        if mode == Mode::Kurama {
            kcm_flame_wisps(painter, center, lift, phase, flash);
        }
    }
    back_hair(painter, center, lift, hair_sway, mode);
    hokage_robe_collar(painter, center, lift, flash, mode);
    anime_head(painter, center, lift, mode);
    anime_eyes(
        painter,
        center,
        lift,
        seconds,
        phase,
        mode,
        options.animate,
        options.blink,
    );
    headband(painter, center, lift, flash, mode);
    front_hair_and_bangs(painter, center, lift, hair_sway, bang_sway, mode);
    flying_raijin_kunai(
        painter,
        center,
        phase,
        flash,
        mode,
        options.show_aura,
        options.animate,
    );
}

fn poly(painter: &Painter, points: Vec<Pos2>, fill: Color32, stroke: Stroke) {
    painter.add(Shape::convex_polygon(points, fill, stroke));
}

fn line(painter: &Painter, points: Vec<Pos2>, stroke: Stroke) {
    painter.add(Shape::line(points, stroke));
}

fn radial_glow(painter: &Painter, center: Pos2, rx: f32, ry: f32, inner: Color32, steps: usize) {
    let mut mesh = Mesh::default();
    mesh.colored_vertex(center, inner);
    for i in 0..steps {
        let angle = i as f32 / steps as f32 * TAU;
        mesh.colored_vertex(
            center + Vec2::new(angle.cos() * rx, angle.sin() * ry),
            Color32::TRANSPARENT,
        );
        mesh.add_triangle(0, 1 + i as u32, 1 + ((i + 1) % steps) as u32);
    }
    painter.add(Shape::mesh(mesh));
}

fn eye_openness(seconds: f64) -> f32 {
    let shifted = seconds + BLINK_PERIOD - 1.65;
    let local = shifted.rem_euclid(BLINK_PERIOD);
    let primary = blink_dip(local, 0.135, 0.045, 0.095);
    // Occasional sharp double-blink every third cycle.
    let cycle = shifted.div_euclid(BLINK_PERIOD) as i64;
    let secondary = if cycle.rem_euclid(3) == 2 {
        blink_dip(local, 0.56, 0.035, 0.08)
    } else {
        1.0
    };
    primary.min(secondary)
}

fn blink_dip(local: f64, center: f64, hold: f64, ramp: f64) -> f32 {
    let delta = ((local - center).abs() - hold).max(0.0);
    let t = (delta / ramp).clamp(0.0, 1.0) as f32;
    t * t * (3.0 - 2.0 * t)
}

fn flash_pulse(seconds: f64) -> f32 {
    let local = seconds.rem_euclid(FLASH_PERIOD);
    let peak = 2.55;
    let half = 0.28;
    let dist = ((local - peak).abs() / half).clamp(0.0, 1.0) as f32;
    let smooth = 1.0 - dist * dist * (3.0 - 2.0 * dist);
    smooth * smooth
}

fn atmospheric_backdrop(painter: &Painter, center: Pos2, phase: f32, flash: f32, mode: Mode) {
    let pulse = (phase.sin() + 1.0) * 0.5;
    let kurama = mode == Mode::Kurama;
    let sage = mode == Mode::Sage;

    // Warm dark-gold radial atmosphere blooming gently behind and beneath the
    // floating head, leaving clear dark glowing space above the bottom horizon.
    let boost = if kurama {
        36.0
    } else if sage {
        14.0
    } else {
        0.0
    };
    let glow_alpha = (58.0 + boost + pulse * 16.0 + flash * 26.0).min(148.0) as u8;
    radial_glow(
        painter,
        center + Vec2::new(0.0, 4.0),
        (if kurama { 84.0 } else { 78.0 }) + pulse * 5.0 + flash * 8.0,
        12.2,
        Color32::from_rgba_unmultiplied(
            if kurama {
                252
            } else if sage {
                222
            } else {
                198
            },
            if kurama {
                164
            } else if sage {
                132
            } else {
                118
            },
            if kurama { 24 } else { 22 },
            glow_alpha,
        ),
        28,
    );

    // Rich pulsing Nine-Tails golden-orange chakra halo closely hugging the
    // floating KCM head (or subtle Sage halo in Toad Sage Mode).
    if kurama {
        radial_glow(
            painter,
            center + Vec2::new(0.0, 0.2),
            34.0 + pulse * 3.2 + flash * 3.8,
            14.2,
            Color32::from_rgba_unmultiplied(
                255,
                204,
                38,
                (74.0 + pulse * 24.0 + flash * 36.0).min(145.0) as u8,
            ),
            24,
        );
        radial_glow(
            painter,
            center + Vec2::new(0.0, 0.0),
            24.5 + pulse * 2.0,
            12.5,
            Color32::from_rgba_unmultiplied(
                255,
                238,
                118,
                (52.0 + pulse * 18.0 + flash * 28.0).min(110.0) as u8,
            ),
            20,
        );
    } else if sage {
        radial_glow(
            painter,
            center + Vec2::new(0.0, 0.2),
            30.0 + pulse * 2.8,
            13.8,
            Color32::from_rgba_unmultiplied(
                255,
                168,
                36,
                (28.0 + pulse * 14.0 + flash * 22.0) as u8,
            ),
            20,
        );
    }

    // Subtle warm under-glow floating directly beneath the Hokage robe collar.
    radial_glow(
        painter,
        center + Vec2::new(0.0, 10.6),
        27.0,
        5.0,
        Color32::from_rgba_unmultiplied(
            248,
            if kurama { 148 } else { 118 },
            26,
            (if kurama { 58.0 } else { 38.0 } + flash * 26.0) as u8,
        ),
        16,
    );

    // Sleek thin golden horizon light-beam along the bottom edge of the pill.
    let hy = center.y + 15.2;
    let half_w = 88.0;
    let beam_h = 1.35;
    let peak_alpha =
        ((if kurama { 124.0 } else { 96.0 }) + pulse * 26.0 + flash * 48.0).min(210.0) as u8;
    let beam_center = Color32::from_rgba_unmultiplied(
        255,
        if kurama { 196 } else { 162 },
        if kurama { 54 } else { 46 },
        peak_alpha,
    );
    let beam_mid = Color32::from_rgba_unmultiplied(226, 138, 24, peak_alpha / 3);
    let mut beam = Mesh::default();
    let xs = [-half_w, -34.0, 0.0, 34.0, half_w];
    let cols = [
        Color32::TRANSPARENT,
        beam_mid,
        beam_center,
        beam_mid,
        Color32::TRANSPARENT,
    ];
    for (x, c) in xs.into_iter().zip(cols) {
        beam.colored_vertex(Pos2::new(center.x + x, hy - beam_h), Color32::TRANSPARENT);
        beam.colored_vertex(Pos2::new(center.x + x, hy), c);
        beam.colored_vertex(Pos2::new(center.x + x, hy + beam_h), Color32::TRANSPARENT);
    }
    for seg in 0..4_u32 {
        let a = seg * 3;
        let b = (seg + 1) * 3;
        beam.add_triangle(a, b, a + 1);
        beam.add_triangle(b, b + 1, a + 1);
        beam.add_triangle(a + 1, b + 1, a + 2);
        beam.add_triangle(b + 1, b + 2, a + 2);
    }
    painter.add(Shape::mesh(beam));

    // Soft blurred golden chakra embers / bokeh drifting gently in the dark wings.
    let bokeh: [(f32, f32, f32, f32, f32); 6] = [
        (-76.0, 5.5, 3.4, 0.0, 0.72),
        (-52.0, -4.8, 4.0, 1.3, 0.85),
        (-38.0, 7.5, 2.5, 2.6, 0.65),
        (52.0, -6.2, 3.0, 0.8, 0.70),
        (66.0, 3.4, 4.1, 2.1, 0.88),
        (82.0, 1.5, 2.4, 3.5, 0.62),
    ];
    for (bx, by, radius, offset, intensity) in bokeh {
        let t = phase + offset;
        let drift_x = t.cos() * 2.4;
        let drift_y = t.sin() * 1.8 - if kurama { 0.8 } else { 0.0 };
        let shimmer = 0.55 + 0.45 * (t * 0.5 + offset).sin().abs();
        let base_a = if kurama { 82.0 } else { 58.0 };
        let alpha = ((base_a + flash * 28.0) * intensity * shimmer).min(138.0) as u8;
        let pos = Pos2::new(
            center.x + bx + drift_x,
            (center.y + by + drift_y).clamp(center.y - 14.0 + radius, center.y + 15.5 - radius),
        );
        radial_glow(
            painter,
            pos,
            radius,
            radius,
            Color32::from_rgba_unmultiplied(
                255,
                if kurama { 208 } else { 186 },
                if kurama { 64 } else { 76 },
                alpha,
            ),
            14,
        );
    }
}

fn kcm_flame_wisps(painter: &Painter, center: Pos2, lift: f32, phase: f32, flash: f32) {
    let p = |x: f32, y: f32| center + Vec2::new(x, y + lift);
    let flicker = (phase * 2.0).sin();
    let flicker2 = (phase * 2.0 + 1.4).cos();
    let outer_alpha = (118.0 + (flicker * 0.5 + 0.5) * 48.0 + flash * 50.0).min(220.0) as u8;
    let wisp_gold = Color32::from_rgba_unmultiplied(255, 214, 52, outer_alpha);
    let wisp_amber = Color32::from_rgba_unmultiplied(255, 138, 20, outer_alpha.saturating_sub(24));

    // Flickering golden Kurama chakra flame wisps rising off Minato's spikes.
    let wisps = [
        (
            vec![
                p(-20.0, -7.8),
                p(-29.2 + flicker * 1.4, -12.0 + flicker2 * 0.8),
                p(-18.0, -11.4),
            ],
            wisp_amber,
        ),
        (
            vec![
                p(-17.8, 2.8),
                p(-27.4 + flicker2 * 1.4, -0.6 + flicker * 0.9),
                p(-17.0, -2.2),
            ],
            wisp_gold,
        ),
        (
            vec![
                p(18.0, -11.4),
                p(29.4 + flicker2 * 1.4, -11.6 + flicker * 0.8),
                p(20.0, -7.5),
            ],
            wisp_amber,
        ),
        (
            vec![
                p(16.8, -2.0),
                p(26.8 + flicker * 1.2, 0.4 + flicker2 * 0.8),
                p(17.2, 3.2),
            ],
            wisp_gold,
        ),
    ];
    for (pts, color) in wisps {
        poly(painter, pts, color, Stroke::NONE);
    }
}

fn back_hair(painter: &Painter, center: Pos2, lift: f32, sway: f32, mode: Mode) {
    let p = |x: f32, y: f32| center + Vec2::new(x, y + lift);
    let fill = if mode == Mode::Kurama {
        KCM_HAIR_BACK
    } else {
        HAIR_BACK
    };

    // Sweeping back spikes framing the floating head.
    let spikes = [
        vec![
            p(-10.5, -11.4),
            p(-26.5 + sway * 0.9, -9.2),
            p(-16.2 + sway * 0.3, -5.2),
            p(-10.8, -6.8),
        ],
        vec![
            p(-12.6, -5.8),
            p(-24.5 + sway, -1.5),
            p(-15.2 + sway * 0.4, 2.2),
            p(-11.8, 0.0),
        ],
        vec![
            p(-12.2, 1.4),
            p(-20.0 + sway * 0.8, 5.8),
            p(-12.8, 7.2),
            p(-11.0, 3.8),
        ],
        vec![
            p(10.5, -11.4),
            p(10.8, -6.8),
            p(16.2 + sway * 0.3, -5.2),
            p(26.8 + sway * 0.9, -8.8),
        ],
        vec![
            p(12.6, -5.6),
            p(11.8, 0.0),
            p(15.2 + sway * 0.4, 2.3),
            p(24.8 + sway, -1.1),
        ],
        vec![
            p(12.2, 1.6),
            p(11.0, 3.8),
            p(12.8, 7.2),
            p(20.2 + sway * 0.8, 6.0),
        ],
    ];
    for pts in spikes {
        poly(painter, pts, fill, Stroke::NONE);
    }
}

fn hokage_robe_collar(painter: &Painter, center: Pos2, lift: f32, flash: f32, mode: Mode) {
    let p = |x: f32, y: f32| center + Vec2::new(x, y + lift);

    if mode == Mode::Kurama {
        // Blazing golden Fourth Hokage KCM chakra cloak collar wrapped snugly
        // under the chin (head-only floating silhouette) accented with bold
        // jet-black Kurama collar trim and iconic black tomoe / magatama beads.
        poly(
            painter,
            vec![
                p(-14.2, 4.6),
                p(14.2, 4.6),
                p(14.8, 8.9),
                p(8.6, 11.9),
                p(0.0, 13.1),
                p(-8.6, 11.9),
                p(-14.8, 8.9),
            ],
            KCM_CLOAK_SHADE,
            Stroke::NONE,
        );

        // Luminous golden-yellow KCM collar lapels hugging the jawline.
        poly(
            painter,
            vec![
                p(-14.4, 4.8),
                p(-9.2, 7.2),
                p(0.0, 9.8),
                p(0.0, 12.9),
                p(-8.4, 11.7),
                p(-14.2, 8.9),
            ],
            KCM_CLOAK_GOLD,
            Stroke::NONE,
        );
        poly(
            painter,
            vec![
                p(9.2, 7.2),
                p(14.4, 4.8),
                p(14.2, 8.9),
                p(8.4, 11.7),
                p(0.0, 12.9),
                p(0.0, 9.8),
            ],
            KCM_CLOAK_GOLD,
            Stroke::NONE,
        );

        // Fiery molten-orange chakra flame accents dancing along the golden KCM collar
        // (keeping the luminous gold bright and clean behind the jet-black tomoe beads).
        let flames = [
            (p(-14.0, 9.0), p(-11.1, 7.0), p(-8.4, 11.7)),
            (p(-8.6, 11.7), p(-5.5, 9.6), p(-2.0, 12.7)),
            (p(2.0, 12.7), p(5.5, 9.6), p(8.6, 11.7)),
            (p(8.4, 11.7), p(11.1, 7.0), p(14.0, 9.0)),
        ];
        for (a, peak, b) in flames {
            poly(painter, vec![a, peak, b], FLAME_ORANGE, Stroke::NONE);
        }

        // Sleek jet-black Kurama seal trim along the outer collar wings + 3 bold,
        // cleanly spaced jet-black magatama/tomoe beads curving under the chin so
        // bright gold shines unmistakably between each tomoe even at 1x.
        line(
            painter,
            vec![p(-14.3, 5.0), p(-14.1, 8.9)],
            Stroke::new(1.25, KCM_INK),
        );
        line(
            painter,
            vec![p(14.3, 5.0), p(14.1, 8.9)],
            Stroke::new(1.25, KCM_INK),
        );
        line(
            painter,
            vec![
                p(-11.0, 9.1),
                p(-5.8, 10.7),
                p(0.0, 11.4),
                p(5.8, 10.7),
                p(11.0, 9.1),
            ],
            Stroke::new(0.8, KCM_INK),
        );
        for (tx, ty) in [(-7.8, 10.2), (0.0, 11.35), (7.8, 10.2)] {
            painter.circle_filled(p(tx, ty), 1.35, KCM_INK);
        }

        // Crisp glowing solar-gold outer rim separating the KCM collar from the dark backdrop.
        line(
            painter,
            vec![
                p(-14.2, 8.9),
                p(-8.4, 11.8),
                p(0.0, 13.0),
                p(8.4, 11.8),
                p(14.2, 8.9),
            ],
            Stroke::new(
                0.75,
                Color32::from_rgba_unmultiplied(
                    255,
                    244,
                    156,
                    (175.0 + flash * 75.0).min(250.0) as u8,
                ),
            ),
        );
        return;
    }

    // Iconic Fourth Hokage white robe high-collar wrapped snugly around the base
    // of Minato's floating chin (Normal / Toad Sage modes).
    poly(
        painter,
        vec![
            p(-14.0, 4.6),
            p(14.0, 4.6),
            p(14.6, 8.9),
            p(8.6, 11.8),
            p(0.0, 13.0),
            p(-8.6, 11.8),
            p(-14.6, 8.9),
        ],
        ROBE_SHADE,
        Stroke::NONE,
    );

    poly(
        painter,
        vec![
            p(-14.4, 4.8),
            p(-9.2, 7.2),
            p(0.0, 9.8),
            p(0.0, 12.9),
            p(-8.4, 11.7),
            p(-14.2, 8.9),
        ],
        ROBE_WHITE,
        Stroke::NONE,
    );
    poly(
        painter,
        vec![
            p(9.2, 7.2),
            p(14.4, 4.8),
            p(14.2, 8.9),
            p(8.4, 11.7),
            p(0.0, 12.9),
            p(0.0, 9.8),
        ],
        ROBE_WHITE,
        Stroke::NONE,
    );

    let flames = [
        (p(-13.9, 9.0), p(-10.9, 6.9), p(-8.3, 11.7)),
        (p(-8.6, 11.7), p(-5.4, 9.3), p(-1.8, 12.7)),
        (p(1.8, 12.7), p(5.4, 9.3), p(8.6, 11.7)),
        (p(8.3, 11.7), p(10.9, 6.9), p(13.9, 9.0)),
    ];
    for (a, peak, b) in flames {
        poly(painter, vec![a, peak, b], FLAME_CRIMSON, Stroke::NONE);
        let mid_base = Pos2::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5);
        let inner_peak = Pos2::new(
            peak.x + (mid_base.x - peak.x) * 0.32,
            peak.y + (mid_base.y - peak.y) * 0.32,
        );
        let a_in = Pos2::new(a.x + (b.x - a.x) * 0.22, a.y + (b.y - a.y) * 0.22);
        let b_in = Pos2::new(b.x + (a.x - b.x) * 0.22, b.y + (a.y - b.y) * 0.22);
        poly(
            painter,
            vec![a_in, inner_peak, b_in],
            FLAME_ORANGE,
            Stroke::NONE,
        );
    }

    line(
        painter,
        vec![p(-3.2, 9.95), p(0.0, 10.55), p(3.2, 9.95)],
        Stroke::new(1.05, FLAME_CRIMSON),
    );
    painter.circle_filled(
        p(0.0, 10.45),
        1.1 + flash * 0.15,
        Color32::from_rgb(248, 86, 48),
    );

    line(
        painter,
        vec![
            p(-14.2, 8.9),
            p(-8.4, 11.8),
            p(0.0, 13.0),
            p(8.4, 11.8),
            p(14.2, 8.9),
        ],
        Stroke::new(0.7, ROBE_EDGE),
    );
}

fn anime_head(painter: &Painter, center: Pos2, lift: f32, mode: Mode) {
    let p = |x: f32, y: f32| center + Vec2::new(x, y + lift);
    let kurama = mode == Mode::Kurama;
    let base = if kurama { KCM_FACE } else { SKIN_LIGHT };
    let shade = if kurama { KCM_FACE_SHADE } else { SKIN_SHADE };
    let jaw = if kurama { KCM_FACE_JAW } else { SKIN_JAW };

    // Cute, clean, V-tapered floating anime head (radiant golden chakra in KCM,
    // fair warm anime skin in Normal / Toad Sage).
    poly(
        painter,
        vec![
            p(-12.5, -6.2),
            p(12.5, -6.2),
            p(12.8, -0.6),
            p(11.5, 4.0),
            p(7.6, 7.7),
            p(0.0, 9.7),
            p(-7.6, 7.7),
            p(-11.5, 4.0),
            p(-12.8, -0.6),
        ],
        base,
        Stroke::NONE,
    );

    // Sleek cel-shaded brow shadow right beneath the Konoha headband.
    poly(
        painter,
        vec![p(-12.5, -5.8), p(12.5, -5.8), p(12.5, -3.5), p(-12.5, -3.5)],
        shade,
        Stroke::NONE,
    );

    // Subtle inner-bang cheek shadows for crisp hair-to-face separation.
    poly(
        painter,
        vec![p(-12.5, -3.5), p(-10.2, -3.5), p(-9.7, 5.1), p(-11.5, 4.0)],
        Color32::from_rgba_unmultiplied(shade.r(), shade.g(), shade.b(), 155),
        Stroke::NONE,
    );
    poly(
        painter,
        vec![p(10.2, -3.5), p(12.5, -3.5), p(11.5, 4.0), p(9.7, 5.1)],
        Color32::from_rgba_unmultiplied(shade.r(), shade.g(), shade.b(), 155),
        Stroke::NONE,
    );

    if !kurama {
        for side in [-1.0_f32, 1.0] {
            painter.circle_filled(
                p(side * 8.2, 4.2),
                1.45,
                Color32::from_rgba_unmultiplied(244, 134, 126, 46),
            );
        }
    } else {
        // Subtle luminous KCM cheek glow accents.
        for side in [-1.0_f32, 1.0] {
            painter.circle_filled(
                p(side * 8.1, 4.2),
                1.4,
                Color32::from_rgba_unmultiplied(255, 162, 24, 76),
            );
        }
    }

    // Crisp V-tapered jawline/chin contour.
    line(
        painter,
        vec![
            p(-11.5, 4.0),
            p(-7.6, 7.7),
            p(0.0, 9.7),
            p(7.6, 7.7),
            p(11.5, 4.0),
        ],
        Stroke::new(1.1, jaw),
    );

    // Confident Minato smile.
    let smile_color = if kurama {
        Color32::from_rgb(132, 50, 10)
    } else {
        Color32::from_rgb(154, 88, 76)
    };
    line(
        painter,
        vec![p(-2.5, 6.05), p(0.0, 6.75), p(2.6, 5.80)],
        Stroke::new(1.05, smile_color),
    );
}

#[allow(clippy::too_many_arguments)]
fn anime_eyes(
    painter: &Painter,
    center: Pos2,
    lift: f32,
    seconds: f64,
    phase: f32,
    mode: Mode,
    animate: bool,
    blink: bool,
) {
    let p = |x: f32, y: f32| center + Vec2::new(x, y + lift);
    let open = if blink { eye_openness(seconds) } else { 1.0 };
    let look = if animate {
        (phase * 0.5).sin() * 0.30
    } else {
        0.0
    };
    let squint = if animate {
        1.0 - (phase.cos().max(0.0) * 0.06)
    } else {
        1.0
    };

    for side in [-1.0_f32, 1.0] {
        let ex = side * 6.0;
        let ey = 1.35;
        let outer_x = ex + side * 3.55;
        let inner_x = ex - side * 3.00;
        let top_mid_x = ex + side * 0.40;
        let bot_mid_x = ex - side * 0.15;

        if mode == Mode::Sage {
            // Iconic Perfect Toad Sage Mode vermilion/orange-red pigmentation
            // encircling the eye and sweeping sharply up into pointed outer wings.
            let pigment = if side < 0.0 {
                vec![
                    p(outer_x + side * 2.05, ey - 3.65),
                    p(top_mid_x, ey - 3.15),
                    p(inner_x - side * 0.60, ey + 0.55),
                    p(bot_mid_x, ey + 3.40),
                    p(outer_x + side * 0.90, ey + 2.00),
                ]
            } else {
                vec![
                    p(inner_x - side * 0.60, ey + 0.55),
                    p(top_mid_x, ey - 3.15),
                    p(outer_x + side * 2.05, ey - 3.65),
                    p(outer_x + side * 0.90, ey + 2.00),
                    p(bot_mid_x, ey + 3.40),
                ]
            };
            poly(painter, pigment, SAGE_PIGMENT, Stroke::NONE);
            line(
                painter,
                vec![
                    p(inner_x - side * 0.25, ey + 0.20),
                    p(top_mid_x, ey - 2.75),
                    p(outer_x + side * 1.55, ey - 3.25),
                ],
                Stroke::new(0.95, SAGE_PIGMENT_DEEP),
            );
        }

        // Sleek eyebrow tucked neatly under the headband shadow.
        let brow_color = if mode == Mode::Kurama {
            KCM_INK
        } else {
            Color32::from_rgb(164, 110, 26)
        };
        line(
            painter,
            vec![
                p(ex - side * 2.75, -2.30),
                p(ex + side * 0.25, -2.95),
                p(ex + side * 3.00, -2.60),
            ],
            Stroke::new(1.05, brow_color),
        );

        let lash_color = if mode == Mode::Kurama {
            KCM_INK
        } else {
            EYE_LASH
        };

        if open < 0.14 {
            line(
                painter,
                vec![
                    p(ex - side * 2.9, ey + 0.95),
                    p(ex + side * 0.15, ey + 0.30),
                    p(ex + side * 3.3, ey + 0.10),
                ],
                Stroke::new(1.55, lash_color),
            );
            continue;
        }

        let h = open * squint;
        let sclera_color = if mode == Mode::Kurama {
            Color32::from_rgb(255, 252, 232)
        } else {
            EYE_SCLERA
        };

        let sclera = if side < 0.0 {
            vec![
                p(outer_x, ey - 2.30 * h),
                p(top_mid_x, ey - 1.90 * h),
                p(inner_x, ey + 0.60 * h),
                p(bot_mid_x, ey + 2.35 * h),
                p(ex + side * 2.65, ey + 1.25 * h),
            ]
        } else {
            vec![
                p(inner_x, ey + 0.60 * h),
                p(top_mid_x, ey - 1.90 * h),
                p(outer_x, ey - 2.30 * h),
                p(ex + side * 2.65, ey + 1.25 * h),
                p(bot_mid_x, ey + 2.35 * h),
            ]
        };
        poly(painter, sclera, sclera_color, Stroke::NONE);

        let ix = ex + look - side * 0.20;
        let iy = ey + 0.15;
        match mode {
            Mode::Kurama => {
                // Fierce Kurama fox eyes: blazing crimson-orange irises with crisp
                // vertical jet-black fox-slit pupils (`|`).
                let iris =
                    Rect::from_min_max(p(ix - 2.45, ey - 1.95 * h), p(ix + 2.45, ey + 2.20 * h));
                painter.rect_filled(iris, CornerRadius::same(2), KCM_EYE_OUTER);

                if open > 0.42 {
                    let core = Rect::from_min_max(
                        p(ix - 1.80, ey - 1.30 * h),
                        p(ix + 1.80, ey + 1.75 * h),
                    );
                    painter.rect_filled(core, CornerRadius::same(1), KCM_EYE_CORE);

                    // Sharp vertical jet-black fox-slit pupil (`|`).
                    let slit = Rect::from_min_max(
                        p(ix - 0.56, ey - 1.65 * h),
                        p(ix + 0.56, ey + 1.90 * h),
                    );
                    painter.rect_filled(slit, CornerRadius::ZERO, KCM_INK);

                    // Crisp tiny upper-left glint beside the vertical slit.
                    painter.circle_filled(p(ix - 1.15, ey - 0.95 * h), 0.48, Color32::WHITE);
                }
            }
            Mode::Sage => {
                // Luminous golden-amber Toad Sage Mode iris with sharp dark
                // horizontal frog-bar pupil (`—`).
                let iris =
                    Rect::from_min_max(p(ix - 2.45, ey - 1.95 * h), p(ix + 2.45, ey + 2.20 * h));
                painter.rect_filled(iris, CornerRadius::same(2), SAGE_GOLD_OUTER);

                if open > 0.42 {
                    let inner_gold = Rect::from_min_max(
                        p(ix - 1.90, ey - 1.35 * h),
                        p(ix + 1.90, ey + 1.75 * h),
                    );
                    painter.rect_filled(inner_gold, CornerRadius::same(1), SAGE_GOLD_CORE);

                    let bar_half_h = (0.55 * h).max(0.42);
                    let bar = Rect::from_min_max(
                        p(ix - 1.82, iy - bar_half_h),
                        p(ix + 1.82, iy + bar_half_h),
                    );
                    painter.rect_filled(bar, CornerRadius::ZERO, SAGE_PUPIL);

                    painter.circle_filled(p(ix - 0.95, ey - 1.10 * h), 0.48, Color32::WHITE);
                }
            }
            Mode::Normal => {
                let iris =
                    Rect::from_min_max(p(ix - 2.35, ey - 1.95 * h), p(ix + 2.35, ey + 2.20 * h));
                painter.rect_filled(iris, CornerRadius::same(2), EYE_BLUE);

                if open > 0.42 {
                    let lower =
                        Rect::from_min_max(p(ix - 1.75, iy + 0.15), p(ix + 1.75, ey + 2.05 * h));
                    painter.rect_filled(lower, CornerRadius::same(1), EYE_SKY);

                    painter.circle_filled(p(ix, iy - 0.12), 0.80 * h, EYE_PUPIL);
                    painter.circle_filled(p(ix - 0.90, ey - 0.95 * h), 0.62, Color32::WHITE);
                }
            }
        }

        // Bold razor-sharp dark upper eyeliner/lash sweeping up at the outer wing.
        line(
            painter,
            vec![
                p(inner_x - side * 0.15, ey + 0.45 * h),
                p(top_mid_x, ey - 2.05 * h),
                p(outer_x + side * 0.35, ey - 2.42 * h),
            ],
            Stroke::new(if mode == Mode::Kurama { 1.65 } else { 1.55 }, lash_color),
        );
    }
}

fn headband(painter: &Painter, center: Pos2, lift: f32, flash: f32, mode: Mode) {
    let p = |x: f32, y: f32| center + Vec2::new(x, y + lift);
    let kurama = mode == Mode::Kurama;

    // Slim Konoha forehead protector band (jet-black KCM seal trim vs Konoha navy).
    poly(
        painter,
        vec![p(-12.7, -8.4), p(12.7, -8.4), p(12.6, -5.0), p(-12.6, -5.0)],
        if kurama { KCM_INK } else { BAND_NAVY },
        Stroke::NONE,
    );

    // Forehead protector plate (radiant golden chakra plate in KCM, brushed steel otherwise).
    let plate = Rect::from_min_max(p(-6.8, -8.15), p(6.8, -5.10));
    let plate_base = if kurama { KCM_CLOAK_GOLD } else { PLATE_STEEL };
    let plate_hi = if kurama { KCM_HAIR_TIP } else { PLATE_HI };
    let plate_etch = if kurama { KCM_INK } else { PLATE_ETCH };
    painter.rect_filled(plate, CornerRadius::same(1), plate_base);

    let hi_alpha = (180.0 + flash * 75.0).min(255.0) as u8;
    painter.rect_filled(
        Rect::from_min_max(p(-6.3, -7.95), p(6.3, -6.80)),
        CornerRadius::same(1),
        Color32::from_rgba_unmultiplied(plate_hi.r(), plate_hi.g(), plate_hi.b(), hi_alpha),
    );
    if kurama {
        painter.rect_stroke(
            plate,
            CornerRadius::same(1),
            Stroke::new(0.75, KCM_INK),
            eframe::egui::StrokeKind::Inside,
        );
    }

    // Engraved Konoha leaf swirl.
    line(
        painter,
        vec![
            p(-2.0, -6.15),
            p(-0.8, -7.30),
            p(0.75, -7.30),
            p(1.2, -6.50),
            p(0.2, -6.05),
            p(-0.35, -6.65),
        ],
        Stroke::new(0.85, plate_etch),
    );
    line(
        painter,
        vec![p(1.1, -6.50), p(2.2, -6.10)],
        Stroke::new(0.85, plate_etch),
    );
}

fn front_hair_and_bangs(
    painter: &Painter,
    center: Pos2,
    lift: f32,
    hair_sway: f32,
    bang_sway: f32,
    mode: Mode,
) {
    let p = |x: f32, y: f32| center + Vec2::new(x, y + lift);
    let kurama = mode == Mode::Kurama;
    let main = if kurama { KCM_HAIR_MAIN } else { HAIR_MAIN };
    let tip = if kurama { KCM_HAIR_TIP } else { HAIR_TIP };
    let edge = if kurama { KCM_HAIR_EDGE } else { HAIR_EDGE };

    // Unified clean flat-vector golden hair crown mass.
    poly(
        painter,
        vec![
            p(-14.2, -7.8),
            p(-12.2, -10.3),
            p(0.0, -11.0),
            p(12.2, -10.3),
            p(14.2, -7.8),
        ],
        main,
        Stroke::NONE,
    );

    // Razor-sharp angular Minato crown & lateral spikes.
    let crown_spikes = [
        (
            vec![
                p(-13.8, -7.7),
                p(-23.8 + hair_sway * 0.95, -11.2),
                p(-11.4, -10.2),
                p(-8.6, -7.8),
            ],
            main,
        ),
        (
            vec![
                p(-10.8, -8.0),
                p(-15.0 + hair_sway * 0.8, -14.0),
                p(-4.8, -10.6),
                p(-2.8, -8.0),
            ],
            if kurama { tip } else { main },
        ),
        (
            vec![
                p(-5.6, -8.2),
                p(-3.4 + hair_sway * 0.7, -14.3),
                p(2.0, -10.5),
                p(1.2, -8.2),
            ],
            tip,
        ),
        (
            vec![
                p(-0.4, -8.2),
                p(4.8 + hair_sway * 0.75, -14.3),
                p(8.0, -10.3),
                p(5.6, -8.0),
            ],
            if kurama { tip } else { main },
        ),
        (
            vec![
                p(4.8, -8.0),
                p(14.8 + hair_sway * 0.85, -13.7),
                p(13.6, -9.8),
                p(10.4, -7.8),
            ],
            main,
        ),
        (
            vec![
                p(9.8, -7.8),
                p(23.8 + hair_sway * 0.95, -10.8),
                p(16.2 + hair_sway * 0.35, -7.0),
                p(12.2, -6.2),
            ],
            main,
        ),
    ];
    for (pts, color) in crown_spikes {
        poly(painter, pts, color, Stroke::NONE);
    }

    // Three bold, razor-sharp Minato forehead bangs cutting cleanly over the headband edges.
    let bangs = [
        vec![
            p(-12.9, -8.5),
            p(-7.4, -8.2),
            p(-8.9 + bang_sway * 0.4, -3.0),
        ],
        vec![
            p(-1.25, -8.5),
            p(1.15, -8.5),
            p(-0.2 + bang_sway * 0.25, -4.1),
        ],
        vec![p(7.4, -8.2), p(12.9, -8.5), p(9.0 + bang_sway * 0.4, -2.9)],
    ];
    for pts in bangs {
        poly(painter, pts, main, Stroke::NONE);
    }

    // Minato's signature long, razor-sharp front bangs curving gracefully inward
    // along both cheeks and overlapping cleanly over the collar.
    poly(
        painter,
        vec![
            p(-14.2, -8.1),
            p(-11.0, -6.2),
            p(-10.8, 2.0),
            p(-11.8 + bang_sway * 0.6, 10.6),
            p(-15.2 + bang_sway * 0.4, 1.8),
        ],
        main,
        Stroke::NONE,
    );
    poly(
        painter,
        vec![
            p(11.0, -6.2),
            p(14.2, -8.1),
            p(15.2 + bang_sway * 0.4, 1.8),
            p(11.8 + bang_sway * 0.6, 10.6),
            p(10.8, 2.0),
        ],
        main,
        Stroke::NONE,
    );

    // Crisp molten-amber inner edge along the framing bangs separating hair from face.
    line(
        painter,
        vec![
            p(-11.0, -5.8),
            p(-10.8, 2.0),
            p(-11.7 + bang_sway * 0.6, 10.3),
        ],
        Stroke::new(0.95, edge),
    );
    line(
        painter,
        vec![p(11.0, -5.8), p(10.8, 2.0), p(11.7 + bang_sway * 0.6, 10.3)],
        Stroke::new(0.95, edge),
    );

    // Subtle crisp highlight flick on the top-left crown spike.
    poly(
        painter,
        vec![
            p(-9.2, -11.0),
            p(-13.2 + hair_sway * 0.65, -13.2),
            p(-5.8, -11.4),
        ],
        tip,
        Stroke::NONE,
    );
}

fn flying_raijin_kunai(
    painter: &Painter,
    center: Pos2,
    phase: f32,
    flash: f32,
    mode: Mode,
    show_aura: bool,
    animate: bool,
) {
    let kurama = mode == Mode::Kurama;
    let hover = if animate {
        (phase - 1.1).sin() * 0.75
    } else {
        0.0
    };
    let origin = center + Vec2::new(32.5, 2.6 + hover);
    let angle: f32 = 0.74
        + if animate {
            (phase - 0.7).cos() * 0.04
        } else {
            0.0
        };
    let (sa, ca) = angle.sin_cos();
    let xform = |x: f32, y: f32| origin + Vec2::new(x * ca - y * sa, x * sa + y * ca);

    if kurama && show_aura {
        // Supercharged golden chakra aura hugging the Flying Raijin kunai.
        radial_glow(
            painter,
            origin,
            10.0,
            9.0,
            Color32::from_rgba_unmultiplied(255, 198, 42, 48 + (flash * 42.0) as u8),
            14,
        );
    }

    let steel_dark = if kurama {
        Color32::from_rgb(122, 92, 34)
    } else {
        Color32::from_rgb(70, 84, 108)
    };
    let steel_mid = if kurama {
        Color32::from_rgb(214, 194, 132)
    } else {
        Color32::from_rgb(164, 182, 206)
    };
    let steel_hi = if kurama {
        KCM_HAIR_TIP
    } else {
        Color32::from_rgb(232, 244, 255)
    };

    poly(
        painter,
        vec![
            xform(0.0, -7.6),
            xform(1.45, -1.9),
            xform(0.0, 0.2),
            xform(-1.45, -1.9),
        ],
        steel_mid,
        Stroke::NONE,
    );
    line(
        painter,
        vec![xform(0.0, -7.4), xform(0.85, -2.1)],
        Stroke::new(0.7, steel_hi),
    );
    poly(
        painter,
        vec![
            xform(-1.15, -2.1),
            xform(-3.6, -3.9),
            xform(-2.1, -0.3),
            xform(-0.75, 0.2),
        ],
        steel_dark,
        Stroke::NONE,
    );
    poly(
        painter,
        vec![
            xform(1.15, -2.1),
            xform(3.6, -3.9),
            xform(2.1, -0.3),
            xform(0.75, 0.2),
        ],
        steel_mid,
        Stroke::NONE,
    );

    poly(
        painter,
        vec![
            xform(-1.0, 0.2),
            xform(1.0, 0.2),
            xform(0.92, 5.0),
            xform(-0.92, 5.0),
        ],
        if kurama {
            Color32::from_rgb(255, 244, 196)
        } else {
            Color32::from_rgb(242, 236, 218)
        },
        Stroke::NONE,
    );
    for hy in [1.7, 3.3] {
        line(
            painter,
            vec![xform(-0.65, hy), xform(0.65, hy)],
            Stroke::new(0.75, if kurama { KCM_INK } else { FLAME_CRIMSON }),
        );
    }

    painter.circle_stroke(xform(0.0, 6.3), 1.45, Stroke::new(1.0, steel_hi));

    if !show_aura {
        return;
    }

    // Supercharged golden KCM starburst spark at the kunai tip.
    let tip = xform(0.0, -7.8);
    let shimmer = (if kurama { 0.85 } else { 0.72 }) + 0.28 * (phase * 2.0).sin() + flash * 0.55;
    radial_glow(
        painter,
        tip,
        (if kurama { 5.4 } else { 4.6 }) + flash * 3.2,
        (if kurama { 5.4 } else { 4.6 }) + flash * 3.2,
        Color32::from_rgba_unmultiplied(
            255,
            if kurama { 216 } else { 206 },
            if kurama { 64 } else { 78 },
            ((if kurama { 145.0 } else { 115.0 }) + flash * 95.0).min(240.0) as u8,
        ),
        14,
    );

    let ray_rot = phase * 0.25;
    let ray_color = Color32::from_rgba_unmultiplied(
        255,
        232,
        118,
        ((if kurama { 215.0 } else { 195.0 }) + flash * 40.0).min(255.0) as u8,
    );
    let core_color = Color32::from_rgba_unmultiplied(255, 254, 230, 250);
    for i in 0..4 {
        let a = ray_rot + i as f32 * (PI * 0.25);
        let len = (if i % 2 == 0 { 3.5 } else { 2.4 }) * shimmer;
        let perp = a + PI * 0.5;
        let w = 0.44 + flash * 0.16;
        let dir = Vec2::new(a.cos(), a.sin());
        let side = Vec2::new(perp.cos(), perp.sin()) * w;
        poly(
            painter,
            vec![tip - dir * len, tip + side, tip + dir * len, tip - side],
            ray_color,
            Stroke::NONE,
        );
    }
    painter.circle_filled(tip, 0.9 + flash * 0.35, core_color);
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui;

    #[test]
    fn blink_closes_and_reopens_in_both_cycles() {
        for cycle in [0.0, 4.2] {
            assert_eq!(eye_openness(cycle + 1.5), 1.0);
            assert!(eye_openness(cycle + 1.755) < 0.02);
            assert_eq!(eye_openness(cycle + 2.0), 1.0);
        }
    }

    #[test]
    fn geometry_stays_inside_collapsed_pill_across_animation() {
        let ctx = egui::Context::default();
        let pill = Rect::from_min_size(
            Pos2::ZERO,
            Vec2::new(crate::ui::W_COLLAPSED, crate::ui::H_COLLAPSED),
        );
        let center = pill.center();

        for mode in [Mode::Kurama, Mode::Sage, Mode::Normal] {
            for step in 0..24 {
                let seconds = step as f64 * 0.2;
                let raw = egui::RawInput {
                    screen_rect: Some(pill),
                    ..Default::default()
                };
                let mut output = ctx.run_ui(raw, |ui| {
                    let painter = ui.painter();
                    draw_mode(painter, center, seconds, mode);
                });
                output.textures_delta.clear();
                let primitives = ctx.tessellate(output.shapes, 1.0);
                assert!(
                    !primitives.is_empty(),
                    "mascot emitted no shapes at t={seconds} ({mode:?})"
                );
                for prim in primitives {
                    if let egui::epaint::Primitive::Mesh(mesh) = prim.primitive {
                        for v in &mesh.vertices {
                            assert!(
                                v.pos.x >= 2.0 && v.pos.x <= crate::ui::W_COLLAPSED - 2.0,
                                "vertex x={} outside pill at t={seconds} ({mode:?})",
                                v.pos.x
                            );
                            assert!(
                                v.pos.y >= 1.2 && v.pos.y <= crate::ui::H_COLLAPSED,
                                "vertex y={} outside pill at t={seconds} ({mode:?})",
                                v.pos.y
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    #[ignore = "exports visual inspection PNGs to /tmp/minato-idle-preview.png"]
    fn export_preview_png() {
        let out_path = std::env::var("MINATO_PREVIEW_OUT")
            .unwrap_or_else(|_| "/tmp/minato-idle-preview.png".to_owned());
        let mode = match std::env::var("MINATO_PREVIEW_MODE")
            .unwrap_or_else(|_| "kurama".to_owned())
            .as_str()
        {
            "normal" => Mode::Normal,
            "sage" => Mode::Sage,
            _ => Mode::Kurama,
        };
        let w = crate::ui::W_COLLAPSED as u32;
        let h = crate::ui::H_COLLAPSED as u32;
        let render_frame = |seconds: f64, scale: f32| -> image::RgbaImage {
            let ctx = egui::Context::default();
            let pill = Rect::from_min_size(
                Pos2::ZERO,
                Vec2::new(crate::ui::W_COLLAPSED, crate::ui::H_COLLAPSED),
            );
            let raw = egui::RawInput {
                screen_rect: Some(pill),
                ..Default::default()
            };
            let mut output = ctx.run_ui(raw, |ui| {
                let painter = ui.painter();
                let radius = CornerRadius {
                    nw: 0,
                    ne: 0,
                    sw: 8,
                    se: 8,
                };
                painter.rect_filled(pill, radius, Color32::from_rgb(9, 9, 12));
                painter.rect_stroke(
                    pill,
                    radius,
                    Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 20)),
                    egui::StrokeKind::Inside,
                );
                draw_mode(painter, pill.center(), seconds, mode);
            });
            output.textures_delta.clear();
            let primitives = ctx.tessellate(output.shapes, scale);
            let iw = (w as f32 * scale).round() as u32;
            let ih = (h as f32 * scale).round() as u32;
            let mut buf = vec![[28.0_f32, 32.0, 40.0, 255.0]; (iw * ih) as usize];
            for prim in primitives {
                if let egui::epaint::Primitive::Mesh(mesh) = prim.primitive {
                    rasterize_mesh(&mesh, scale, iw, ih, &mut buf);
                }
            }
            let mut img = image::RgbaImage::new(iw, ih);
            for y in 0..ih {
                for x in 0..iw {
                    let px = buf[(y * iw + x) as usize];
                    img.put_pixel(
                        x,
                        y,
                        image::Rgba([
                            px[0].clamp(0.0, 255.0) as u8,
                            px[1].clamp(0.0, 255.0) as u8,
                            px[2].clamp(0.0, 255.0) as u8,
                            255,
                        ]),
                    );
                }
            }
            img
        };

        let frames_1x = [
            render_frame(0.0, 1.0),
            render_frame(0.70, 1.0),
            render_frame(1.755, 1.0),
            render_frame(2.55, 1.0),
        ];
        let hero_4x_vec = render_frame(2.55, 4.0);
        let pad = 16_u32;
        let zoom = 4_u32;
        let crop_x = 12_u32;
        let crop_w = 190_u32;
        let sheet_w = (crop_w * zoom).max(w * zoom) + pad * 2;
        let sheet_h = pad + h + pad + h * zoom + pad + 4 * (h * zoom + 10) + pad;
        let mut sheet =
            image::RgbaImage::from_pixel(sheet_w, sheet_h, image::Rgba([28, 32, 40, 255]));

        let x_1x = (sheet_w - w) / 2;
        image::imageops::overlay(&mut sheet, &frames_1x[0], x_1x as i64, pad as i64);

        let y_vec = pad + h + pad;
        image::imageops::overlay(&mut sheet, &hero_4x_vec, pad as i64, y_vec as i64);

        let mut y_cur = y_vec + h * zoom + pad;
        for frame in &frames_1x {
            let sub = image::imageops::crop_imm(frame, crop_x, 0, crop_w, h).to_image();
            let zoomed = image::imageops::resize(
                &sub,
                crop_w * zoom,
                h * zoom,
                image::imageops::FilterType::Nearest,
            );
            let x_pos = (sheet_w - crop_w * zoom) / 2;
            image::imageops::overlay(&mut sheet, &zoomed, x_pos as i64, y_cur as i64);
            y_cur += h * zoom + 10;
        }

        sheet.save(&out_path).expect("save preview png");
    }

    fn rasterize_mesh(mesh: &Mesh, scale: f32, w: u32, h: u32, buf: &mut [[f32; 4]]) {
        for tri in mesh.indices.as_chunks::<3>().0 {
            let v0 = mesh.vertices[tri[0] as usize];
            let v1 = mesh.vertices[tri[1] as usize];
            let v2 = mesh.vertices[tri[2] as usize];
            let p0 = Vec2::new(v0.pos.x * scale, v0.pos.y * scale);
            let p1 = Vec2::new(v1.pos.x * scale, v1.pos.y * scale);
            let p2 = Vec2::new(v2.pos.x * scale, v2.pos.y * scale);

            let min_x = p0.x.min(p1.x).min(p2.x).floor().max(0.0) as u32;
            let max_x = p0.x.max(p1.x).max(p2.x).ceil().min((w - 1) as f32) as u32;
            let min_y = p0.y.min(p1.y).min(p2.y).floor().max(0.0) as u32;
            let max_y = p0.y.max(p1.y).max(p2.y).ceil().min((h - 1) as f32) as u32;

            let area = (p1.x - p0.x) * (p2.y - p0.y) - (p2.x - p0.x) * (p1.y - p0.y);
            if area.abs() < 1e-6 {
                continue;
            }
            let inv_area = 1.0 / area;

            for y in min_y..=max_y {
                let py = y as f32 + 0.5;
                for x in min_x..=max_x {
                    let px = x as f32 + 0.5;
                    let w0 = ((p1.x - px) * (p2.y - py) - (p2.x - px) * (p1.y - py)) * inv_area;
                    let w1 = ((p2.x - px) * (p0.y - py) - (p0.x - px) * (p2.y - py)) * inv_area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0 {
                        let sr = w0 * v0.color.r() as f32
                            + w1 * v1.color.r() as f32
                            + w2 * v2.color.r() as f32;
                        let sg = w0 * v0.color.g() as f32
                            + w1 * v1.color.g() as f32
                            + w2 * v2.color.g() as f32;
                        let sb = w0 * v0.color.b() as f32
                            + w1 * v1.color.b() as f32
                            + w2 * v2.color.b() as f32;
                        let sa = (w0 * v0.color.a() as f32
                            + w1 * v1.color.a() as f32
                            + w2 * v2.color.a() as f32)
                            / 255.0;
                        let dst = &mut buf[(y * w + x) as usize];
                        let inv_a = 1.0 - sa.clamp(0.0, 1.0);
                        dst[0] = sr + dst[0] * inv_a;
                        dst[1] = sg + dst[1] * inv_a;
                        dst[2] = sb + dst[2] * inv_a;
                    }
                }
            }
        }
    }
}
