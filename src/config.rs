use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

use crate::storage;

pub const CONFIG_VERSION: u32 = 1;
const WATCH_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabId {
    #[default]
    Music,
    Control,
    Focus,
    Shelf,
    Clips,
}

impl TabId {
    pub const ALL: [TabId; 5] = [
        TabId::Music,
        TabId::Control,
        TabId::Focus,
        TabId::Shelf,
        TabId::Clips,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TabId::Music => "Music",
            TabId::Control => "Control",
            TabId::Focus => "Focus",
            TabId::Shelf => "Shelf",
            TabId::Clips => "Clips",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdleMode {
    #[default]
    Mascot,
    Clock,
    Minimal,
}

impl IdleMode {
    pub const ALL: [IdleMode; 3] = [IdleMode::Mascot, IdleMode::Clock, IdleMode::Minimal];

    pub fn label(self) -> &'static str {
        match self {
            IdleMode::Mascot => "Mascot",
            IdleMode::Clock => "Minimal Clock",
            IdleMode::Minimal => "Glacier Pill",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MascotForm {
    #[default]
    Kurama,
    Normal,
    Sage,
}

impl MascotForm {
    pub const ALL: [MascotForm; 3] = [MascotForm::Kurama, MascotForm::Normal, MascotForm::Sage];

    pub fn label(self) -> &'static str {
        match self {
            MascotForm::Kurama => "Kurama (KCM)",
            MascotForm::Normal => "Normal",
            MascotForm::Sage => "Toad Sage",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub features: FeaturesConfig,
    pub appearance: AppearanceConfig,
    pub mascot: MascotConfig,
    pub music: MusicConfig,
    pub controls: ControlsConfig,
    pub focus: FocusConfig,
    pub shelf: ShelfConfig,
    pub clipboard: ClipboardConfig,
    pub system: SystemConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            features: FeaturesConfig::default(),
            appearance: AppearanceConfig::default(),
            mascot: MascotConfig::default(),
            music: MusicConfig::default(),
            controls: ControlsConfig::default(),
            focus: FocusConfig::default(),
            shelf: ShelfConfig::default(),
            clipboard: ClipboardConfig::default(),
            system: SystemConfig::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FeaturesConfig {
    pub music: bool,
    pub controls: bool,
    pub volume: bool,
    pub brightness: bool,
    pub focus: bool,
    pub shelf: bool,
    pub shelf_drop_to_notch: bool,
    pub clipboard: bool,
    pub notes: bool,
    pub default_tab: TabId,
}

impl Default for FeaturesConfig {
    fn default() -> Self {
        Self {
            music: true,
            controls: true,
            volume: true,
            brightness: true,
            focus: true,
            shelf: true,
            shelf_drop_to_notch: true,
            clipboard: true,
            notes: true,
            default_tab: TabId::Music,
        }
    }
}

impl FeaturesConfig {
    pub fn volume_enabled(&self) -> bool {
        self.controls && self.volume
    }

    pub fn brightness_enabled(&self) -> bool {
        self.controls && self.brightness
    }

    pub fn shelf_drop_enabled(&self) -> bool {
        self.shelf && self.shelf_drop_to_notch
    }

    pub fn is_tab_enabled(&self, tab: TabId) -> bool {
        match tab {
            TabId::Music => self.music,
            TabId::Control => self.controls && (self.volume || self.brightness),
            TabId::Focus => self.focus,
            TabId::Shelf => self.shelf,
            TabId::Clips => self.clipboard || self.notes,
        }
    }

    pub fn enabled_tabs(&self) -> Vec<TabId> {
        TabId::ALL
            .into_iter()
            .filter(|&tab| self.is_tab_enabled(tab))
            .collect()
    }

    pub fn resolve_tab(&self, current: TabId) -> Option<TabId> {
        if self.is_tab_enabled(current) {
            return Some(current);
        }
        if self.is_tab_enabled(self.default_tab) {
            return Some(self.default_tab);
        }
        self.enabled_tabs().into_iter().next()
    }

    pub fn step_tab(&self, current: TabId, forward: bool) -> Option<TabId> {
        let enabled = self.enabled_tabs();
        if enabled.is_empty() {
            return None;
        }
        let Some(index) = enabled.iter().position(|&tab| tab == current) else {
            return Some(enabled[0]);
        };
        let len = enabled.len();
        let next = if forward {
            (index + 1) % len
        } else {
            (index + len - 1) % len
        };
        Some(enabled[next])
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppearanceConfig {
    pub hover_open_delay_ms: u32,
    pub hover_close_delay_ms: u32,
    pub reduce_motion: bool,
    pub dynamic_music_accent: bool,
    pub accent_rgb: [u8; 3],
    pub show_settings_button: bool,
    pub escape_quits: bool,
    pub tab_hotkeys: bool,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            hover_open_delay_ms: 0,
            hover_close_delay_ms: 0,
            reduce_motion: false,
            dynamic_music_accent: true,
            accent_rgb: [122, 211, 255],
            show_settings_button: true,
            escape_quits: true,
            tab_hotkeys: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MascotConfig {
    pub idle_mode: IdleMode,
    pub form: MascotForm,
    pub show_aura: bool,
    pub animate: bool,
    pub blink: bool,
}

impl Default for MascotConfig {
    fn default() -> Self {
        Self {
            idle_mode: IdleMode::Mascot,
            form: MascotForm::Kurama,
            show_aura: true,
            animate: true,
            blink: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MusicConfig {
    pub show_lyrics: bool,
    pub online_lrclib: bool,
    pub seek_step_secs: u32,
    pub scroll_seek: bool,
    pub ignored_players: Vec<String>,
}

impl Default for MusicConfig {
    fn default() -> Self {
        Self {
            show_lyrics: true,
            online_lrclib: true,
            seek_step_secs: 5,
            scroll_seek: true,
            ignored_players: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ControlsConfig {
    pub volume_max: f32,
    pub brightness_min: f32,
    pub xrandr_output: Option<String>,
}

impl Default for ControlsConfig {
    fn default() -> Self {
        Self {
            volume_max: 1.0,
            brightness_min: 0.30,
            xrandr_output: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FocusConfig {
    pub focus_minutes: u32,
    pub break_minutes: u32,
    pub notifications: bool,
    pub auto_start_next: bool,
}

impl Default for FocusConfig {
    fn default() -> Self {
        Self {
            focus_minutes: 25,
            break_minutes: 5,
            notifications: true,
            auto_start_next: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShelfConfig {
    pub max_items: usize,
    pub persist: bool,
}

impl Default for ShelfConfig {
    fn default() -> Self {
        Self {
            max_items: 8,
            persist: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipboardConfig {
    pub max_clips: usize,
}

impl Default for ClipboardConfig {
    fn default() -> Self {
        Self { max_clips: 40 }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SystemConfig {
    pub tray_icon: bool,
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self { tray_icon: true }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        storage::config_path("config.json")
    }

    pub fn load() -> Self {
        Self::load_from(&Self::path())
    }

    pub fn load_from(path: &Path) -> Self {
        match storage::read_object_at::<Config>(path) {
            Ok(Some(config)) => config.sanitized(),
            Ok(None) => Self::default(),
            Err(error) => {
                eprintln!(
                    "Glacier config read failed ({}): {error}; using defaults",
                    path.display()
                );
                Self::default()
            }
        }
    }

    pub fn save(&self) -> io::Result<()> {
        self.save_to(&Self::path())
    }

    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        let clean = self.clone().sanitized();
        storage::write_object_at(path, &clean)
    }

    pub fn sanitized(mut self) -> Self {
        self.version = CONFIG_VERSION;
        self.appearance.hover_open_delay_ms = self.appearance.hover_open_delay_ms.min(500);
        self.appearance.hover_close_delay_ms = self.appearance.hover_close_delay_ms.min(800);
        self.music.seek_step_secs = self.music.seek_step_secs.clamp(1, 60);
        self.music.ignored_players = self
            .music
            .ignored_players
            .into_iter()
            .map(|item| item.trim().to_owned())
            .filter(|item| !item.is_empty())
            .collect();
        self.music.ignored_players.sort();
        self.music.ignored_players.dedup();
        self.controls.volume_max = if self.controls.volume_max.is_finite() {
            self.controls.volume_max.clamp(0.5, 1.5)
        } else {
            1.0
        };
        self.controls.brightness_min = if self.controls.brightness_min.is_finite() {
            self.controls.brightness_min.clamp(0.10, 0.80)
        } else {
            0.30
        };
        self.controls.xrandr_output = self
            .controls
            .xrandr_output
            .map(|output| output.trim().to_owned())
            .filter(|output| !output.is_empty());
        self.focus.focus_minutes = self.focus.focus_minutes.clamp(1, 180);
        self.focus.break_minutes = self.focus.break_minutes.clamp(1, 60);
        self.shelf.max_items = self.shelf.max_items.clamp(1, 32);
        self.clipboard.max_clips = self.clipboard.max_clips.clamp(5, 200);
        self
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    modified: Option<SystemTime>,
    len: u64,
    ino: u64,
}

impl FileStamp {
    fn read(path: &Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        Some(Self {
            modified: meta.modified().ok(),
            len: meta.len(),
            ino: meta.ino(),
        })
    }
}

/// Polls `config.json` metadata at a low rate so atomic saves from the
/// Settings window apply live in the running notch daemon.
pub struct ConfigWatcher {
    path: PathBuf,
    last_check: Instant,
    stamp: Option<FileStamp>,
}

impl ConfigWatcher {
    pub fn new(path: PathBuf) -> Self {
        let stamp = FileStamp::read(&path);
        Self {
            path,
            last_check: Instant::now(),
            stamp,
        }
    }

    pub fn poll(&mut self) -> Option<Config> {
        if self.last_check.elapsed() < WATCH_INTERVAL {
            return None;
        }
        self.poll_now()
    }

    pub fn poll_now(&mut self) -> Option<Config> {
        self.last_check = Instant::now();
        let current = FileStamp::read(&self.path);
        if current == self.stamp {
            return None;
        }
        self.stamp = current;
        Some(Config::load_from(&self.path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_json_fills_missing_fields_with_defaults() {
        let raw = r#"{
            "features": { "music": false, "notes": false },
            "focus": { "focus_minutes": 45 }
        }"#;
        let parsed: Config = serde_json::from_str(raw).unwrap();
        let clean = parsed.sanitized();
        assert_eq!(clean.version, CONFIG_VERSION);
        assert!(!clean.features.music);
        assert!(clean.features.clipboard);
        assert!(!clean.features.notes);
        assert!(clean.features.is_tab_enabled(TabId::Clips));
        assert_eq!(clean.focus.focus_minutes, 45);
        assert_eq!(clean.focus.break_minutes, 5);
        assert_eq!(clean.mascot.form, MascotForm::Kurama);
        assert!(clean.mascot.show_aura);
        assert!(clean.system.tray_icon);
    }

    #[test]
    fn sanitize_clamps_out_of_range_values_and_cleans_strings() {
        let cfg = Config {
            version: 99,
            appearance: AppearanceConfig {
                hover_open_delay_ms: 9999,
                hover_close_delay_ms: 9999,
                ..AppearanceConfig::default()
            },
            music: MusicConfig {
                seek_step_secs: 0,
                ignored_players: vec!["  ".into(), "firefox".into(), "firefox".into()],
                ..MusicConfig::default()
            },
            controls: ControlsConfig {
                volume_max: 8.0,
                brightness_min: 0.01,
                xrandr_output: Some("   ".into()),
            },
            focus: FocusConfig {
                focus_minutes: 0,
                break_minutes: 999,
                ..FocusConfig::default()
            },
            shelf: ShelfConfig {
                max_items: 0,
                ..ShelfConfig::default()
            },
            clipboard: ClipboardConfig { max_clips: 999 },
            ..Config::default()
        };

        let clean = cfg.sanitized();
        assert_eq!(clean.version, CONFIG_VERSION);
        assert_eq!(clean.appearance.hover_open_delay_ms, 500);
        assert_eq!(clean.appearance.hover_close_delay_ms, 800);
        assert_eq!(clean.music.seek_step_secs, 1);
        assert_eq!(clean.music.ignored_players, vec!["firefox".to_owned()]);
        assert_eq!(clean.controls.volume_max, 1.5);
        assert_eq!(clean.controls.brightness_min, 0.10);
        assert_eq!(clean.controls.xrandr_output, None);
        assert_eq!(clean.focus.focus_minutes, 1);
        assert_eq!(clean.focus.break_minutes, 60);
        assert_eq!(clean.shelf.max_items, 1);
        assert_eq!(clean.clipboard.max_clips, 200);
    }

    #[test]
    fn feature_toggles_resolve_and_step_only_enabled_tabs() {
        let features = FeaturesConfig {
            music: false,
            controls: true,
            volume: false,
            brightness: false,
            focus: true,
            shelf: false,
            clipboard: false,
            notes: true,
            default_tab: TabId::Clips,
            ..FeaturesConfig::default()
        };
        assert_eq!(features.enabled_tabs(), vec![TabId::Focus, TabId::Clips]);
        assert!(!features.is_tab_enabled(TabId::Control));
        assert_eq!(features.resolve_tab(TabId::Music), Some(TabId::Clips));
        assert_eq!(features.resolve_tab(TabId::Focus), Some(TabId::Focus));
        assert_eq!(features.step_tab(TabId::Focus, true), Some(TabId::Clips));
        assert_eq!(features.step_tab(TabId::Clips, true), Some(TabId::Focus));
        assert_eq!(features.step_tab(TabId::Focus, false), Some(TabId::Clips));
    }

    #[test]
    fn disabling_all_tabs_resolves_to_none() {
        let features = FeaturesConfig {
            music: false,
            controls: false,
            focus: false,
            shelf: false,
            clipboard: false,
            notes: false,
            ..FeaturesConfig::default()
        };
        assert!(features.enabled_tabs().is_empty());
        assert_eq!(features.resolve_tab(TabId::Music), None);
        assert_eq!(features.step_tab(TabId::Music, true), None);
    }
}
