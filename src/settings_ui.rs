//! Standalone GlacierNotch Settings & Feature Manager window (`glacier --settings`).
//!
//! Runs as a normal decorated X11 `eframe/egui` app completely isolated from
//! the override-redirect notch overlay. Saves atomically to
//! `~/.config/glacier/config.json`, which the notch daemon hot-reloads live.

use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Margin, Pos2, Rect, Stroke, StrokeKind, Vec2,
};
use glacier::autostart;
use glacier::config::{Config, IdleMode, MascotForm};
use glacier::storage;
use zbus::blocking::{Connection, Proxy, connection::Builder};

use crate::controls;
use crate::tray;
use crate::ui::mascot::{self, MascotOptions};

const SETTINGS_BUS_NAME: &str = "org.glaciernotch.Settings";
const SETTINGS_OBJ_PATH: &str = "/org/glaciernotch/Settings";
const SERVICE_UNIT: &str = "glaciernotch.service";

const BG_BASE: Color32 = Color32::from_rgb(9, 9, 12);
const BG_SIDEBAR: Color32 = Color32::from_rgb(13, 13, 18);
const BG_CARD: Color32 = Color32::from_rgb(19, 19, 26);
const BG_WELL: Color32 = Color32::from_rgb(26, 26, 35);
const BORDER: Color32 = Color32::from_rgb(38, 38, 50);
const TEXT: Color32 = Color32::from_rgb(240, 240, 245);
const MUTED: Color32 = Color32::from_rgb(154, 154, 168);
const ACCENT: Color32 = Color32::from_rgb(122, 211, 255);
const GREEN: Color32 = Color32::from_rgb(94, 214, 138);
const AMBER: Color32 = Color32::from_rgb(245, 184, 76);
const RED: Color32 = Color32::from_rgb(240, 108, 108);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Section {
    #[default]
    Features,
    Appearance,
    Idle,
    Modules,
    System,
}

impl Section {
    const ALL: [Section; 5] = [
        Section::Features,
        Section::Appearance,
        Section::Idle,
        Section::Modules,
        Section::System,
    ];

    pub fn from_arg(name: &str) -> Option<Self> {
        match name.trim().to_lowercase().as_str() {
            "features" | "tabs" => Some(Section::Features),
            "appearance" | "notch" => Some(Section::Appearance),
            "idle" | "mascot" | "companion" => Some(Section::Idle),
            "modules" | "music" | "controls" | "focus" | "shelf" | "clips" => {
                Some(Section::Modules)
            }
            "system" | "service" | "about" => Some(Section::System),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Section::Features => "Features",
            Section::Appearance => "Appearance",
            Section::Idle => "Idle Companion",
            Section::Modules => "Modules",
            Section::System => "System & Service",
        }
    }

