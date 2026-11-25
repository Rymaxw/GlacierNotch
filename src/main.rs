//! Glacier for Linux: a top-center notch strip that expands on hover into a
//! tabbed panel (music, system controls, focus timer, file shelf, clipboard).
//! eframe/egui + MPRIS over D-Bus, X11 SHAPE for click-through.

mod artwork;
mod clipboard;
mod controls;
mod focus;
mod lyrics;
mod media;
mod settings_ui;
mod shelf;
mod tray;
mod ui;
mod x11_overlay;

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use eframe::egui;
use glacier::autostart;
use glacier::config::{Config, ConfigWatcher, IdleMode};
use glacier::notes::{self, Notes};
use media::{Cmd, Media, PlaybackClock, Track};

struct Glacier {
    config: Config,
    config_watcher: ConfigWatcher,
    tray: tray::Tray,
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
    hidden: bool,
    hover_enter_at: Option<Instant>,
    hover_leave_at: Option<Instant>,
    hovering_files: bool,
    editing_focus: bool,
    idle_clock: String,
    last_clock_check: Instant,
}

impl Glacier {
    fn new(demo: bool, cc: &eframe::CreationContext<'_>) -> Self {
        let config = Config::load();
        let config_watcher = ConfigWatcher::new(Config::path());
        let input_shape = match x11_overlay::InputShape::new(cc) {
            Ok(shape) => Some(shape),
            Err(error) => {
                eprintln!("Glacier needs X11 SHAPE input support: {error}");
                None
            }
        };

        let mut lyrics = lyrics::Lyrics::new();
        lyrics.set_config(
            config.features.music && config.music.show_lyrics,
            config.music.online_lrclib,
        );

        let mut controls = controls::Controls::new();
        controls.apply_config(
            config.features.volume_enabled(),
            config.features.brightness_enabled(),
            config.controls.volume_max,
            config.controls.brightness_min,
            config.controls.xrandr_output.as_deref(),
        );

        let mut timer = focus::Timer::default();
        timer.apply_config(
            config.focus.focus_minutes,
            config.focus.break_minutes,
            config.focus.notifications,
            config.focus.auto_start_next,
        );

        let mut shelf = shelf::Shelf::default();
        shelf.configure(config.shelf.max_items, config.shelf.persist);

        let mut interactions = ui::Interactions::default();
        interactions.sync_clips_mode(config.features.clipboard, config.features.notes);

        let tab = config
            .features
            .resolve_tab(config.features.default_tab)
            .unwrap_or(ui::Tab::Music);

        let media = Media::start_with_config(
            demo,
            config.features.music,
            config.music.ignored_players.clone(),
        );
        let clips = clipboard::Clips::start_with_config(
            config.features.clipboard,
            config.clipboard.max_clips,
        );
        let tray = tray::Tray::new(config.system.tray_icon);
        let idle_clock = current_clock_hhmm();

        Self {
            config,
            config_watcher,
            tray,
            media,
            lyrics,
            artwork: artwork::Artwork::new(),
            track: Track::default(),
            playback_clock: PlaybackClock::new(Instant::now()),
            controls,
            timer,
            shelf,
            clips,
            notes: Notes::default(),
            tab,
            interactions,
            demo,
            started: Instant::now(),
            open: 0.0,
            size: egui::vec2(ui::W_COLLAPSED, ui::H_COLLAPSED),
            monitor: None,
            last_sent: None,

            input_shape,
            expanded: false,
            hidden: false,
            hover_enter_at: None,
            hover_leave_at: None,
            hovering_files: false,
            editing_focus: false,
            idle_clock,
            last_clock_check: Instant::now(),
        }
    }

    fn sync_focus(&mut self, ctx: &egui::Context) {
        let editing = self.interactions.editing();
        let clicked_while_editing = editing && ctx.input(|i| i.pointer.any_pressed());
        if editing && (!self.editing_focus || clicked_while_editing) {
            if let Some(shape) = &mut self.input_shape
                && let Err(error) = shape.acquire_focus()
            {
                eprintln!("Glacier focus acquire failed: {error}");
            }
            if !self.editing_focus {
                ctx.request_repaint();
            }
            self.editing_focus = true;
        } else if !editing && self.editing_focus {
            self.release_editing_focus();
            ctx.request_repaint();
        }
    }

    fn release_editing_focus(&mut self) {
        if let Some(shape) = &mut self.input_shape
            && let Err(error) = shape.release_focus()
        {
            eprintln!("Glacier focus release failed: {error}");
        }
        self.editing_focus = false;
    }

