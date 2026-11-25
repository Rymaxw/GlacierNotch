//! Notch silhouette, tab bar and the five panels. Everything is painted
//! directly: no widget tree, so the collapsed pill stays a single rounded shape
//! with no seams.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Shape, Stroke, StrokeKind, Vec2,
};

use crate::clipboard::Clips;
use crate::controls::{self, Controls};
use crate::focus::{self, Timer};
use crate::lyrics::{LyricState, Lyrics};
use crate::media::{Cmd, Track, next_rate};
use crate::shelf::{self, Shelf};
use glacier::notes::{self, Notes};

mod mascot;
mod native;

const PAGE_SIZE: usize = 5;

pub const W_COLLAPSED: f32 = 214.0;
pub const H_COLLAPSED: f32 = 34.0;
pub const W_EXPANDED: f32 = 424.0;
pub const H_EXPANDED: f32 = 284.0;
pub const COLLAPSED_FADE: f32 = 0.40;

const TAB_H: f32 = 26.0;

const BG: Color32 = Color32::from_rgb(9, 9, 12);
const TEXT: Color32 = Color32::from_rgb(240, 240, 245);
const MUTED: Color32 = Color32::from_rgb(150, 150, 162);
const FAINT: Color32 = Color32::from_rgb(134, 134, 146);
const ACCENT: Color32 = Color32::from_rgb(122, 211, 255);
const TRACK: Color32 = Color32::from_rgb(28, 28, 35);
const CHIP: Color32 = Color32::from_rgb(24, 24, 31);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Music,
    Control,
    Focus,
    Shelf,
    Clips,
}

impl Tab {
    pub const ALL: [Tab; 5] = [Tab::Music, Tab::Control, Tab::Focus, Tab::Shelf, Tab::Clips];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Music => "Music",
            Tab::Control => "Control",
            Tab::Focus => "Focus",
            Tab::Shelf => "Shelf",
            Tab::Clips => "Clips",
        }
    }

    pub fn step(self, forward: bool) -> Self {
        let index = Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0);
        let len = Self::ALL.len();
        let next = if forward {
            (index + 1) % len
        } else {
            (index + len - 1) % len
        };
        Self::ALL[next]
    }
}

/// Everything the notch can ask the app to do. One action per frame is enough:
/// the pointer cannot click two controls at once.
#[derive(Clone)]
pub enum Action {
    Ui(UiAction),
    Note(notes::Action),
    Transport(Cmd),
    Seek(f64),
    Rate(f64),
    Volume { value: f32, force: bool },
    Brightness { value: f32, force: bool },
    Timer(focus::Action),
    Shelf(shelf::Action),
    Clips(crate::clipboard::Action),
    Tab(Tab),
}

/// Drag state that has to survive between frames.
#[derive(Clone, Copy)]
pub enum UiAction {
    Notes(bool),
    EditNote(Option<u64>),
    CancelNote,
    Page(i8),
}

pub struct Draft {
    id: Option<u64>,
    text: String,
}

#[derive(Default)]
pub struct Interactions {
    progress: bool,
    slider: Option<u8>,
    page: usize,
    notes: bool,
    editor: Option<Draft>,
    keyboard_mode: bool,
    focus_first: bool,
}

impl Interactions {
    pub fn begin_frame(&mut self, ctx: &egui::Context) -> bool {
        let escaped_editor =
            self.editor.is_some() && ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if escaped_editor {
            self.editor = None;
            self.keyboard_mode = false;
        }
        if ctx.input(|i| i.pointer.any_pressed() || i.viewport().focused == Some(false)) {
            self.keyboard_mode = false;
        }
        if self.editor.is_none() && ctx.input(|i| i.key_pressed(egui::Key::Tab)) {
            self.keyboard_mode = true;
            self.focus_first = ctx.memory(|m| m.focused().is_none());
        }
        escaped_editor
    }

    pub fn holds_open(&self) -> bool {
        self.editor.is_some() || self.keyboard_mode
    }

    pub fn editing(&self) -> bool {
        self.editor.is_some()
    }

    pub fn reset_page(&mut self) {
        self.page = 0;
    }

    pub fn act(&mut self, action: UiAction, notes: &Notes) {
        match action {
            UiAction::Notes(value) => {
                self.notes = value;
                self.page = 0;
            }
            UiAction::EditNote(id) => {
                let text = id
                    .and_then(|id| notes.items.iter().find(|note| note.id == id))
                    .map(|note| note.text.clone())
                    .unwrap_or_default();
                self.editor = Some(Draft { id, text });
                self.focus_first = true;
                self.keyboard_mode = true;
            }
            UiAction::CancelNote => {
                self.editor = None;
                self.keyboard_mode = false;
            }
            UiAction::Page(delta) => {
                self.page = if delta < 0 {
                    self.page.saturating_sub(1)
                } else {
                    self.page.saturating_add(1)
                };
            }
        }
    }

    pub fn note_saved(&mut self) {
        self.editor = None;
        self.keyboard_mode = false;
    }
}

pub struct View<'a> {
    pub w: f32,
    pub h: f32,
    pub open: f32,
    pub seconds: f64,
    pub tab: Tab,
    pub track: &'a Track,
    pub lyrics: &'a Lyrics,
    pub controls: &'a Controls,
    pub artwork: &'a crate::artwork::Artwork,
    pub timer: &'a Timer,
    pub shelf: &'a Shelf,
    pub clips: &'a Clips,
    pub notes: &'a Notes,
    pub hovering_files: bool,
}

/// Pointer state for one frame.
struct Pointer {
    at: Option<Pos2>,
    clicked: bool,
    down: bool,
    released: bool,
    scroll: f32,
    live: bool,
}

fn edge() -> Color32 {
    Color32::from_rgba_unmultiplied(255, 255, 255, 20)
}

fn corners(bottom: u8) -> CornerRadius {
    CornerRadius {
        nw: 0,
        ne: 0,
        sw: bottom,
        se: bottom,
    }
}