    fn subtitle(self) -> &'static str {
        match self {
            Section::Features => "Enable or disable notch modules and choose your default tab.",
            Section::Appearance => {
                "Tune hover timing, motion, accent color, and notch keyboard behavior."
            }
            Section::Idle => "Customize what the collapsed notch displays when no media is active.",
            Section::Modules => "Fine-tune Music, Lyrics, Controls, Focus Timer, Shelf, and Clips.",
            Section::System => {
                "Manage the GlacierNotch service, login autostart, tray icon, and config."
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ServiceState {
    Checking,
    Running,
    Stopped,
    Failed,
    Unknown,
}

impl ServiceState {
    fn label(self) -> &'static str {
        match self {
            ServiceState::Checking => "Checking...",
            ServiceState::Running => "Running",
            ServiceState::Stopped => "Stopped",
            ServiceState::Failed => "Failed",
            ServiceState::Unknown => "Unavailable",
        }
    }

    fn color(self) -> Color32 {
        match self {
            ServiceState::Running => GREEN,
            ServiceState::Stopped | ServiceState::Checking => AMBER,
            ServiceState::Failed => RED,
            ServiceState::Unknown => MUTED,
        }
    }
}

enum ServiceReq {
    Refresh,
    Start,
    Restart,
    Stop,
}

struct ServiceUpdate {
    state: ServiceState,
    message: Option<String>,
}

struct ServiceMonitor {
    tx: Sender<ServiceReq>,
    rx: Receiver<ServiceUpdate>,
    state: ServiceState,
    last_refresh: Instant,
    busy: bool,
    status_note: Option<String>,
}

impl ServiceMonitor {
    fn start() -> Self {
        let (tx, req_rx) = channel::<ServiceReq>();
        let (upd_tx, rx) = channel::<ServiceUpdate>();
        thread::Builder::new()
            .name("glacier-service-mon".into())
            .spawn(move || {
                while let Ok(req) = req_rx.recv() {
                    let message = match req {
                        ServiceReq::Refresh => None,
                        ServiceReq::Start => run_systemctl(&["start", SERVICE_UNIT]),
                        ServiceReq::Restart => run_systemctl(&["restart", SERVICE_UNIT]),
                        ServiceReq::Stop => run_systemctl(&["stop", SERVICE_UNIT]),
                    };
                    let state = query_service_state();
                    if upd_tx.send(ServiceUpdate { state, message }).is_err() {
                        break;
                    }
                }
            })
            .ok();
        let _ = tx.send(ServiceReq::Refresh);
        Self {
            tx,
            rx,
            state: ServiceState::Checking,
            last_refresh: Instant::now(),
            busy: false,
            status_note: None,
        }
    }

    fn poll(&mut self) {
        while let Ok(update) = self.rx.try_recv() {
            self.state = update.state;
            self.busy = false;
            if update.message.is_some() {
                self.status_note = update.message;
            }
        }
        if !self.busy && self.last_refresh.elapsed() >= Duration::from_secs(3) {
            self.last_refresh = Instant::now();
            let _ = self.tx.send(ServiceReq::Refresh);
        }
    }

    fn send(&mut self, req: ServiceReq) {
        self.busy = true;
        self.status_note = None;
        self.last_refresh = Instant::now();
        let _ = self.tx.send(req);
    }
}

fn query_service_state() -> ServiceState {
    let Ok(output) = Command::new("systemctl")
        .args(["--user", "is-active", SERVICE_UNIT])
        .output()
    else {
        return ServiceState::Unknown;
    };
    match String::from_utf8_lossy(&output.stdout).trim() {
        "active" | "activating" | "reloading" => ServiceState::Running,
        "inactive" | "deactivating" => ServiceState::Stopped,
        "failed" => ServiceState::Failed,
        _ => ServiceState::Unknown,
    }
}

fn run_systemctl(args: &[&str]) -> Option<String> {
    match Command::new("systemctl").arg("--user").args(args).output() {
        Ok(out) if out.status.success() => None,
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_owned();
            Some(if err.is_empty() {
                format!("systemctl {} failed", args.join(" "))
            } else {
                err
            })
        }
        Err(error) => Some(format!("systemctl unavailable: {error}")),
    }
}

/// D-Bus interface for single-instance enforcement of `glacier --settings`.
struct SettingsInstance {
    raise_flag: Arc<AtomicBool>,
}

#[zbus::interface(name = "org.glaciernotch.Settings")]
impl SettingsInstance {
    fn raise(&self) {
        self.raise_flag.store(true, Ordering::SeqCst);
    }
}

struct SingleInstanceGuard {
    raise_flag: Arc<AtomicBool>,
    _stop_tx: Option<Sender<()>>,
}

/// Returns `Ok(None)` if an existing Settings window was signaled to raise.
/// Returns `Ok(Some(guard))` if this process should open the Settings window.
fn acquire_single_instance() -> Option<SingleInstanceGuard> {
    if let Ok(conn) = Connection::session()
        && let Ok(proxy) = Proxy::new(
            &conn,
            SETTINGS_BUS_NAME,
            SETTINGS_OBJ_PATH,
            SETTINGS_BUS_NAME,
        )
        && proxy.call::<_, _, ()>("Raise", &()).is_ok()
    {
        return None;
    }

    let raise_flag = Arc::new(AtomicBool::new(false));
    let flag_clone = Arc::clone(&raise_flag);
    let (stop_tx, stop_rx) = channel::<()>();
    thread::Builder::new()
        .name("glacier-settings-dbus".into())
        .spawn(move || {
            let Ok(builder) = Builder::session() else {
                return;
            };
            let Ok(builder) = builder.name(SETTINGS_BUS_NAME) else {
                return;
            };
            let Ok(builder) = builder.serve_at(
                SETTINGS_OBJ_PATH,
                SettingsInstance {
                    raise_flag: flag_clone,
                },
            ) else {
                return;
            };
            let Ok(conn) = builder.build() else {
                return;
            };
            let _ = stop_rx.recv();
            drop(conn);
        })
        .ok();

    Some(SingleInstanceGuard {
        raise_flag,
        _stop_tx: Some(stop_tx),
    })
}

pub fn run(initial_section: Section) -> eframe::Result<()> {
    let Some(instance_guard) = acquire_single_instance() else {
        return Ok(());
    };

    let icon_rgba = tray::build_window_icon_rgba(64);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("GlacierNotch Settings")
            .with_inner_size([780.0, 560.0])
            .with_min_inner_size([640.0, 440.0])
            .with_icon(Arc::new(egui::IconData {
                rgba: icon_rgba,
                width: 64,
                height: 64,
            })),
        ..Default::default()
    };

    eframe::run_native(
        "GlacierNotch Settings",
        options,
        Box::new(move |cc| {
            crate::install_fonts(&cc.egui_ctx);
            apply_glacier_theme(&cc.egui_ctx);
            Ok(Box::new(SettingsApp::new(initial_section, instance_guard)))
        }),
    )
}

