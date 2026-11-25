//! Pomodoro timer for the Focus tab. Pure arithmetic, no external service.

use std::process::{Command, Stdio};

pub const FOCUS_SECS: u32 = 25 * 60;
pub const BREAK_SECS: u32 = 5 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Focus,
    Break,
}

impl Mode {
    #[allow(dead_code)]
    pub fn seconds(self) -> u32 {
        match self {
            Mode::Focus => FOCUS_SECS,
            Mode::Break => BREAK_SECS,
        }
    }

    #[allow(dead_code)]
    pub fn label(self) -> &'static str {
        match self {
            Mode::Focus => "Focus 25m",
            Mode::Break => "Break 5m",
        }
    }

    fn other(self) -> Self {
        match self {
            Mode::Focus => Mode::Break,
            Mode::Break => Mode::Focus,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Toggle,
    Reset,
    Set(Mode),
}

pub struct Timer {
    mode: Mode,
    remaining: f64,
    running: bool,
    /// Sessions finished since launch, shown as small dots.
    completed: u32,
    focus_secs: u32,
    break_secs: u32,
    notifications: bool,
    auto_start_next: bool,
}

impl Default for Timer {
    fn default() -> Self {
        Self {
            mode: Mode::Focus,
            remaining: FOCUS_SECS as f64,
            running: false,
            completed: 0,
            focus_secs: FOCUS_SECS,
            break_secs: BREAK_SECS,
            notifications: true,
            auto_start_next: false,
        }
    }
}

impl Timer {
    pub fn apply_config(
        &mut self,
        focus_minutes: u32,
        break_minutes: u32,
        notifications: bool,
        auto_start_next: bool,
    ) {
        let old_total = self.mode_seconds(self.mode) as f64;
        let at_full = !self.running && (self.remaining - old_total).abs() < 0.5;
        self.focus_secs = focus_minutes.clamp(1, 180) * 60;
        self.break_secs = break_minutes.clamp(1, 60) * 60;
        self.notifications = notifications;
        self.auto_start_next = auto_start_next;
        if at_full {
            self.remaining = self.mode_seconds(self.mode) as f64;
        }
    }

    pub fn mode_seconds(&self, mode: Mode) -> u32 {
        match mode {
            Mode::Focus => self.focus_secs,
            Mode::Break => self.break_secs,
        }
    }

    pub fn mode_label(&self, mode: Mode) -> String {
        match mode {
            Mode::Focus => format!("Focus {}m", self.focus_secs / 60),
            Mode::Break => format!("Break {}m", self.break_secs / 60),
        }
    }

    pub fn act(&mut self, action: Action) {
        match action {
            Action::Toggle => {
                if self.remaining <= 0.5 {
                    self.remaining = self.mode_seconds(self.mode) as f64;
                }
                self.running = !self.running;
            }
            Action::Reset => {
                self.running = false;
                self.remaining = self.mode_seconds(self.mode) as f64;
            }
            Action::Set(mode) => {
                self.mode = mode;
                self.running = false;
                self.remaining = self.mode_seconds(mode) as f64;
            }
        }
    }

    /// Advance the clock. Returns the mode that just finished, if any.
    pub fn tick(&mut self, dt: f64) -> Option<Mode> {
        if !self.running {
            return None;
        }
        self.remaining -= dt.max(0.0);
        if self.remaining > 0.0 {
            return None;
        }
        let finished = self.mode;
        self.completed += 1;
        self.running = self.auto_start_next;
        self.mode = finished.other();
        self.remaining = self.mode_seconds(self.mode) as f64;
        if self.notifications {
            let break_mins = self.break_secs / 60;
            notify(&match finished {
                Mode::Focus => format!("Focus session done. Take {break_mins} minutes."),
                Mode::Break => "Break over. Back to focus.".to_owned(),
            });
        }
        Some(finished)
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn running(&self) -> bool {
        self.running
    }

    pub fn completed(&self) -> u32 {
        self.completed
    }

    pub fn remaining(&self) -> f64 {
        self.remaining.max(0.0)
    }

    pub fn progress(&self) -> f32 {
        let total = self.mode_seconds(self.mode) as f64;
        if total <= 0.0 {
            return 0.0;
        }
        (1.0 - self.remaining / total).clamp(0.0, 1.0) as f32
    }

    /// `mm:ss`, floored so the display never shows a second that has not
    /// elapsed yet.
    pub fn clock(&self) -> String {
        let total = self.remaining().ceil() as u32;
        format!("{:02}:{:02}", total / 60, total % 60)
    }
}

fn notify(body: &str) {
    let _ = Command::new("notify-send")
        .args(["-a", "Glacier", "-t", "5000", "Glacier", body])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_counts_down_and_formats() {
        let mut timer = Timer::default();
        assert_eq!(timer.clock(), "25:00");
        timer.act(Action::Toggle);
        timer.tick(65.0);
        assert_eq!(timer.clock(), "23:55");
        assert!(timer.running());
    }

    #[test]
    fn finishing_switches_mode_and_stops() {
        let mut timer = Timer::default();
        timer.act(Action::Set(Mode::Break));
        timer.act(Action::Toggle);
        let finished = timer.tick(BREAK_SECS as f64 + 1.0);
        assert_eq!(finished, Some(Mode::Break));
        assert_eq!(timer.mode(), Mode::Focus);
        assert_eq!(timer.clock(), "25:00");
        assert!(!timer.running());
        assert_eq!(timer.completed(), 1);
    }

    #[test]
    fn paused_timer_ignores_time() {
        let mut timer = Timer::default();
        timer.tick(30.0);
        assert_eq!(timer.clock(), "25:00");
        timer.act(Action::Toggle);
        timer.tick(10.0);
        timer.act(Action::Toggle);
        timer.tick(10.0);
        assert_eq!(timer.clock(), "24:50");
    }

    #[test]
    fn reset_and_mode_switch_restore_full_duration() {
        let mut timer = Timer::default();
        timer.act(Action::Toggle);
        timer.tick(600.0);
        timer.act(Action::Reset);
        assert_eq!(timer.clock(), "25:00");
        assert!(!timer.running());
        timer.act(Action::Set(Mode::Break));
        assert_eq!(timer.clock(), "05:00");
        assert!((timer.progress() - 0.0).abs() < 1e-6);
    }
}