fn title_font() -> FontId {
    FontId::new(15.0, egui::FontFamily::Proportional)
}

fn sub_font() -> FontId {
    FontId::new(12.0, egui::FontFamily::Proportional)
}

fn tiny() -> FontId {
    FontId::new(10.5, egui::FontFamily::Proportional)
}

fn tab_font() -> FontId {
    FontId::new(11.0, egui::FontFamily::Proportional)
}

/// Paint the notch and report the single action this frame produced.
pub fn draw(root: &mut egui::Ui, state: &mut Interactions, view: &View<'_>) -> Option<Action> {
    let ctx = root.ctx().clone();
    let count = match view.tab {
        Tab::Shelf => view.shelf.items.len(),
        Tab::Clips if state.notes => view.notes.items.len(),
        Tab::Clips => view.clips.items.len(),
        _ => 0,
    };
    state.page = state.page.min(count.saturating_sub(1) / PAGE_SIZE);
    let w = view.w;
    let h = view.h;
    let x = (W_EXPANDED - w) / 2.0;
    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(w, h));
    let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("glacier-notch"));
    let painter = ctx.layer_painter(layer).with_clip_rect(ctx.viewport_rect());

    let radius = (8.0 + 6.0 * view.open).round() as u8;
    painter.rect_filled(rect, corners(radius), BG);
    painter.rect_stroke(
        rect,
        corners(radius),
        Stroke::new(1.0, edge()),
        StrokeKind::Inside,
    );
    if view.hovering_files {
        painter.rect_stroke(
            rect.shrink(1.0),
            corners(radius.saturating_sub(1)),
            Stroke::new(1.5, ACCENT),
            StrokeKind::Inside,
        );
    }

    let pointer = Pointer {
        at: ctx
            .input(|i| i.pointer.hover_pos())
            .map(|p| Pos2::new(p.x - x, p.y)),
        clicked: ctx.input(|i| i.pointer.primary_clicked()),
        down: ctx.input(|i| i.pointer.primary_down()),
        released: ctx.input(|i| i.pointer.primary_released()),
        scroll: ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|event| match event {
                    egui::Event::MouseWheel { delta, .. } => Some(delta.y),
                    _ => None,
                })
                .sum()
        }),
        live: view.open > 0.95 && state.editor.is_none(),
    };

    if view.open < COLLAPSED_FADE {
        let mut faded = painter.clone();
        faded.set_opacity((1.0 - view.open / COLLAPSED_FADE).clamp(0.0, 1.0));
        collapsed(&faded, view);
    }

    let mut action = None;
    if view.open > COLLAPSED_FADE {
        let mut body =
            painter.with_clip_rect(Rect::from_min_size(Pos2::ZERO, Vec2::new(W_EXPANDED, h)));
        body.set_opacity(((view.open - COLLAPSED_FADE) / (1.0 - COLLAPSED_FADE)).clamp(0.0, 1.0));
        action = tab_bar(&body, w, view.tab, &pointer).map(Action::Tab);
        let panel = match view.tab {
            Tab::Music => music(&body, view, &pointer, state),
            Tab::Control => control(&body, view, &pointer, state),
            Tab::Focus => focus_panel(&body, view, &pointer),
            Tab::Shelf => shelf_panel(&body, view, &pointer, state),
            Tab::Clips => clips_panel(&body, view, &pointer, state),
        };
        if action.is_none() {
            action = panel;
        }
    }

    ctx.transform_layer_shapes(
        layer,
        egui::emath::TSTransform::from_translation(Vec2::new(x, 0.0)),
    );
    if view.open > 0.95 {
        let native_action = native::draw(&ctx, state, view, Vec2::new(x, 0.0));
        if action.is_none() {
            action = native_action;
        }
    }
    if !pointer.down {
        state.progress = false;
        state.slider = None;
    }
    action
}

fn tab_bar(painter: &egui::Painter, w: f32, active: Tab, pointer: &Pointer) -> Option<Tab> {
    let font = tab_font();
    // Equal-width slots so a tap target exists even for the shortest label.
    let slot = w / Tab::ALL.len() as f32;
    let mut hit = None;

    for (index, tab) in Tab::ALL.iter().enumerate() {
        let rect = Rect::from_min_size(
            Pos2::new(slot * index as f32, 0.0),
            Vec2::new(slot, TAB_H - 4.0),
        );
        let selected = *tab == active;
        let hovered = pointer.at.is_some_and(|p| rect.contains(p));
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            tab.label(),
            font.clone(),
            if selected {
                TEXT
            } else if hovered {
                MUTED
            } else {
                FAINT
            },
        );
        if selected {
            let underline = Rect::from_min_size(
                Pos2::new(rect.center().x - 14.0, TAB_H - 4.0),
                Vec2::new(28.0, 2.0),
            );
            painter.rect_filled(underline, CornerRadius::same(1), ACCENT);
        }
        if pointer.live && pointer.clicked && hovered {
            hit = Some(*tab);
        }
    }

    painter.rect_filled(
        Rect::from_min_max(Pos2::new(0.0, TAB_H - 1.0), Pos2::new(w, TAB_H)),
        CornerRadius::ZERO,
        Color32::from_rgb(20, 20, 26),
    );
    hit
}