fn apply_glacier_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = BG_BASE;
    visuals.panel_fill = BG_BASE;
    visuals.faint_bg_color = BG_CARD;
    visuals.extreme_bg_color = BG_WELL;
    visuals.override_text_color = Some(TEXT);
    visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(122, 211, 255, 44);
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    visuals.widgets.noninteractive.bg_fill = BG_CARD;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.inactive.bg_fill = BG_WELL;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(34, 36, 48);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.active.bg_fill = Color32::from_rgb(40, 46, 60);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, TEXT);
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_visuals_of(egui::Theme::Dark, visuals);

    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.spacing.item_spacing = Vec2::new(10.0, 8.0);
    style.spacing.button_padding = Vec2::new(12.0, 6.0);
    ctx.set_style_of(egui::Theme::Dark, style);
}

struct SettingsApp {
    section: Section,
    config: Config,
    ignored_players_text: String,
    autostart_enabled: bool,
    audio_backend: &'static str,
    xrandr_outputs: Vec<String>,
    service: ServiceMonitor,
    started: Instant,
    dirty_since: Option<Instant>,
    last_saved_at: Option<Instant>,
    save_error: Option<String>,
    confirm_reset: bool,
    instance_guard: SingleInstanceGuard,
}

impl SettingsApp {
    fn new(section: Section, instance_guard: SingleInstanceGuard) -> Self {
        let config = Config::load();
        let ignored_players_text = config.music.ignored_players.join(", ");
        Self {
            section,
            config,
            ignored_players_text,
            autostart_enabled: autostart::status(),
            audio_backend: controls::detected_audio_backend_label(),
            xrandr_outputs: controls::list_outputs(),
            service: ServiceMonitor::start(),
            started: Instant::now(),
            dirty_since: None,
            last_saved_at: None,
            save_error: None,
            confirm_reset: false,
            instance_guard,
        }
    }

    fn save_now(&mut self) {
        self.dirty_since = None;
        self.config = self.config.clone().sanitized();
        match self.config.save() {
            Ok(()) => {
                self.save_error = None;
                self.last_saved_at = Some(Instant::now());
            }
            Err(error) => {
                self.save_error = Some(format!("Cannot save config: {error}"));
            }
        }
    }

    fn mark_debounced(&mut self) {
        self.dirty_since = Some(Instant::now());
    }

    fn flush_debounced(&mut self) {
        if let Some(since) = self.dirty_since
            && since.elapsed() >= Duration::from_millis(160)
        {
            self.save_now();
        }
    }

