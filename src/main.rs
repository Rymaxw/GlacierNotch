//! Glacier for Linux: a top-center notch strip that expands on hover into a
//! tabbed panel (music, system controls, focus timer, file shelf, clipboard).
//! eframe/egui + MPRIS over D-Bus, X11 SHAPE for click-through.

mod artwork;
mod autostart;
mod clipboard;
mod controls;
mod focus;
mod lyrics;
mod media;
mod shelf;
mod ui;
mod x11_overlay;

use std::time::Instant;

use eframe::egui;
use glacier::notes::{self, Notes};
use media::{Cmd, Media, PlaybackClock, Track};

struct Glacier {
    media: Media,
    lyrics: lyrics::Lyrics,
    artwork: artwork::Artwork,
    track: Track,
    playback_clock: PlaybackClock,
    controls: controls::Controls,
    timer: focus::Timer,
    shelf: shelf::Shelf,
    clips: clipboard::Clips,
    notes: Notes,
    tab: ui::Tab,
    interactions: ui::Interactions,
    demo: bool,
    started: Instant,
    open: f32,
    size: egui::Vec2,
    monitor: Option<egui::Vec2>,
    last_sent: Option<(u32, u32, i32, i32)>,

    input_shape: Option<x11_overlay::InputShape>,
    expanded: bool,
    hovering_files: bool,
}

impl Glacier {
    fn new(demo: bool, cc: &eframe::CreationContext<'_>) -> Self {
        let input_shape = match x11_overlay::InputShape::new(cc) {
            Ok(shape) => Some(shape),
            Err(error) => {
                eprintln!("Glacier needs X11 SHAPE input support: {error}");
                None
            }
        };
        Self {
            media: Media::start(demo),
            lyrics: lyrics::Lyrics::new(),
            artwork: artwork::Artwork::new(),
            track: Track::default(),
            playback_clock: PlaybackClock::new(Instant::now()),
            controls: controls::Controls::new(),
            timer: focus::Timer::default(),
            shelf: shelf::Shelf::default(),
            clips: clipboard::Clips::start(),
            notes: Notes::default(),
            tab: ui::Tab::default(),
            interactions: ui::Interactions::default(),
            demo,
            started: Instant::now(),
            open: 0.0,
            size: egui::vec2(ui::W_COLLAPSED, ui::H_COLLAPSED),
            monitor: None,
            last_sent: None,

            input_shape,
            expanded: false,
            hovering_files: false,
        }
    }

    fn sync_window(&mut self, ctx: &egui::Context) {
        let monitor = *self.monitor.get_or_insert_with(|| {
            ctx.input(|i| i.viewport().monitor_size)
                .unwrap_or(egui::vec2(1920.0, 1080.0))
        });
        let width = ui::W_EXPANDED as u32;
        let height = ui::H_EXPANDED as u32;
        let x = ((monitor.x - ui::W_EXPANDED) / 2.0).round() as i32;
        let key = (width, height, x, 0);

        if self.last_sent != Some(key) {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                width as f32,
                height as f32,
            )));
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                x as f32, 0.0,
            )));
            self.last_sent = Some(key);
        }
    }

    /// Carry out whatever the notch asked for. Seek feedback is applied to the
    /// local clock immediately so the bar does not snap back while the player
    /// catches up.
    fn act(&mut self, action: ui::Action) {
        match action {
            ui::Action::Transport(cmd) => {
                match cmd {
                    Cmd::SeekTo(position) => {
                        self.playback_clock.jump_to(position, Instant::now());
                        self.track.position = position;
                    }
                    Cmd::SeekBy(delta) => {
                        let target = (self.track.position + delta).max(0.0);
                        self.playback_clock.jump_to(target, Instant::now());
                        self.track.position = target;
                    }
                    _ => {}
                }
                if !self.demo {
                    self.media.send(cmd);
                }
            }
            ui::Action::Seek(position) => {
                self.playback_clock.jump_to(position, Instant::now());
                self.track.position = position;
                if !self.demo {
                    self.media.send(Cmd::SeekTo(position));
                }
            }
            ui::Action::Rate(rate) => {
                self.track.rate = rate;
                if !self.demo {
                    self.media.send(Cmd::SetRate(rate));
                }
            }
            ui::Action::Volume { value, force } => self.controls.set_volume(value, force),
            ui::Action::Brightness { value, force } => self.controls.set_brightness(value, force),
            ui::Action::Timer(action) => self.timer.act(action),
            ui::Action::Shelf(action) => {
                if let Some(path) = self.shelf.act(action) {
                    self.clips.act(clipboard::Action::Copy(path));
                }
            }
            ui::Action::Clips(action) => self.clips.act(action),
            ui::Action::Ui(action) => self.interactions.act(action, &self.notes),
            ui::Action::Note(action) => {
                let saving = matches!(action, notes::Action::Add(_) | notes::Action::Edit { .. });
                if let Some(text) = self.notes.act(action) {
                    self.clips.act(clipboard::Action::Copy(text));
                }
                if saving && self.notes.error.is_none() {
                    self.interactions.note_saved();
                }
            }
            ui::Action::Tab(tab) => {
                self.tab = tab;
                self.interactions.reset_page();
            }
        }
    }
}