fn music(
    painter: &egui::Painter,
    view: &View<'_>,
    pointer: &Pointer,
    state: &mut Interactions,
) -> Option<Action> {
    let w = view.w;
    let track = view.track;
    let accent = track_accent(track, view.artwork);
    let mut action = None;

    let dot = Pos2::new(16.0, 48.0);
    painter.circle_filled(dot, 3.2, if track.is_playing() { accent } else { FAINT });

    let header = if track.title.is_empty() {
        "no player".to_owned()
    } else {
        track.player.to_lowercase()
    };
    painter.text(
        Pos2::new(w - 16.0, 48.0),
        Align2::RIGHT_CENTER,
        header,
        tiny(),
        FAINT,
    );

    let art = Rect::from_min_size(Pos2::new(16.0, 66.0), Vec2::splat(56.0));
    let fallback = track_fallback_color(track);
    painter.rect_filled(art, CornerRadius::same(12), fallback);
    if let Some(texture) = view.artwork.texture() {
        painter.image(
            texture.id(),
            art,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    } else {
        let monogram = track_monogram(track);
        painter.circle_filled(
            art.center(),
            19.0,
            Color32::from_rgba_unmultiplied(0, 0, 0, 44),
        );
        painter.text(
            art.center(),
            Align2::CENTER_CENTER,
            monogram,
            FontId::new(23.0, egui::FontFamily::Proportional),
            if track.title.is_empty() { FAINT } else { TEXT },
        );
    }
    painter.rect_stroke(
        art,
        CornerRadius::same(12),
        Stroke::new(1.0, edge()),
        StrokeKind::Inside,
    );

    let text_x = 86.0;
    let text_w = w - text_x - 18.0;
    let title = if track.title.is_empty() {
        "Play something to see it here".to_owned()
    } else {
        track.title.clone()
    };
    painter.text(
        Pos2::new(text_x, 76.0),
        Align2::LEFT_TOP,
        clip(painter, &title, &title_font(), text_w),
        title_font(),
        if track.title.is_empty() {
            MUTED
        } else {
            accent
        },
    );
    painter.text(
        Pos2::new(text_x, 96.0),
        Align2::LEFT_TOP,
        clip(painter, &track.artist, &sub_font(), text_w),
        sub_font(),
        MUTED,
    );
    painter.text(
        Pos2::new(text_x, 114.0),
        Align2::LEFT_TOP,
        clip(painter, &track.album, &tiny(), text_w),
        tiny(),
        FAINT,
    );

    // ── scrubbable progress ─────────────────────────────────────────────────
    let bar = Rect::from_min_max(Pos2::new(16.0, 138.0), Pos2::new(w - 16.0, 142.0));
    let hit = Rect::from_min_max(Pos2::new(16.0, 128.0), Pos2::new(w - 16.0, 152.0));
    painter.rect_filled(bar, CornerRadius::same(2), TRACK);

    let seekable = track.can_seek && track.length.is_some_and(|len| len > 0.0);
    let mut progress = track.progress();
    let mut scrubbing = false;
    if seekable {
        if pointer.live
            && pointer.clicked
            && let Some(p) = pointer.at
            && hit.contains(p)
        {
            state.progress = true;
        }
        if state.progress
            && let Some(p) = pointer.at
        {
            progress = ((p.x - 16.0) / (w - 32.0)).clamp(0.0, 1.0);
            scrubbing = true;
            let length = track.length.unwrap_or(0.0);
            action = Some(Action::Seek((progress as f64 * length).max(0.0)));
        }
        if pointer.released && state.progress {
            state.progress = false;
        }
    }

    if progress > 0.0 {
        let filled = Rect::from_min_max(bar.min, Pos2::new(16.0 + (w - 32.0) * progress, 142.0));
        painter.rect_filled(filled, CornerRadius::same(2), accent);
        painter.circle_filled(
            Pos2::new(filled.max.x, 140.0),
            if scrubbing { 4.5 } else { 3.0 },
            accent,
        );
    }

    // ── elapsed / remaining double as -5s and +5s ───────────────────────────
    let elapsed = Rect::from_min_max(Pos2::new(16.0, 150.0), Pos2::new(70.0, 166.0));
    let remaining = Rect::from_min_max(Pos2::new(w - 70.0, 150.0), Pos2::new(w - 16.0, 166.0));
    let elapsed_hover = seekable && pointer.at.is_some_and(|p| elapsed.contains(p));
    let remaining_hover = seekable && pointer.at.is_some_and(|p| remaining.contains(p));

    painter.text(
        Pos2::new(16.0, 148.0),
        Align2::LEFT_TOP,
        clock(track.position),
        tiny(),
        if elapsed_hover { TEXT } else { accent },
    );
    painter.text(
        Pos2::new(w - 16.0, 148.0),
        Align2::RIGHT_TOP,
        track
            .remaining()
            .map(clock)
            .unwrap_or_else(|| "--:--".into()),
        tiny(),
        if remaining_hover { TEXT } else { accent },
    );

    if pointer.live && pointer.clicked && !state.progress {
        if elapsed_hover {
            action = Some(Action::Transport(Cmd::SeekBy(-5.0)));
        } else if remaining_hover {
            action = Some(Action::Transport(Cmd::SeekBy(5.0)));
        }
    }

    // ── lyrics: the active and next line are seek targets ───────────────────
    let [current, next] = lyric_rows(view);
    let line_now = Rect::from_min_max(Pos2::new(16.0, 168.0), Pos2::new(w - 16.0, 190.0));
    let line_next = Rect::from_min_max(Pos2::new(16.0, 190.0), Pos2::new(w - 16.0, 210.0));
    let now_hover = seekable && pointer.at.is_some_and(|p| line_now.contains(p));
    let next_hover = seekable && pointer.at.is_some_and(|p| line_next.contains(p));

    painter.text(
        Pos2::new(w / 2.0, 179.0),
        Align2::CENTER_CENTER,
        clip(
            painter,
            &current.text,
            &FontId::proportional(13.0),
            w - 36.0,
        ),
        FontId::proportional(13.0),
        if now_hover {
            ACCENT
        } else if current.synced {
            TEXT
        } else {
            MUTED
        },
    );
    if !next.text.is_empty() {
        painter.text(
            Pos2::new(w / 2.0, 198.0),
            Align2::CENTER_CENTER,
            clip(painter, &next.text, &FontId::proportional(11.0), w - 36.0),
            FontId::proportional(11.0),
            if next_hover { ACCENT } else { MUTED },
        );
    }

    if pointer.live && pointer.clicked && action.is_none() {
        if now_hover && let Some(at) = current.at {
            action = Some(Action::Seek(at));
        } else if next_hover && let Some(at) = next.at {
            action = Some(Action::Seek(at));
        }
    }

    // ── transport ───────────────────────────────────────────────────────────
    let cy = 246.0;
    let (previous_x, play_x, next_x) = transport_centers(w);
    let prev = Rect::from_center_size(Pos2::new(previous_x, cy), Vec2::splat(30.0));
    let play = Rect::from_center_size(Pos2::new(play_x, cy), Vec2::splat(38.0));
    let next_btn = Rect::from_center_size(Pos2::new(next_x, cy), Vec2::splat(30.0));

    for (r, enabled) in [(prev, track.can_prev), (next_btn, track.can_next)] {
        if enabled && pointer.at.is_some_and(|p| r.contains(p)) {
            painter.circle_filled(r.center(), r.width() / 2.0, CHIP);
        }
    }

    let dim = Color32::from_rgb(64, 64, 76);
    let active = if track.title.is_empty() { dim } else { TEXT };
    skip_icon(painter, prev.center(), active, true, track.can_prev);
    skip_icon(painter, next_btn.center(), active, false, track.can_next);

    if pointer.at.is_some_and(|p| play.contains(p)) && !track.title.is_empty() {
        painter.circle_filled(play.center(), 17.0, Color32::from_rgb(30, 30, 38));
    }
    if track.is_playing() {
        pause_icon(painter, play.center(), accent);
    } else {
        play_icon(
            painter,
            play.center(),
            if track.title.is_empty() { dim } else { accent },
        );
    }

    // ── speed badge ─────────────────────────────────────────────────────────
    let badge = Rect::from_center_size(Pos2::new(w - 46.0, cy), Vec2::new(48.0, 22.0));
    let badge_hover = track.can_control && pointer.at.is_some_and(|p| badge.contains(p));
    painter.rect_filled(
        badge,
        CornerRadius::same(7),
        if badge_hover { CHIP } else { TRACK },
    );
    painter.text(
        badge.center(),
        Align2::CENTER_CENTER,
        rate_label(track.rate),
        tiny(),
        if track.can_control { TEXT } else { FAINT },
    );

    if pointer.live
        && pointer.clicked
        && action.is_none()
        && let Some(p) = pointer.at
    {
        if play.contains(p) && !track.title.is_empty() {
            action = Some(Action::Transport(Cmd::PlayPause));
        } else if prev.contains(p) && track.can_prev {
            action = Some(Action::Transport(Cmd::Previous));
        } else if next_btn.contains(p) && track.can_next {
            action = Some(Action::Transport(Cmd::Next));
        } else if badge_hover {
            action = Some(Action::Rate(next_rate(track.rate)));
        }
    }

    // ── scroll anywhere on the panel steps the track ────────────────────────
    if seekable
        && action.is_none()
        && pointer.scroll.abs() > 0.5
        && pointer.at.is_some_and(|p| p.y > 34.0)
    {
        action = Some(Action::Transport(Cmd::SeekBy(if pointer.scroll > 0.0 {
            5.0
        } else {
            -5.0
        })));
    }

    action
}

/// One painted lyric row. `at` is the timestamp the row seeks to when clicked.
struct LyricRow {
    text: String,
    at: Option<f64>,
    synced: bool,
}

impl LyricRow {
    fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            at: None,
            synced: false,
        }
    }

    fn timed(text: impl Into<String>, at: f64) -> Self {
        Self {
            text: text.into(),
            at: Some(at),
            synced: true,
        }
    }
}