    fn draw_sidebar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            let (rect, _) = ui.allocate_exact_size(Vec2::new(26.0, 22.0), egui::Sense::hover());
            let painter = ui.painter();
            painter.rect_filled(rect, CornerRadius::same(6), BG_BASE);
            painter.rect_stroke(
                rect,
                CornerRadius::same(6),
                Stroke::new(1.0, BORDER),
                StrokeKind::Inside,
            );
            painter.circle_filled(rect.center() + Vec2::new(-4.5, 2.0), 2.2, ACCENT);
            painter.circle_filled(rect.center() + Vec2::new(4.5, 2.0), 2.2, ACCENT);
            painter.rect_filled(
                Rect::from_min_max(
                    rect.min + Vec2::new(4.0, 3.0),
                    Pos2::new(rect.max.x - 4.0, rect.min.y + 8.5),
                ),
                CornerRadius::same(2),
                Color32::from_rgb(248, 195, 75),
            );
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(
                    egui::RichText::new("GlacierNotch")
                        .size(15.0)
                        .strong()
                        .color(TEXT),
                );
                ui.label(
                    egui::RichText::new("v0.2.0 · Settings")
                        .size(10.5)
                        .color(MUTED),
                );
            });
        });

        ui.add_space(14.0);

        for section in Section::ALL {
            let selected = self.section == section;
            let fill = if selected {
                Color32::from_rgb(24, 34, 46)
            } else {
                Color32::TRANSPARENT
            };
            let stroke = if selected {
                Stroke::new(1.0, Color32::from_rgba_unmultiplied(122, 211, 255, 110))
            } else {
                Stroke::NONE
            };
            let button = egui::Button::new(
                egui::RichText::new(section.label())
                    .size(13.0)
                    .color(if selected { TEXT } else { MUTED }),
            )
            .fill(fill)
            .stroke(stroke)
            .corner_radius(CornerRadius::same(7))
            .min_size(Vec2::new(ui.available_width(), 32.0));
            if ui.add(button).clicked() {
                self.section = section;
                self.confirm_reset = false;
            }
        }

        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            ui.add_space(8.0);
            egui::Frame::new()
                .fill(BG_CARD)
                .stroke(Stroke::new(1.0, BORDER))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::same(10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let (dot, _) =
                            ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
                        ui.painter()
                            .circle_filled(dot.center(), 4.0, self.service.state.color());
                        ui.label(
                            egui::RichText::new(format!("Notch: {}", self.service.state.label()))
                                .size(11.5)
                                .color(TEXT),
                        );
                    });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        let label = if self.service.state == ServiceState::Running {
                            "Restart"
                        } else {
                            "Start"
                        };
                        if ui
                            .add_enabled(
                                !self.service.busy,
                                egui::Button::new(label).min_size(Vec2::new(68.0, 24.0)),
                            )
                            .clicked()
                        {
                            self.service
                                .send(if self.service.state == ServiceState::Running {
                                    ServiceReq::Restart
                                } else {
                                    ServiceReq::Start
                                });
                        }
                        if self.service.state == ServiceState::Running
                            && ui
                                .add_enabled(
                                    !self.service.busy,
                                    egui::Button::new("Stop").min_size(Vec2::new(56.0, 24.0)),
                                )
                                .clicked()
                        {
                            self.service.send(ServiceReq::Stop);
                        }
                    });
                });
        });
    }

    fn draw_header(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new(self.section.label())
                        .size(20.0)
                        .strong()
                        .color(TEXT),
                );
                ui.label(
                    egui::RichText::new(self.section.subtitle())
                        .size(12.0)
                        .color(MUTED),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                if let Some(error) = &self.save_error {
                    ui.label(egui::RichText::new(error).size(11.5).color(RED));
                } else if self.dirty_since.is_some() {
                    ui.label(egui::RichText::new("Saving...").size(11.5).color(AMBER));
                } else if self
                    .last_saved_at
                    .is_some_and(|at| at.elapsed() < Duration::from_secs(4))
                {
                    ui.label(
                        egui::RichText::new("Saved · Live applied")
                            .size(11.5)
                            .color(GREEN),
                    );
                }
            });
        });
        ui.add_space(10.0);
    }

    fn draw_features(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        card(ui, "Notch Modules & Tabs", |ui| {
            ui.label(
                egui::RichText::new(
                    "Disabling a module hides its tab and pauses its background worker immediately.",
                )
                .size(11.5)
                .color(MUTED),
            );
            ui.add_space(6.0);

            changed |= toggle_row(
                ui,
                "Music & Media",
                "MPRIS playback tracking, local album artwork, and synced lyrics.",
                &mut self.config.features.music,
            );
            ui.separator();

            changed |= toggle_row(
                ui,
                "System Controls",
                "Quick volume and software gamma brightness sliders.",
                &mut self.config.features.controls,
            );
            if self.config.features.controls {
                ui.indent("controls-sub", |ui| {
                    changed |= toggle_row(
                        ui,
                        "Volume slider",
                        "Control default PipeWire / PulseAudio sink volume.",
                        &mut self.config.features.volume,
                    );
                    changed |= toggle_row(
                        ui,
                        "Software brightness (gamma dim)",
                        "Turning this off immediately restores display gamma to 100%.",
                        &mut self.config.features.brightness,
                    );
                });
            }
            ui.separator();

            changed |= toggle_row(
                ui,
                "Focus Timer",
                "Pomodoro countdown timer with break intervals and desktop alerts.",
                &mut self.config.features.focus,
            );
            ui.separator();

            changed |= toggle_row(
                ui,
                "File Shelf",
                "Pin dropped files to the notch for quick open or path copy.",
                &mut self.config.features.shelf,
            );
            if self.config.features.shelf {
                ui.indent("shelf-sub", |ui| {
                    changed |= toggle_row(
                        ui,
                        "Accept drag-and-drop onto notch",
                        "Automatically adds dropped files and opens the Shelf tab.",
                        &mut self.config.features.shelf_drop_to_notch,
                    );
                });
            }
            ui.separator();

            changed |= toggle_row(
                ui,
                "Clipboard History",
                "Monitors X11 CLIPBOARD selections. Disabling stops clipboard capture.",
                &mut self.config.features.clipboard,
            );
            changed |= toggle_row(
                ui,
                "Quick Notes",
                "Lightweight persistent notes inside the Clips tab.",
                &mut self.config.features.notes,
            );
        });

        ui.add_space(10.0);

        let enabled = self.config.features.enabled_tabs();
        card(ui, "Default Tab", |ui| {
            if enabled.is_empty() {
                ui.label(
                    egui::RichText::new(
                        "All tabs are currently disabled. The notch will show an Open Settings shortcut when expanded.",
                    )
                    .size(12.0)
                    .color(AMBER),
                );
            } else {
                ui.horizontal(|ui| {
                    ui.label("Open notch on tab:");
                    egui::ComboBox::from_id_salt("default-tab-combo")
                        .selected_text(self.config.features.default_tab.label())
                        .show_ui(ui, |ui| {
                            for tab in enabled {
                                if ui
                                    .selectable_value(
                                        &mut self.config.features.default_tab,
                                        tab,
                                        tab.label(),
                                    )
                                    .clicked()
                                {
                                    changed = true;
                                }
                            }
                        });
                });
            }
        });

        if changed {
            self.save_now();
        }
    }

    fn draw_appearance(&mut self, ui: &mut egui::Ui) {
        let mut instant_change = false;
        let mut debounced_change = false;

        card(ui, "Hover & Motion", |ui| {
            ui.horizontal(|ui| {
                ui.label("Hover open delay:");
                if ui
                    .add(
                        egui::Slider::new(&mut self.config.appearance.hover_open_delay_ms, 0..=500)
                            .suffix(" ms"),
                    )
                    .changed()
                {
                    debounced_change = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("Hover close grace delay:");
                if ui
                    .add(
                        egui::Slider::new(
                            &mut self.config.appearance.hover_close_delay_ms,
                            0..=800,
                        )
                        .suffix(" ms"),
                    )
                    .changed()
                {
                    debounced_change = true;
                }
            });
            instant_change |= toggle_row(
                ui,
                "Reduce motion",
                "Snap the notch open and closed without spring easing.",
                &mut self.config.appearance.reduce_motion,
            );
            instant_change |= toggle_row(
                ui,
                "Show Settings gear button in notch",
                "Keep a compact gear icon at the top-right of the expanded notch.",
                &mut self.config.appearance.show_settings_button,
            );
        });

        ui.add_space(10.0);

        card(ui, "Accent & Colors", |ui| {
            instant_change |= toggle_row(
                ui,
                "Dynamic album-art accent color",
                "Tint the Music tab and collapsed track title using the cover's dominant hue.",
                &mut self.config.appearance.dynamic_music_accent,
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("UI accent color:");
                let mut color = Color32::from_rgb(
                    self.config.appearance.accent_rgb[0],
                    self.config.appearance.accent_rgb[1],
                    self.config.appearance.accent_rgb[2],
                );
                if ui.color_edit_button_srgba(&mut color).changed() {
                    self.config.appearance.accent_rgb = [color.r(), color.g(), color.b()];
                    debounced_change = true;
                }
                for (name, rgb) in [
                    ("Glacier", [122, 211, 255]),
                    ("Gold", [248, 195, 75]),
                    ("Mint", [98, 224, 168]),
                    ("Coral", [255, 134, 122]),
                ] {
                    let swatch = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                    if ui
                        .add(
                            egui::Button::new(egui::RichText::new(name).size(11.0).color(BG_BASE))
                                .fill(swatch)
                                .corner_radius(CornerRadius::same(5)),
                        )
                        .clicked()
                    {
                        self.config.appearance.accent_rgb = rgb;
                        instant_change = true;
                    }
                }
            });
        });

        ui.add_space(10.0);

        card(ui, "Keyboard Shortcuts", |ui| {
            instant_change |= toggle_row(
                ui,
                "Number & Alt+Arrow tab switching",
                "Press 1–5 or Alt+Left / Alt+Right to switch enabled tabs.",
                &mut self.config.appearance.tab_hotkeys,
            );
            instant_change |= toggle_row(
                ui,
                "Escape key quits notch daemon",
                "Turn off so Escape only collapses/cancels instead of closing the notch (Ctrl+Q still quits).",
                &mut self.config.appearance.escape_quits,
            );
        });

        if instant_change {
            self.save_now();
        } else if debounced_change {
            self.mark_debounced();
        }
    }

    fn draw_idle(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;

        card(ui, "Live Collapsed Notch Preview", |ui| {
            let (outer, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 74.0), egui::Sense::hover());
            let painter = ui.painter();
            painter.rect_filled(outer, CornerRadius::same(10), Color32::from_rgb(14, 16, 22));
            painter.rect_stroke(
                outer,
                CornerRadius::same(10),
                Stroke::new(1.0, BORDER),
                StrokeKind::Inside,
            );

            let pill = Rect::from_center_size(outer.center(), Vec2::new(214.0, 34.0));
            painter.rect_filled(
                pill,
                CornerRadius {
                    nw: 0,
                    ne: 0,
                    sw: 8,
                    se: 8,
                },
                BG_BASE,
            );
            painter.rect_stroke(
                pill,
                CornerRadius {
                    nw: 0,
                    ne: 0,
                    sw: 8,
                    se: 8,
                },
                Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 24)),
                StrokeKind::Inside,
            );

            let preview_painter = painter.with_clip_rect(pill);
            match self.config.mascot.idle_mode {
                IdleMode::Mascot => {
                    mascot::draw_with_options(
                        &preview_painter,
                        pill.center(),
                        self.started.elapsed().as_secs_f64(),
                        MascotOptions {
                            mode: mascot::Mode::from(self.config.mascot.form),
                            show_aura: self.config.mascot.show_aura,
                            animate: self.config.mascot.animate,
                            blink: self.config.mascot.blink,
                        },
                    );
                }
                IdleMode::Clock => {
                    preview_painter.text(
                        pill.center(),
                        Align2::CENTER_CENTER,
                        "12:45",
                        FontId::proportional(13.5),
                        TEXT,
                    );
                }
                IdleMode::Minimal => {
                    let accent = Color32::from_rgb(
                        self.config.appearance.accent_rgb[0],
                        self.config.appearance.accent_rgb[1],
                        self.config.appearance.accent_rgb[2],
                    );
                    preview_painter.circle_filled(
                        pill.center() + Vec2::new(-28.0, 0.0),
                        3.0,
                        accent,
                    );
                    preview_painter.text(
                        pill.center() + Vec2::new(6.0, 0.0),
                        Align2::CENTER_CENTER,
                        "glacier",
                        FontId::proportional(11.5),
                        MUTED,
                    );
                }
            }
        });

        ui.add_space(10.0);

        card(ui, "Idle Behavior", |ui| {
            ui.horizontal(|ui| {
                ui.label("Idle display mode:");
                for mode in IdleMode::ALL {
                    if ui
                        .selectable_value(&mut self.config.mascot.idle_mode, mode, mode.label())
                        .clicked()
                    {
                        changed = true;
                    }
                }
            });

            if self.config.mascot.idle_mode == IdleMode::Mascot {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("Minato form:");
                    for form in MascotForm::ALL {
                        if ui
                            .selectable_value(&mut self.config.mascot.form, form, form.label())
                            .clicked()
                        {
                            changed = true;
                        }
                    }
                });
                ui.add_space(6.0);
                changed |= toggle_row(
                    ui,
                    "Golden aura & sparks",
                    "Show the pulsing golden glow around the mascot.",
                    &mut self.config.mascot.show_aura,
                );
                changed |= toggle_row(
                    ui,
                    "Animate hair & gaze",
                    "Turn off for a static mascot with lower idle repaint rate.",
                    &mut self.config.mascot.animate,
                );
                changed |= toggle_row(
                    ui,
                    "Periodic eye blink",
                    "Natural eye blinks every few seconds.",
                    &mut self.config.mascot.blink,
                );
            }
        });

        if changed {
            self.save_now();
        }
    }

    fn draw_modules(&mut self, ui: &mut egui::Ui) {
        let mut instant_change = false;
        let mut debounced_change = false;

        card(ui, "Music & Lyrics", |ui| {
            instant_change |= toggle_row(
                ui,
                "Show lyrics in Music panel",
                "Display synced or plain lyrics under the progress bar.",
                &mut self.config.music.show_lyrics,
            );
            if self.config.music.show_lyrics {
                instant_change |= toggle_row(
                    ui,
                    "Online LRCLIB lookup",
                    "Query lrclib.net when no local .lrc file is found in ~/.local/share/glacier/lyrics/.",
                    &mut self.config.music.online_lrclib,
                );
            }
            instant_change |= toggle_row(
                ui,
                "Mouse-wheel seek on Music panel",
                "Scroll up/down on the Music tab to nudge playback.",
                &mut self.config.music.scroll_seek,
            );
            ui.horizontal(|ui| {
                ui.label("Seek nudge step:");
                if ui
                    .add(
                        egui::Slider::new(&mut self.config.music.seek_step_secs, 1..=30)
                            .suffix(" s"),
                    )
                    .changed()
                {
                    debounced_change = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("Ignored MPRIS players:");
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.ignored_players_text)
                        .hint_text("e.g. chromium, firefox")
                        .desired_width(240.0),
                );
                if resp.changed() {
                    self.config.music.ignored_players = self
                        .ignored_players_text
                        .split(',')
                        .map(|s| s.trim().to_owned())
                        .filter(|s| !s.is_empty())
                        .collect();
                    debounced_change = true;
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Open Local Lyrics Folder").clicked() {
                    let dir = storage::data_path("lyrics");
                    let _ = std::fs::create_dir_all(&dir);
                    open_path(&dir);
                }
            });
        });

        ui.add_space(10.0);

        card(ui, "System Controls", |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("Audio backend: {}", self.audio_backend))
                        .size(11.5)
                        .color(MUTED),
                );
            });
            let mut allow_boost = self.config.controls.volume_max > 1.05;
            if toggle_row(
                ui,
                "Allow volume boost up to 150%",
                "When off, volume slider caps at 100%.",
                &mut allow_boost,
            ) {
                self.config.controls.volume_max = if allow_boost { 1.5 } else { 1.0 };
                instant_change = true;
            }
            ui.horizontal(|ui| {
                ui.label("Minimum brightness floor:");
                let mut min_pct = (self.config.controls.brightness_min * 100.0).round() as u32;
                if ui
                    .add(egui::Slider::new(&mut min_pct, 10..=80).suffix("%"))
                    .changed()
                {
                    self.config.controls.brightness_min = min_pct as f32 / 100.0;
                    debounced_change = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("Brightness xrandr output:");
                let selected_label = self
                    .config
                    .controls
                    .xrandr_output
                    .as_deref()
                    .unwrap_or("Auto (first connected)");
                egui::ComboBox::from_id_salt("xrandr-output-combo")
                    .selected_text(selected_label)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_value(
                                &mut self.config.controls.xrandr_output,
                                None,
                                "Auto (first connected)",
                            )
                            .clicked()
                        {
                            instant_change = true;
                        }
                        for output in &self.xrandr_outputs {
                            if ui
                                .selectable_value(
                                    &mut self.config.controls.xrandr_output,
                                    Some(output.clone()),
                                    output,
                                )
                                .clicked()
                            {
                                instant_change = true;
                            }
                        }
                    });
                if ui.button("Refresh").clicked() {
                    self.xrandr_outputs = controls::list_outputs();
                }
                if ui.button("Reset Gamma 100%").clicked() {
                    controls::reset_gamma(self.config.controls.xrandr_output.as_deref());
                }
            });
        });

        ui.add_space(10.0);

        card(ui, "Focus Timer", |ui| {
            ui.horizontal(|ui| {
                ui.label("Focus duration:");
                if ui
                    .add(
                        egui::Slider::new(&mut self.config.focus.focus_minutes, 5..=90)
                            .suffix(" min"),
                    )
                    .changed()
                {
                    debounced_change = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("Break duration:");
                if ui
                    .add(
                        egui::Slider::new(&mut self.config.focus.break_minutes, 1..=30)
                            .suffix(" min"),
                    )
                    .changed()
                {
                    debounced_change = true;
                }
            });
            instant_change |= toggle_row(
                ui,
                "Desktop notification when session ends",
                "Send a notification via notify-send on Focus/Break completion.",
                &mut self.config.focus.notifications,
            );
            instant_change |= toggle_row(
                ui,
                "Auto-start next session",
                "Automatically start the break or next focus block when time runs out.",
                &mut self.config.focus.auto_start_next,
            );
        });

        ui.add_space(10.0);

        card(ui, "Shelf & Clipboard", |ui| {
            ui.horizontal(|ui| {
                ui.label("Clipboard history limit:");
                if ui
                    .add(
                        egui::Slider::new(&mut self.config.clipboard.max_clips, 10..=200)
                            .suffix(" items"),
                    )
                    .changed()
                {
                    debounced_change = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("File shelf limit:");
                if ui
                    .add(
                        egui::Slider::new(&mut self.config.shelf.max_items, 4..=20)
                            .suffix(" files"),
                    )
                    .changed()
                {
                    debounced_change = true;
                }
            });
            instant_change |= toggle_row(
                ui,
                "Persist file shelf across restarts",
                "Save pinned shelf paths to ~/.local/share/glacier/shelf.json.",
                &mut self.config.shelf.persist,
            );
            if ui.button("Open Glacier Data Folder").clicked() {
                let dir = storage::data_path("");
                let _ = std::fs::create_dir_all(&dir);
                open_path(&dir);
            }
        });

        if instant_change {
            self.save_now();
        } else if debounced_change {
            self.mark_debounced();
        }
    }

    fn draw_system(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;

        card(ui, "Notch Service (systemd user unit)", |ui| {
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(Vec2::splat(10.0), egui::Sense::hover());
                ui.painter()
                    .circle_filled(dot.center(), 4.5, self.service.state.color());
                ui.label(
                    egui::RichText::new(format!("{SERVICE_UNIT}: {}", self.service.state.label()))
                        .size(13.0)
                        .strong(),
                );
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!self.service.busy, egui::Button::new("Start"))
                    .clicked()
                {
                    self.service.send(ServiceReq::Start);
                }
                if ui
                    .add_enabled(!self.service.busy, egui::Button::new("Restart"))
                    .clicked()
                {
                    self.service.send(ServiceReq::Restart);
                }
                if ui
                    .add_enabled(!self.service.busy, egui::Button::new("Stop"))
                    .clicked()
                {
                    self.service.send(ServiceReq::Stop);
                }
                if ui
                    .add_enabled(!self.service.busy, egui::Button::new("Refresh Status"))
                    .clicked()
                {
                    self.service.send(ServiceReq::Refresh);
                }
            });
            if let Some(note) = &self.service.status_note {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(note).size(11.5).color(AMBER));
            }
        });

        ui.add_space(10.0);

        card(ui, "Desktop Integration", |ui| {
            let mut autostart_ui = self.autostart_enabled;
            if toggle_row(
                ui,
                "Launch notch at login (XFCE autostart)",
                "Starts only the silent notch service on login without opening Settings.",
                &mut autostart_ui,
            ) {
                let result = if autostart_ui {
                    autostart::enable().map(|_| ())
                } else {
                    autostart::disable().map(|_| ())
                };
                match result {
                    Ok(()) => {
                        self.autostart_enabled = autostart::status();
                        self.save_error = None;
                    }
                    Err(error) => {
                        self.save_error = Some(format!("Autostart update failed: {error}"));
                    }
                }
            }
            ui.separator();
            changed |= toggle_row(
                ui,
                "Show XFCE system tray icon",
                "Left-click tray to open Settings; right-click to toggle notch, restart, or quit.",
                &mut self.config.system.tray_icon,
            );
        });

        ui.add_space(10.0);

        card(ui, "Configuration & Reset", |ui| {
            ui.label(
                egui::RichText::new(format!("Config file: {}", Config::path().display()))
                    .size(11.5)
                    .color(MUTED),
            );
            ui.label(
                egui::RichText::new(format!(
                    "Autostart entry: {}",
                    autostart::entry_path().display()
                ))
                .size(11.5)
                .color(MUTED),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Open Config Folder").clicked()
                    && let Some(parent) = Config::path().parent()
                {
                    let _ = std::fs::create_dir_all(parent);
                    open_path(parent);
                }
                if !self.confirm_reset {
                    if ui.button("Reset to Defaults").clicked() {
                        self.confirm_reset = true;
                    }
                } else {
                    ui.label(egui::RichText::new("Reset all settings?").color(AMBER));
                    if ui
                        .add(
                            egui::Button::new(egui::RichText::new("Confirm Reset").color(BG_BASE))
                                .fill(AMBER),
                        )
                        .clicked()
                    {
                        self.config = Config::default();
                        self.ignored_players_text.clear();
                        self.confirm_reset = false;
                        self.save_now();
                    }
                    if ui.button("Cancel").clicked() {
                        self.confirm_reset = false;
                    }
                }
            });
        });

        if changed {
            self.save_now();
        }
    }
}