    fn apply_config(&mut self, next: Config) {
        let prev = std::mem::replace(&mut self.config, next);

        if prev.features.music != self.config.features.music
            || prev.music.ignored_players != self.config.music.ignored_players
        {
            if !self.config.features.music {
                self.track = Track::default();
            }
            if !self.demo {
                self.media.send(Cmd::Configure {
                    enabled: self.config.features.music,
                    ignored_players: self.config.music.ignored_players.clone(),
                });
            }
        }

        self.lyrics.set_config(
            self.config.features.music && self.config.music.show_lyrics,
            self.config.music.online_lrclib,
        );

        self.controls.apply_config(
            self.config.features.volume_enabled(),
            self.config.features.brightness_enabled(),
            self.config.controls.volume_max,
            self.config.controls.brightness_min,
            self.config.controls.xrandr_output.as_deref(),
        );

        self.timer.apply_config(
            self.config.focus.focus_minutes,
            self.config.focus.break_minutes,
            self.config.focus.notifications,
            self.config.focus.auto_start_next,
        );

        self.shelf
            .configure(self.config.shelf.max_items, self.config.shelf.persist);

        if prev.features.clipboard != self.config.features.clipboard
            || prev.clipboard.max_clips != self.config.clipboard.max_clips
        {
            self.clips.act(clipboard::Action::Configure {
                enabled: self.config.features.clipboard,
                max_clips: self.config.clipboard.max_clips,
            });
        }

        self.tray.set_enabled(self.config.system.tray_icon);
        self.interactions
            .sync_clips_mode(self.config.features.clipboard, self.config.features.notes);

        if let Some(resolved) = self.config.features.resolve_tab(self.tab)
            && resolved != self.tab
        {
            self.tab = resolved;
            self.interactions.reset_page();
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
            ui::Action::OpenSettings => {
                settings_ui::spawn_detached();
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
        if let Some(updated) = self.config_watcher.poll() {
            self.apply_config(updated);
        }
        while let Some(event) = self.tray.poll() {
            match event {
                tray::TrayEvent::OpenSettings => settings_ui::spawn_detached(),
                tray::TrayEvent::ToggleNotch => {
                    self.hidden = !self.hidden;
                    if self.hidden {
                        self.release_editing_focus();
                    }
                    self.tray.set_notch_visible(!self.hidden);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(!self.hidden));
                }
                tray::TrayEvent::RestartService => {
                    self.release_editing_focus();
                    self.controls.restore_brightness();
                    let _ = Command::new("systemctl")
                        .args(["--user", "restart", "glaciernotch.service"])
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn();
                }
                tray::TrayEvent::Quit => {
                    self.release_editing_focus();
                    self.controls.restore_brightness();
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }

        let escaped_editor = self.interactions.begin_frame(&ctx);
        let dt = ctx.input(|i| i.stable_dt).clamp(0.001, 0.1) as f64;

        if self.config.mascot.idle_mode == IdleMode::Clock
            && self.last_clock_check.elapsed() >= Duration::from_secs(10)
        {
            self.idle_clock = current_clock_hhmm();
            self.last_clock_check = Instant::now();
        }

        if self.config.features.music {
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
        } else {
            self.track = Track::default();
        }
        self.controls.refresh();
        self.clips.poll();
        if self.config.features.focus && self.timer.tick(dt).is_some() {
            // The notification already fired; keep the new mode visible.
        }

        let dropped: Vec<std::path::PathBuf> = if self.config.features.shelf_drop_enabled() {
            ctx.input(|i| {
                i.raw
                    .dropped_files
                    .iter()
                    .map(|file| file.path().to_path_buf())
                    .collect()
            })
        } else {
            Vec::new()
        };
        if !dropped.is_empty() {
            self.shelf.add(dropped);
            self.act(ui::Action::Tab(ui::Tab::Shelf));
        }
        self.hovering_files = self.config.features.shelf_drop_enabled()
            && ctx.input(|i| !i.raw.hovered_files.is_empty());

        let mut display_track = self.track.clone();
        if !self.demo && display_track.is_playing() {
            display_track.position = self.playback_clock.position(Instant::now());
            if let Some(length) = display_track.length {
                display_track.position = display_track.position.min(length);
            }
        }

        let escape_pressed = ctx.input(|i| i.key_pressed(egui::Key::Escape) && !escaped_editor);
        let ctrl_q = ctx.input(|i| i.key_pressed(egui::Key::Q) && i.modifiers.ctrl);
        if ctrl_q || (escape_pressed && self.config.appearance.escape_quits) {
            self.release_editing_focus();
            self.controls.restore_brightness();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if escape_pressed {
            self.interactions.clear_keyboard_mode();
        }

        if self.config.appearance.tab_hotkeys && !self.interactions.editing() {
            let enabled_tabs = self.config.features.enabled_tabs();
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
                if let Some(&tab) = enabled_tabs.get(index)
                    && ctx.input(|i| i.key_pressed(key))
                {
                    self.act(ui::Action::Tab(tab));
                }
            }
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight) && i.modifiers.alt)
                && let Some(next) = self.config.features.step_tab(self.tab, true)
            {
                self.act(ui::Action::Tab(next));
            }
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft) && i.modifiers.alt)
                && let Some(prev) = self.config.features.step_tab(self.tab, false)
            {
                self.act(ui::Action::Tab(prev));
            }
        }