fn lyric_rows(view: &View<'_>) -> [LyricRow; 2] {
    match &view.lyrics.state {
        LyricState::Loading => [LyricRow::plain("Finding lyrics…"), LyricRow::plain("")],
        LyricState::Synced(_) => {
            let lines = view.lyrics.synced().unwrap_or_default();
            match view.lyrics.window(view.track.position) {
                Some((index, next)) => [
                    LyricRow::timed(lines[index].text.clone(), lines[index].at),
                    next.map(|slot| LyricRow::timed(lines[slot].text.clone(), lines[slot].at))
                        .unwrap_or_else(|| LyricRow::plain("")),
                ],
                None => [
                    LyricRow::timed("Waiting for first line…", 0.0),
                    lines
                        .first()
                        .map(|line| LyricRow::timed(line.text.clone(), line.at))
                        .unwrap_or_else(|| LyricRow::plain("")),
                ],
            }
        }
        LyricState::Plain(text) => [
            LyricRow::plain(
                text.lines()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or("Lyrics available (untimed)"),
            ),
            LyricRow::plain(""),
        ],
        LyricState::Instrumental => [LyricRow::plain("Instrumental"), LyricRow::plain("")],
        LyricState::Unavailable => [
            LyricRow::plain("No synced lyrics for this track"),
            LyricRow::plain(""),
        ],
    }
}

/// `1.0x`, `1.25x`, `2.0x`; trailing zeros dropped but one decimal kept.
fn rate_label(rate: f64) -> String {
    let mut text = format!("{rate:.2}");
    if text.ends_with('0') {
        text.pop();
    }
    format!("{text}x")
}