impl eframe::App for SettingsApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.service.poll();
        self.flush_debounced();

        if self.instance_guard.raise_flag.swap(false, Ordering::SeqCst) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Q) && i.modifiers.ctrl) {
            self.save_now();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        egui::Panel::left("settings-sidebar")
            .resizable(false)
            .exact_size(196.0)
            .frame(
                egui::Frame::new()
                    .fill(BG_SIDEBAR)
                    .stroke(Stroke::new(1.0, BORDER))
                    .inner_margin(Margin::symmetric(10, 10)),
            )
            .show(ui, |ui| {
                self.draw_sidebar(ui);
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(BG_BASE)
                    .inner_margin(Margin::symmetric(22, 18)),
            )
            .show(ui, |ui| {
                self.draw_header(ui);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.section {
                        Section::Features => self.draw_features(ui),
                        Section::Appearance => self.draw_appearance(ui),
                        Section::Idle => self.draw_idle(ui),
                        Section::Modules => self.draw_modules(ui),
                        Section::System => self.draw_system(ui),
                    });
            });

        let repaint_ms = if self.section == Section::Idle
            && self.config.mascot.idle_mode == IdleMode::Mascot
            && (self.config.mascot.animate || self.config.mascot.blink)
        {
            40
        } else if self.dirty_since.is_some() {
            60
        } else {
            250
        };
        ctx.request_repaint_after(Duration::from_millis(repaint_ms));
    }
}

