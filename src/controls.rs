//! System controls for the Control tab: PipeWire/Pulse sink volume and screen
//! brightness.
//!
//! Brightness has no `/sys/class/backlight` device on this desktop, and the
//! NVIDIA driver ignores `xrandr --brightness`, so the only path that actually
//! moves is the CRTC gamma ramp. That is a software dim: it scales what the
//! monitor receives without touching any backlight.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const VOLUME_MAX: f32 = 1.0;
const BRIGHTNESS_MIN: f32 = 0.30;
const REFRESH_EVERY: Duration = Duration::from_millis(1200);
const APPLY_EVERY: Duration = Duration::from_millis(45);

pub struct Controls {
    pub volume: f32,
    pub brightness: f32,
    pub muted: bool,
    /// `wpctl` is preferred; `pactl` covers boxes that only ship PulseAudio.
    backend: Backend,
    output: Option<String>,
    volume_enabled: bool,
    brightness_enabled: bool,
    volume_max: f32,
    brightness_min: f32,
    last_volume: Instant,
    last_brightness: Instant,
    last_refresh: Instant,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Backend {
    Wpctl,
    Pactl,
    None,
}

fn which(program: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {program}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

impl Controls {
    pub fn new() -> Self {
        let backend = if which("wpctl") {
            Backend::Wpctl
        } else if which("pactl") {
            Backend::Pactl
        } else {
            Backend::None
        };
        let mut controls = Self {
            volume: 1.0,
            brightness: 1.0,
            muted: false,
            backend,
            output: detect_output(),
            volume_enabled: true,
            brightness_enabled: true,
            volume_max: VOLUME_MAX,
            brightness_min: BRIGHTNESS_MIN,
            last_volume: Instant::now() - Duration::from_secs(1),
            last_brightness: Instant::now() - Duration::from_secs(1),
            last_refresh: Instant::now() - Duration::from_secs(5),
        };
        controls.refresh();
        controls
    }

    pub fn has_volume(&self) -> bool {
        self.volume_enabled && self.backend != Backend::None
    }

    pub fn has_brightness(&self) -> bool {
        self.brightness_enabled && self.output.is_some()
    }

    pub fn volume_max(&self) -> f32 {
        self.volume_max
    }

    pub fn apply_config(
        &mut self,
        volume_enabled: bool,
        brightness_enabled: bool,
        volume_max: f32,
        brightness_min: f32,
        preferred_output: Option<&str>,
    ) {
        if self.brightness_enabled && !brightness_enabled {
            self.restore_brightness();
        }
        self.volume_enabled = volume_enabled;
        self.brightness_enabled = brightness_enabled;
        self.volume_max = volume_max.clamp(0.5, 1.5);
        self.brightness_min = brightness_min.clamp(0.10, 0.80);
        self.volume = self.volume.clamp(0.0, self.volume_max);
        self.brightness = self.brightness.clamp(self.brightness_min, 1.0);
        self.output = resolve_output(preferred_output);
    }

    pub fn refresh(&mut self) {
        if !self.volume_enabled || self.last_refresh.elapsed() < REFRESH_EVERY {
            return;
        }
        self.last_refresh = Instant::now();
        let Some(raw) = self.read_volume() else {
            return;
        };
        if let Some((volume, muted)) = parse_volume(&raw) {
            self.volume = volume.clamp(0.0, self.volume_max);
            self.muted = muted;
        }
    }

    fn read_volume(&self) -> Option<String> {
        let output = match self.backend {
            Backend::Wpctl => Command::new("wpctl")
                .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
                .output()
                .ok()?,
            Backend::Pactl => Command::new("pactl")
                .args(["get-sink-volume", "@DEFAULT_SINK@"])
                .output()
                .ok()?,
            Backend::None => return None,
        };
        String::from_utf8(output.stdout).ok()
    }

    /// `force` bypasses the rate limit so the value the pointer was released on
    /// is always the value the sink ends up with.
    pub fn set_volume(&mut self, value: f32, force: bool) {
        if !self.volume_enabled {
            return;
        }
        let value = value.clamp(0.0, self.volume_max);
        self.volume = value;
        if !force && self.last_volume.elapsed() < APPLY_EVERY {
            return;
        }
        self.last_volume = Instant::now();
        let text = format!("{value:.2}");
        let _ = match self.backend {
            Backend::Wpctl => Command::new("wpctl")
                .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &text])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status(),
            Backend::Pactl => Command::new("pactl")
                .args([
                    "set-sink-volume",
                    "@DEFAULT_SINK@",
                    &format!("{}%", (value * 100.0).round() as i32),
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status(),
            Backend::None => return,
        };
        self.muted = false;
    }

    pub fn set_brightness(&mut self, value: f32, force: bool) {
        if !self.brightness_enabled {
            return;
        }
        let value = value.clamp(self.brightness_min, 1.0);
        self.brightness = value;
        if !force && self.last_brightness.elapsed() < APPLY_EVERY {
            return;
        }
        self.last_brightness = Instant::now();
        let Some(output) = self.output.as_deref() else {
            return;
        };
        let gamma = format!("{value:.3}:{value:.3}:{value:.3}");
        let _ = Command::new("xrandr")
            .args(["--output", output, "--gamma", &gamma])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    /// Put the gamma ramp back so quitting does not leave the screen dimmed.
    pub fn restore_brightness(&mut self) {
        self.brightness = 1.0;
        let Some(output) = self.output.as_deref() else {
            return;
        };
        let _ = Command::new("xrandr")
            .args(["--output", output, "--gamma", "1.000:1.000:1.000"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

pub fn detected_audio_backend_label() -> &'static str {
    if which("wpctl") {
        "PipeWire (wpctl)"
    } else if which("pactl") {
        "PulseAudio (pactl)"
    } else {
        "Not detected"
    }
}

pub fn list_outputs() -> Vec<String> {
    let Ok(output) = Command::new("xrandr").arg("--query").output() else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let mut outputs = Vec::new();
    for line in text.lines() {
        if let Some((name, rest)) = line.split_once(' ')
            && rest.trim_start().starts_with("connected")
            && !name.contains("VIRTUAL")
        {
            outputs.push(name.to_owned());
        }
    }
    outputs
}

pub fn reset_gamma(preferred_output: Option<&str>) {
    let Some(output) = resolve_output(preferred_output) else {
        return;
    };
    let _ = Command::new("xrandr")
        .args(["--output", &output, "--gamma", "1.000:1.000:1.000"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn resolve_output(preferred: Option<&str>) -> Option<String> {
    let outputs = list_outputs();
    if let Some(wanted) = preferred.map(str::trim).filter(|s| !s.is_empty())
        && outputs.iter().any(|name| name == wanted)
    {
        return Some(wanted.to_owned());
    }
    outputs.into_iter().next()
}

/// First connected output that is not a virtual/offscreen one.
fn detect_output() -> Option<String> {
    list_outputs().into_iter().next()
}

/// `Volume: 1.06` or `Volume: 0.45 [MUTED]`.
fn parse_volume(raw: &str) -> Option<(f32, bool)> {
    let line = raw.lines().next()?;
    let value = line.split_whitespace().nth(1)?.parse::<f32>().ok()?;
    Some((value, line.contains("[MUTED]")))
}

/// Pango-free percentage for the slider label, rounded so 1.06 reads as 106%.
pub fn percent(value: f32) -> String {
    format!("{}%", (value * 100.0).round() as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_wpctl_volume_lines() {
        assert_eq!(parse_volume("Volume: 1.06\n"), Some((1.06, false)));
        assert_eq!(parse_volume("Volume: 0.45 [MUTED]\n"), Some((0.45, true)));
        assert_eq!(parse_volume("garbage"), None);
    }

    #[test]
    fn percent_rounds_to_whole_numbers() {
        assert_eq!(percent(1.06), "106%");
        assert_eq!(percent(0.0), "0%");
        assert_eq!(percent(0.455), "46%");
    }

    #[test]
    fn volume_is_clamped_to_one_hundred_percent() {
        let mut controls = Controls {
            volume: 1.0,
            brightness: 1.0,
            muted: false,
            backend: Backend::None,
            output: None,
            volume_enabled: true,
            brightness_enabled: true,
            volume_max: VOLUME_MAX,
            brightness_min: BRIGHTNESS_MIN,
            last_volume: Instant::now(),
            last_brightness: Instant::now(),
            last_refresh: Instant::now(),
        };
        controls.set_volume(4.0, true);
        assert_eq!(controls.volume, 1.0);
        controls.set_volume(-1.0, true);
        assert_eq!(controls.volume, 0.0);
    }
}