fn control(
    painter: &egui::Painter,
    view: &View<'_>,
    pointer: &Pointer,
    state: &mut Interactions,
) -> Option<Action> {
    let w = view.w;
    let mut action = None;
    let controls = view.controls;

    let rows: [(u8, &str, f32, bool); 2] = [
        (0, "Volume", controls.volume, controls.has_volume()),
        (
            1,
            "Brightness",
            controls.brightness,
            controls.has_brightness(),
        ),
    ];

    for (index, label, value, enabled) in rows {
        let max = if index == 0 {
            controls::VOLUME_MAX
        } else {
            1.0
        };
        let top = 92.0 + index as f32 * 72.0;
        painter.text(
            Pos2::new(16.0, top),
            Align2::LEFT_CENTER,
            label,
            sub_font(),
            if enabled { TEXT } else { FAINT },
        );
        painter.text(
            Pos2::new(w - 16.0, top),
            Align2::RIGHT_CENTER,
            if enabled {
                controls::percent(value)
            } else {
                "n/a".to_owned()
            },
            sub_font(),
            if enabled { MUTED } else { FAINT },
        );

        let bar = Rect::from_min_max(Pos2::new(16.0, top + 14.0), Pos2::new(w - 16.0, top + 20.0));
        let hit = Rect::from_min_max(Pos2::new(16.0, top + 4.0), Pos2::new(w - 16.0, top + 30.0));
        painter.rect_filled(bar, CornerRadius::same(3), TRACK);

        let span = w - 32.0;
        let mut shown = value;
        if enabled {
            if pointer.live && pointer.clicked && pointer.at.is_some_and(|p| hit.contains(p)) {
                state.slider = Some(index);
            }
            if state.slider == Some(index)
                && let Some(p) = pointer.at
            {
                let ratio = ((p.x - 16.0) / span).clamp(0.0, 1.0);
                shown = ratio * max;
                action = Some(slider_action(index, shown, false));
            }
            if pointer.released && state.slider == Some(index) {
                action = Some(slider_action(index, shown, true));
                state.slider = None;
            }
            if pointer.scroll.abs() > 0.5 && pointer.at.is_some_and(|p| hit.contains(p)) {
                let step = 0.03;
                let delta = if pointer.scroll > 0.0 { step } else { -step };
                action = Some(slider_action(index, (value + delta).clamp(0.0, max), true));
            }
        }

        let ratio = (shown / max).clamp(0.0, 1.0);
        if enabled {
            let filled = Rect::from_min_max(bar.min, Pos2::new(16.0 + span * ratio, bar.max.y));
            painter.rect_filled(filled, CornerRadius::same(3), ACCENT);
            painter.circle_filled(Pos2::new(filled.max.x, bar.center().y), 4.0, ACCENT);
        }
    }

    painter.text(
        Pos2::new(w / 2.0, 244.0),
        Align2::CENTER_CENTER,
        if controls.has_brightness() {
            "drag or scroll · brightness is a gamma dim, not backlight"
        } else {
            "drag or scroll to adjust"
        },
        tiny(),
        FAINT,
    );

    action
}

fn slider_action(index: u8, value: f32, force: bool) -> Action {
    if index == 0 {
        Action::Volume { value, force }
    } else {
        Action::Brightness { value, force }
    }
}

fn focus_panel(painter: &egui::Painter, view: &View<'_>, pointer: &Pointer) -> Option<Action> {
    let w = view.w;
    let timer = view.timer;
    let mut action = None;

    painter.text(
        Pos2::new(w / 2.0, 96.0),
        Align2::CENTER_CENTER,
        timer.clock(),
        FontId::new(42.0, egui::FontFamily::Proportional),
        if timer.running() { TEXT } else { MUTED },
    );
    painter.text(
        Pos2::new(w / 2.0, 130.0),
        Align2::CENTER_CENTER,
        match timer.mode() {
            focus::Mode::Focus => "Focus session",
            focus::Mode::Break => "Break",
        },
        sub_font(),
        FAINT,
    );

    let bar = Rect::from_min_max(Pos2::new(16.0, 152.0), Pos2::new(w - 16.0, 156.0));
    painter.rect_filled(bar, CornerRadius::same(2), TRACK);
    let progress = timer.progress();
    if progress > 0.0 {
        painter.rect_filled(
            Rect::from_min_max(bar.min, Pos2::new(16.0 + (w - 32.0) * progress, 156.0)),
            CornerRadius::same(2),
            ACCENT,
        );
    }

    let chips_top = 174.0;
    let mode_focus =
        Rect::from_center_size(Pos2::new(w / 2.0 - 62.0, chips_top), Vec2::new(116.0, 26.0));
    let mode_break =
        Rect::from_center_size(Pos2::new(w / 2.0 + 62.0, chips_top), Vec2::new(108.0, 26.0));
    let toggle = Rect::from_center_size(
        Pos2::new(w / 2.0 - 62.0, chips_top + 38.0),
        Vec2::new(116.0, 26.0),
    );
    let reset = Rect::from_center_size(
        Pos2::new(w / 2.0 + 62.0, chips_top + 38.0),
        Vec2::new(108.0, 26.0),
    );

    chip(
        painter,
        mode_focus,
        focus::Mode::Focus.label(),
        timer.mode() == focus::Mode::Focus,
        pointer.at.is_some_and(|p| mode_focus.contains(p)),
        FAINT,
    );
    chip(
        painter,
        mode_break,
        focus::Mode::Break.label(),
        timer.mode() == focus::Mode::Break,
        pointer.at.is_some_and(|p| mode_break.contains(p)),
        FAINT,
    );
    chip(
        painter,
        toggle,
        if timer.running() { "Pause" } else { "Start" },
        timer.running(),
        pointer.at.is_some_and(|p| toggle.contains(p)),
        ACCENT,
    );
    chip(
        painter,
        reset,
        "Reset",
        false,
        pointer.at.is_some_and(|p| reset.contains(p)),
        MUTED,
    );

    if pointer.live
        && pointer.clicked
        && let Some(p) = pointer.at
    {
        if mode_focus.contains(p) {
            action = Some(Action::Timer(focus::Action::Set(focus::Mode::Focus)));
        } else if mode_break.contains(p) {
            action = Some(Action::Timer(focus::Action::Set(focus::Mode::Break)));
        } else if toggle.contains(p) {
            action = Some(Action::Timer(focus::Action::Toggle));
        } else if reset.contains(p) {
            action = Some(Action::Timer(focus::Action::Reset));
        }
    }

    if timer.completed() > 0 {
        let dots = timer.completed().min(8) as usize;
        let start = w / 2.0 - (dots as f32 - 1.0) * 6.0;
        for i in 0..dots {
            painter.circle_filled(Pos2::new(start + i as f32 * 12.0, 256.0), 2.5, ACCENT);
        }
    }

    action
}