fn card(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(BG_CARD)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new(title).size(13.5).strong().color(TEXT));
            ui.add_space(6.0);
            body(ui);
        });
}

fn toggle_row(ui: &mut egui::Ui, title: &str, description: &str, value: &mut bool) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(egui::RichText::new(title).size(12.5).color(TEXT));
            ui.label(egui::RichText::new(description).size(11.0).color(MUTED));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if switch(ui, value).changed() {
                changed = true;
            }
        });
    });
    changed
}

fn switch(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let desired = Vec2::new(38.0, 20.0);
    let (rect, mut response) = ui.allocate_exact_size(desired, egui::Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let how_on = ui.ctx().animate_bool_responsive(response.id, *on);
        let fill = if *on {
            Color32::from_rgba_unmultiplied(122, 211, 255, 195)
        } else {
            BG_WELL
        };
        let stroke_color = if response.hovered() || *on {
            ACCENT
        } else {
            BORDER
        };
        let radius = (rect.height() * 0.5).round() as u8;
        ui.painter()
            .rect_filled(rect, CornerRadius::same(radius), fill);
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(radius),
            Stroke::new(1.0, stroke_color),
            StrokeKind::Inside,
        );
        let knob_r = rect.height() * 0.36;
        let knob_x = egui::lerp(
            (rect.left() + rect.height() * 0.5)..=(rect.right() - rect.height() * 0.5),
            how_on,
        );
        ui.painter().circle_filled(
            Pos2::new(knob_x, rect.center().y),
            knob_r,
            if *on { BG_BASE } else { TEXT },
        );
    }
    response
}

fn open_path(path: &std::path::Path) {
    let _ = Command::new("xdg-open")
        .arg(path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

/// Spawns `glacier --settings` detached from the notch service cgroup so
/// restarting `glaciernotch.service` from Settings does not kill Settings.
pub fn spawn_detached() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let scoped = Command::new("systemd-run")
        .args(["--user", "--scope", "--"])
        .arg(&exe)
        .arg("--settings")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if scoped.is_err() {
        let _ = Command::new(&exe)
            .arg("--settings")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
}