impl eframe::App for Glacier {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let escaped_editor = self.interactions.begin_frame(&ctx);
        let dt = ctx.input(|i| i.stable_dt).clamp(0.001, 0.1) as f64;

        if self.demo {
            self.track = ui::demo_track(self.started.elapsed().as_secs_f64());
        } else if let Some(track) = self.media.poll() {
            self.playback_clock.observe(&track, Instant::now());
            self.track = track;
        }
        self.lyrics
            .update_track(&self.track.title, &self.track.artist, self.track.length);
        self.lyrics.update_duration(self.track.length);
        self.lyrics.poll();
        self.artwork.update(&ctx, self.track.art_url.as_deref());
        self.controls.refresh();
        self.clips.poll();
        if self.timer.tick(dt).is_some() {
            // The notification already fired; keep the new mode visible.
        }

        let dropped: Vec<std::path::PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .collect()
        });
        if !dropped.is_empty() {
            self.shelf.add(dropped);
            self.act(ui::Action::Tab(ui::Tab::Shelf));
        }
        self.hovering_files = ctx.input(|i| !i.raw.hovered_files.is_empty());

        let mut display_track = self.track.clone();
        if !self.demo && display_track.is_playing() {
            display_track.position = self.playback_clock.position(Instant::now());
            if let Some(length) = display_track.length {
                display_track.position = display_track.position.min(length);
            }
        }

        let quit = ctx.input(|i| {
            (i.key_pressed(egui::Key::Escape) && !escaped_editor)
                || (i.key_pressed(egui::Key::Q) && i.modifiers.ctrl)
        });
        if quit {
            self.controls.restore_brightness();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        if !self.interactions.editing() {
            for (index, key) in [
                egui::Key::Num1,
                egui::Key::Num2,
                egui::Key::Num3,
                egui::Key::Num4,
                egui::Key::Num5,
            ]
            .into_iter()
            .enumerate()
            {
                if ctx.input(|i| i.key_pressed(key)) {
                    self.act(ui::Action::Tab(ui::Tab::ALL[index]));
                }
            }
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight) && i.modifiers.alt) {
                self.act(ui::Action::Tab(self.tab.step(true)));
            }
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft) && i.modifiers.alt) {
                self.act(ui::Action::Tab(self.tab.step(false)));
            }
        }

        // Hover drives the notch open/closed, with a short ease so it feels
        // like a flap rather than a resize.
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        let notch = egui::Rect::from_min_max(
            egui::pos2((ui::W_EXPANDED - ui::W_COLLAPSED) / 2.0, 0.0),
            egui::pos2((ui::W_EXPANDED + ui::W_COLLAPSED) / 2.0, ui::H_COLLAPSED),
        );
        let full =
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(ui::W_EXPANDED, ui::H_EXPANDED));
        let hovering = self.interactions.holds_open()
            || self.hovering_files
            || pointer.is_some_and(|p| {
                if self.expanded {
                    full.contains(p)
                } else {
                    notch.contains(p)
                }
            });
        if hovering != self.expanded {
            if let Some(shape) = &mut self.input_shape
                && let Err(error) = shape.set(hovering)
            {
                eprintln!("Glacier input shape update failed: {error}");
            }
            self.expanded = hovering;
        }
        let blend = 1.0 - (-dt as f32 * 14.0).exp();
        self.open += ((if hovering { 1.0 } else { 0.0 }) - self.open) * blend;
        if self.open < 0.002 {
            self.open = 0.0;
        }

        let target_w = ui::W_COLLAPSED + (ui::W_EXPANDED - ui::W_COLLAPSED) * self.open;
        let target_h = ui::H_COLLAPSED + (ui::H_EXPANDED - ui::H_COLLAPSED) * self.open;
        self.size.x += (target_w - self.size.x) * blend;
        self.size.y += (target_h - self.size.y) * blend;

        self.sync_window(&ctx);

        let action = ui::draw(
            ui,
            &mut self.interactions,
            &ui::View {
                w: self.size.x,
                h: self.size.y,
                open: self.open,
                seconds: self.started.elapsed().as_secs_f64(),
                tab: self.tab,
                track: &display_track,
                lyrics: &self.lyrics,
                controls: &self.controls,
                artwork: &self.artwork,
                timer: &self.timer,
                shelf: &self.shelf,
                clips: &self.clips,
                notes: &self.notes,
                hovering_files: self.hovering_files,
            },
        );
        if let Some(action) = action {
            self.act(action);
        }

        let moving = (self.open - if hovering { 1.0 } else { 0.0 }).abs() > 0.003;
        let interval = if moving || self.timer.running() {
            16
        } else if self.open < ui::COLLAPSED_FADE && ui::shows_mascot(&display_track) {
            40
        } else if self.expanded {
            50
        } else {
            120
        };
        ctx.request_repaint_after(std::time::Duration::from_millis(interval));
    }
}