fn shelf_panel(
    painter: &egui::Painter,
    view: &View<'_>,
    pointer: &Pointer,
    state: &Interactions,
) -> Option<Action> {
    let w = view.w;
    let shelf = view.shelf;
    let mut action = None;
    paging(painter, state.page, shelf.items.len(), pointer);

    if shelf.items.is_empty() {
        painter.text(
            Pos2::new(w / 2.0, 118.0),
            Align2::CENTER_CENTER,
            "Drop files onto the notch",
            sub_font(),
            MUTED,
        );
        painter.text(
            Pos2::new(w / 2.0, 140.0),
            Align2::CENTER_CENTER,
            "they stay pinned here until you clear them",
            tiny(),
            FAINT,
        );
        return None;
    }

    for (index, item) in shelf
        .items
        .iter()
        .enumerate()
        .skip(state.page * PAGE_SIZE)
        .take(PAGE_SIZE)
    {
        let top = 46.0 + (index % PAGE_SIZE) as f32 * 34.0;
        let row = Rect::from_min_max(Pos2::new(12.0, top), Pos2::new(w - 12.0, top + 30.0));
        if pointer.at.is_some_and(|p| row.contains(p)) {
            painter.rect_filled(row, CornerRadius::same(7), CHIP);
        }
        painter.text(
            Pos2::new(18.0, top + 8.0),
            Align2::LEFT_TOP,
            clip(painter, &item.name, &sub_font(), w - 190.0),
            sub_font(),
            TEXT,
        );
        painter.text(
            Pos2::new(18.0, top + 22.0),
            Align2::LEFT_TOP,
            shelf::human_size(item.size, item.dir),
            tiny(),
            FAINT,
        );

        let open = Rect::from_min_size(Pos2::new(w - 108.0, top + 5.0), Vec2::new(44.0, 20.0));
        let copy = Rect::from_min_size(Pos2::new(w - 58.0, top + 5.0), Vec2::new(44.0, 20.0));
        small_button(
            painter,
            open,
            "Open",
            pointer.at.is_some_and(|p| open.contains(p)),
        );
        small_button(
            painter,
            copy,
            "Copy",
            pointer.at.is_some_and(|p| copy.contains(p)),
        );

        if pointer.live
            && pointer.clicked
            && let Some(p) = pointer.at
        {
            if open.contains(p) {
                action = Some(Action::Shelf(shelf::Action::Open(index)));
            } else if copy.contains(p) {
                action = Some(Action::Shelf(shelf::Action::Copy(index)));
            }
        }
    }

    let clear = Rect::from_min_size(Pos2::new(w - 84.0, 250.0), Vec2::new(70.0, 20.0));
    small_button(
        painter,
        clear,
        "Clear all",
        pointer.at.is_some_and(|p| clear.contains(p)),
    );
    if pointer.live && pointer.clicked && pointer.at.is_some_and(|p| clear.contains(p)) {
        action = Some(Action::Shelf(shelf::Action::Clear));
    }

    action
}

fn clips_panel(
    painter: &egui::Painter,
    view: &View<'_>,
    pointer: &Pointer,
    state: &Interactions,
) -> Option<Action> {
    let w = view.w;
    let clips = view.clips;
    let mut action = None;

    for (rect, label, selected) in [
        (
            Rect::from_min_size(Pos2::new(16.0, 34.0), Vec2::new(73.0, 24.0)),
            "History",
            !state.notes,
        ),
        (
            Rect::from_min_size(Pos2::new(97.0, 34.0), Vec2::new(59.0, 24.0)),
            "Notes",
            state.notes,
        ),
    ] {
        chip(
            painter,
            rect,
            label,
            selected,
            pointer.at.is_some_and(|p| rect.contains(p)),
            ACCENT,
        );
    }
    if state.notes {
        notes_panel(painter, view, pointer, state);
        return None;
    }
    paging(painter, state.page, clips.items.len(), pointer);

    if !clips.available || clips.items.is_empty() {
        painter.text(
            Pos2::new(w / 2.0, 118.0),
            Align2::CENTER_CENTER,
            if clips.available {
                "Copy some text anywhere and it lands here"
            } else {
                &clips.status
            },
            sub_font(),
            MUTED,
        );
        return None;
    }

    let ordered = clips.ordered();
    for (index, entry) in ordered
        .iter()
        .enumerate()
        .skip(state.page * PAGE_SIZE)
        .take(PAGE_SIZE)
    {
        let top = 68.0 + (index % PAGE_SIZE) as f32 * 34.0;
        let row = Rect::from_min_max(Pos2::new(12.0, top), Pos2::new(w - 12.0, top + 30.0));
        if pointer.at.is_some_and(|p| row.contains(p)) {
            painter.rect_filled(row, CornerRadius::same(7), CHIP);
        }
        let preview = entry.text.split_whitespace().collect::<Vec<_>>().join(" ");
        painter.text(
            Pos2::new(18.0, top + 8.0),
            Align2::LEFT_TOP,
            clip(painter, &preview, &sub_font(), w - 180.0),
            sub_font(),
            if entry.pinned { ACCENT } else { TEXT },
        );
        painter.text(
            Pos2::new(18.0, top + 22.0),
            Align2::LEFT_TOP,
            if entry.pinned { "pinned" } else { "clipboard" },
            tiny(),
            FAINT,
        );

        let copy = Rect::from_min_size(Pos2::new(w - 132.0, top + 5.0), Vec2::new(46.0, 20.0));
        let pin = Rect::from_min_size(Pos2::new(w - 82.0, top + 5.0), Vec2::new(46.0, 20.0));
        let remove = Rect::from_min_size(Pos2::new(w - 32.0, top + 5.0), Vec2::new(20.0, 20.0));
        small_button(
            painter,
            copy,
            "Copy",
            pointer.at.is_some_and(|p| copy.contains(p)),
        );
        small_button(
            painter,
            pin,
            if entry.pinned { "Unpin" } else { "Pin" },
            pointer.at.is_some_and(|p| pin.contains(p)),
        );
        small_button(
            painter,
            remove,
            "×",
            pointer.at.is_some_and(|p| remove.contains(p)),
        );

        if pointer.live
            && pointer.clicked
            && let Some(p) = pointer.at
        {
            if copy.contains(p) {
                action = Some(Action::Clips(crate::clipboard::Action::Copy(
                    entry.text.clone(),
                )));
            } else if pin.contains(p) {
                action = Some(Action::Clips(crate::clipboard::Action::TogglePin(
                    entry.text.clone(),
                )));
            } else if remove.contains(p) {
                action = Some(Action::Clips(crate::clipboard::Action::Remove(
                    entry.text.clone(),
                )));
            }
        }
    }

    let clear = Rect::from_min_size(Pos2::new(w - 84.0, 250.0), Vec2::new(70.0, 20.0));
    small_button(
        painter,
        clear,
        "Clear all",
        pointer.at.is_some_and(|p| clear.contains(p)),
    );
    if pointer.live && pointer.clicked && pointer.at.is_some_and(|p| clear.contains(p)) {
        action = Some(Action::Clips(crate::clipboard::Action::Clear));
    }

    action
}