        // Hover drives the notch open/closed, with optional open/close delays
        // and a short ease so it feels like a flap rather than a resize.
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        let notch = egui::Rect::from_min_max(
            egui::pos2((ui::W_EXPANDED - ui::W_COLLAPSED) / 2.0, 0.0),
            egui::pos2((ui::W_EXPANDED + ui::W_COLLAPSED) / 2.0, ui::H_COLLAPSED),
        );
        let full =
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(ui::W_EXPANDED, ui::H_EXPANDED));
        let force_open = self.interactions.holds_open() || self.hovering_files;
        let pointer_over = pointer.is_some_and(|p| {
            if self.expanded {
                full.contains(p)
            } else {
                notch.contains(p)
            }
        });
        let raw_hovering = force_open || pointer_over;
        let hovering = if raw_hovering == self.expanded {
            self.hover_enter_at = None;
            self.hover_leave_at = None;
            self.expanded
        } else if raw_hovering {
            self.hover_leave_at = None;
            let delay =
                Duration::from_millis(u64::from(self.config.appearance.hover_open_delay_ms));
            if force_open || delay.is_zero() {
                self.hover_enter_at = None;
                true
            } else {
                let start = *self.hover_enter_at.get_or_insert_with(Instant::now);
                start.elapsed() >= delay
            }
        } else {
            self.hover_enter_at = None;
            let delay =
                Duration::from_millis(u64::from(self.config.appearance.hover_close_delay_ms));
            if delay.is_zero() {
                self.hover_leave_at = None;
                false
            } else {
                let start = *self.hover_leave_at.get_or_insert_with(Instant::now);
                start.elapsed() < delay
            }
        };

        if hovering != self.expanded {
            if let Some(shape) = &mut self.input_shape
                && let Err(error) = shape.set(hovering)
            {
                eprintln!("Glacier input shape update failed: {error}");
            }
            self.expanded = hovering;
            self.hover_enter_at = None;
            self.hover_leave_at = None;
        }
        let target_open = if hovering { 1.0 } else { 0.0 };
        if self.config.appearance.reduce_motion {
            self.open = target_open;
        } else {
            let blend = 1.0 - (-dt as f32 * 14.0).exp();
            self.open += (target_open - self.open) * blend;
            if self.open < 0.002 {
                self.open = 0.0;
            }
        }

        let target_w = ui::W_COLLAPSED + (ui::W_EXPANDED - ui::W_COLLAPSED) * self.open;
        let target_h = ui::H_COLLAPSED + (ui::H_EXPANDED - ui::H_COLLAPSED) * self.open;
        if self.config.appearance.reduce_motion {
            self.size = egui::vec2(target_w, target_h);
        } else {
            let blend = 1.0 - (-dt as f32 * 14.0).exp();
            self.size.x += (target_w - self.size.x) * blend;
            self.size.y += (target_h - self.size.y) * blend;
        }

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
                config: &self.config,
                idle_clock: &self.idle_clock,
                hovering_files: self.hovering_files,
            },
        );
        if let Some(action) = action {
            self.act(action);
        }
        self.sync_focus(&ctx);

        let moving = (self.open - target_open).abs() > 0.003;
        let mascot_animating = self.config.mascot.idle_mode == IdleMode::Mascot
            && (self.config.mascot.animate || self.config.mascot.blink);
        let interval = if moving || self.timer.running() {
            16
        } else if self.open < ui::COLLAPSED_FADE
            && ui::shows_mascot(&display_track)
            && mascot_animating
        {
            40
        } else if self.expanded || self.hover_enter_at.is_some() || self.hover_leave_at.is_some() {
            50
        } else {
            120
        };
        ctx.request_repaint_after(Duration::from_millis(interval));
    }
}

fn current_clock_hhmm() -> String {
    Command::new("date")
        .arg("+%H:%M")
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "--:--".to_owned())
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

fn parse_settings_args() -> Option<settings_ui::Section> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.iter().any(|arg| arg == "--settings") {
        return None;
    }
    let mut section = settings_ui::Section::Features;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--tab" {
            if let Some(value) = iter.next()
                && let Some(parsed) = settings_ui::Section::from_arg(value)
            {
                section = parsed;
            }
        } else if let Some(value) = arg.strip_prefix("--tab=")
            && let Some(parsed) = settings_ui::Section::from_arg(value)
        {
            section = parsed;
        }
    }
    Some(section)
}

fn main() -> eframe::Result<()> {
    if let Err(error) = handle_autostart_args() {
        eprintln!("Glacier autostart: {error}");
        std::process::exit(1);
    }
    if let Some(section) = parse_settings_args() {
        return settings_ui::run(section);
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
pub(crate) fn install_fonts(ctx: &egui::Context) {
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