fn handle_autostart_args() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let Some(flag) = args.next() else {
        return Ok(());
    };
    let command = if flag == "--autostart" {
        args.next().ok_or_else(|| {
            std::io::Error::other("usage: glacier --autostart enable|disable|status")
        })?
    } else if let Some(command) = flag.strip_prefix("--autostart=") {
        command.to_owned()
    } else {
        return Ok(());
    };
    if args.next().is_some() {
        return Err(std::io::Error::other(
            "usage: glacier --autostart enable|disable|status",
        ));
    }
    match command.as_str() {
        "enable" => {
            let path = autostart::enable()?;
            println!("Autostart enabled: {}", path.display());
            std::process::exit(0);
        }
        "disable" => {
            let path = autostart::disable()?;
            println!("Autostart disabled: {}", path.display());
            std::process::exit(0);
        }
        "status" => {
            println!(
                "Autostart {} ({})",
                if autostart::status() {
                    "enabled"
                } else {
                    "disabled"
                },
                autostart::entry_path().display()
            );
            std::process::exit(0);
        }
        _ => Err(std::io::Error::other(
            "usage: glacier --autostart enable|disable|status",
        )),
    }
}

fn main() -> eframe::Result<()> {
    if let Err(error) = handle_autostart_args() {
        eprintln!("Glacier autostart: {error}");
        std::process::exit(1);
    }
    let demo = std::env::args().any(|a| a == "--demo");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("GlacierNotch")
            .with_inner_size([ui::W_COLLAPSED, ui::H_COLLAPSED])
            .with_position([850.0, 0.0])
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_window_type(egui::X11WindowType::DropdownMenu)
            .with_override_redirect(true)
            .with_taskbar(false)
            .with_resizable(false)
            .with_active(false),
        ..Default::default()
    };

    eframe::run_native(
        "GlacierNotch",
        options,
        Box::new(move |cc| {
            install_fonts(&cc.egui_ctx);
            Ok(Box::new(Glacier::new(demo, cc)))
        }),
    )
}

/// DejaVu Sans carries the glyphs this UI uses (♪, …, ×) unlike egui's bundled
/// font.
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let candidates = [
        ("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", "dejavu"),
        (
            "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
            "dejavu-bold",
        ),
    ];
    for (path, name) in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert(
                name.to_owned(),
                std::sync::Arc::new(egui::FontData::from_owned(bytes)),
            );
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .push(name.to_owned());
        }
    }
    ctx.set_fonts(fonts);
}