fn notes_panel(painter: &egui::Painter, view: &View<'_>, pointer: &Pointer, state: &Interactions) {
    let w = view.w;
    let new = Rect::from_min_size(Pos2::new(w - 96.0, 34.0), Vec2::new(80.0, 24.0));
    small_button(
        painter,
        new,
        "New note",
        pointer.at.is_some_and(|p| new.contains(p)),
    );
    if state.editor.is_some() {
        return;
    }
    if view.notes.items.is_empty() {
        painter.text(
            Pos2::new(w / 2.0, 118.0),
            Align2::CENTER_CENTER,
            "No quick notes yet. Choose New note.",
            sub_font(),
            MUTED,
        );
    }
    for (index, note) in view
        .notes
        .items
        .iter()
        .enumerate()
        .skip(state.page * PAGE_SIZE)
        .take(PAGE_SIZE)
    {
        let top = 68.0 + (index % PAGE_SIZE) as f32 * 34.0;
        let row = Rect::from_min_max(Pos2::new(12.0, top), Pos2::new(w - 12.0, top + 30.0));
        if pointer.at.is_some_and(|p| row.contains(p)) {
            painter.rect_filled(row, CornerRadius::same(7), CHIP);
        }
        let preview = note.text.split_whitespace().collect::<Vec<_>>().join(" ");
        painter.text(
            Pos2::new(18.0, top + 8.0),
            Align2::LEFT_TOP,
            clip(painter, &preview, &sub_font(), w - 166.0),
            sub_font(),
            TEXT,
        );
        for (x, width, label) in [
            (w - 132.0, 46.0, "Copy"),
            (w - 82.0, 46.0, "Edit"),
            (w - 32.0, 20.0, "×"),
        ] {
            let rect = Rect::from_min_size(Pos2::new(x, top + 5.0), Vec2::new(width, 20.0));
            small_button(
                painter,
                rect,
                label,
                pointer.at.is_some_and(|p| rect.contains(p)),
            );
        }
    }
    if let Some(error) = &view.notes.error {
        painter.text(
            Pos2::new(18.0, 238.0),
            Align2::LEFT_CENTER,
            clip(painter, error, &tiny(), w - 36.0),
            tiny(),
            TEXT,
        );
    }
    paging(painter, state.page, view.notes.items.len(), pointer);
}

fn paging(painter: &egui::Painter, page: usize, count: usize, pointer: &Pointer) {
    if count <= PAGE_SIZE {
        return;
    }
    if page > 0 {
        let rect = Rect::from_min_size(Pos2::new(16.0, 250.0), Vec2::new(46.0, 20.0));
        small_button(
            painter,
            rect,
            "Prev",
            pointer.at.is_some_and(|p| rect.contains(p)),
        );
    }
    painter.text(
        Pos2::new(108.0, 260.0),
        Align2::CENTER_CENTER,
        format!("{} / {}", page + 1, count.div_ceil(PAGE_SIZE)),
        tiny(),
        FAINT,
    );
    if (page + 1) * PAGE_SIZE < count {
        let rect = Rect::from_min_size(Pos2::new(154.0, 250.0), Vec2::new(46.0, 20.0));
        small_button(
            painter,
            rect,
            "Next",
            pointer.at.is_some_and(|p| rect.contains(p)),
        );
    }
}

fn chip(
    painter: &egui::Painter,
    rect: Rect,
    label: &str,
    filled: bool,
    hovered: bool,
    accent: Color32,
) {
    painter.rect_filled(
        rect,
        CornerRadius::same(7),
        if filled {
            Color32::from_rgb(30, 34, 42)
        } else if hovered {
            CHIP
        } else {
            TRACK
        },
    );
    if filled {
        painter.rect_stroke(
            rect,
            CornerRadius::same(7),
            Stroke::new(1.0, accent),
            StrokeKind::Inside,
        );
    }
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        tiny(),
        if filled || hovered { TEXT } else { MUTED },
    );
}

fn small_button(painter: &egui::Painter, rect: Rect, label: &str, hovered: bool) {
    painter.rect_filled(
        rect,
        CornerRadius::same(6),
        if hovered { CHIP } else { TRACK },
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::new(10.0, egui::FontFamily::Proportional),
        if hovered { TEXT } else { MUTED },
    );
}

pub fn shows_mascot(track: &Track) -> bool {
    track.title.trim().is_empty() || track.status == crate::media::Status::Stopped
}

fn collapsed(painter: &egui::Painter, view: &View<'_>) {
    let View {
        w,
        h,
        track,
        artwork,
        seconds,
        ..
    } = *view;
    if shows_mascot(track) {
        mascot::draw(painter, Pos2::new(w / 2.0, h / 2.0), seconds);
        return;
    }

    if let Some(texture) = artwork.texture() {
        let cover = Rect::from_center_size(Pos2::new(25.0, h / 2.0), Vec2::splat(23.0));
        painter.image(
            texture.id(),
            cover,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    } else {
        let cover = Rect::from_center_size(Pos2::new(25.0, h / 2.0), Vec2::splat(21.0));
        painter.rect_filled(cover, CornerRadius::same(5), track_fallback_color(track));
        painter.text(
            cover.center(),
            Align2::CENTER_CENTER,
            track_monogram(track),
            FontId::new(12.0, egui::FontFamily::Proportional),
            TEXT,
        );
    }

    let font = FontId::new(13.0, egui::FontFamily::Proportional);
    painter.text(
        Pos2::new(w / 2.0 + 8.0, h / 2.0),
        Align2::CENTER_CENTER,
        clip(painter, &track.title, &font, w - 50.0),
        font,
        track_accent(track, artwork),
    );
}

fn track_monogram(track: &Track) -> String {
    track
        .title
        .chars()
        .find(|character| !character.is_whitespace())
        .map(|character| character.to_uppercase().collect())
        .unwrap_or_else(|| "♪".to_owned())
}

fn track_fallback_color(track: &Track) -> Color32 {
    if track.title.trim().is_empty() {
        Color32::from_rgb(24, 28, 36)
    } else {
        egui::ecolor::Hsva::new(track_hue(track), 0.60, 0.27, 1.0).into()
    }
}

fn track_accent(track: &Track, artwork: &crate::artwork::Artwork) -> Color32 {
    if track.title.trim().is_empty() {
        return FAINT;
    }
    artwork
        .accent()
        .unwrap_or_else(|| crate::artwork::readable_accent(track_hue(track), 0.72))
}

fn track_hue(track: &Track) -> f32 {
    let seed = track
        .title
        .bytes()
        .chain(track.artist.bytes())
        .fold(0x811c9dc5_u32, |hash, byte| {
            (hash ^ u32::from(byte)).wrapping_mul(0x01000193)
        });
    (seed % 360) as f32 / 360.0
}

fn transport_centers(w: f32) -> (f32, f32, f32) {
    let cx = w / 2.0;
    let step = 44.0;
    (cx - step, cx, cx + step)
}

fn play_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let s = 9.0;
    painter.add(Shape::convex_polygon(
        vec![
            Pos2::new(c.x - s * 0.45, c.y - s),
            Pos2::new(c.x - s * 0.45, c.y + s),
            Pos2::new(c.x + s * 0.85, c.y),
        ],
        color,
        Stroke::NONE,
    ));
}

fn pause_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let h = 9.0;
    for dx in [-3.5, 2.0] {
        painter.rect_filled(
            Rect::from_min_size(Pos2::new(c.x + dx, c.y - h), Vec2::new(3.0, h * 2.0)),
            CornerRadius::same(1),
            color,
        );
    }
}

fn skip_icon(painter: &egui::Painter, c: Pos2, color: Color32, back: bool, enabled: bool) {
    let color = if enabled {
        color
    } else {
        Color32::from_rgb(52, 52, 62)
    };
    let s = 7.5;
    let dir = if back { -1.0 } else { 1.0 };
    let bar_x = c.x + dir * (s + 2.0);
    painter.rect_filled(
        Rect::from_min_size(Pos2::new(bar_x - 1.0, c.y - s), Vec2::new(2.0, s * 2.0)),
        CornerRadius::same(1),
        color,
    );
    painter.add(Shape::convex_polygon(
        vec![
            Pos2::new(c.x - dir * s * 0.65, c.y - s),
            Pos2::new(c.x - dir * s * 0.65, c.y + s),
            Pos2::new(c.x + dir * s * 0.8, c.y),
        ],
        color,
        Stroke::NONE,
    ));
}

fn text_width(painter: &egui::Painter, text: &str, font: &FontId) -> f32 {
    painter
        .layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE)
        .size()
        .x
}

/// Truncate with an ellipsis so text never runs under the notch edge.
fn clip(painter: &egui::Painter, text: &str, font: &FontId, max_w: f32) -> String {
    if text.is_empty() {
        return String::new();
    }
    if text_width(painter, text, font) <= max_w {
        return text.to_owned();
    }
    let mut cut: String = text.to_owned();
    while !cut.is_empty() && text_width(painter, &format!("{cut}…"), font) > max_w {
        cut.pop();
    }
    format!("{cut}…")
}

fn clock(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

/// Recording-free smoke check: a fake track so the UI can be exercised without
/// a media player running.
pub fn demo_track(elapsed: f64) -> Track {
    let length = 214.0;
    Track {
        player: "demo".into(),
        art_url: None,
        title: "Sample track (demo)".into(),
        artist: "Glacier".into(),
        album: "Demo album".into(),
        status: crate::media::Status::Playing,
        position: elapsed % length,
        length: Some(length),
        can_next: true,
        can_prev: true,
        can_seek: true,
        can_control: true,
        rate: 1.0,
        track_id: None,
    }
}

#[cfg(test)]
mod track_display_tests {
    use super::*;

    #[test]
    fn fallback_monogram_uses_title_and_handles_empty_metadata() {
        let track = Track {
            title: "  Yellow Submarine".into(),
            ..Track::default()
        };
        assert_eq!(track_monogram(&track), "Y");
        assert_eq!(track_monogram(&Track::default()), "♪");
    }

    #[test]
    fn fallback_color_is_stable_and_track_specific() {
        let first = Track {
            title: "Yellow Submarine".into(),
            artist: "Beatles".into(),
            ..Track::default()
        };
        let same = first.clone();
        let other = Track {
            title: "Blue Train".into(),
            artist: "Coltrane".into(),
            ..Track::default()
        };
        assert_eq!(track_fallback_color(&first), track_fallback_color(&same));
        assert_ne!(track_fallback_color(&first), track_fallback_color(&other));
    }

    #[test]
    fn transport_controls_are_centered_as_a_group() {
        let (previous, play, next) = transport_centers(W_EXPANDED);
        let middle = W_EXPANDED / 2.0;
        assert_eq!(play, middle);
        assert!(((previous + next) / 2.0 - middle).abs() < 0.001);
        assert!(previous < play && play < next);
    }
}
