//! MPRIS media reader/controller over D-Bus.
//!
//! Runs on its own thread: polls the session bus every 400 ms for whatever
//! player is registered (`org.mpris.MediaPlayer2.*`), reports a snapshot to the
//! UI over a channel and takes commands back the other way. Seeking and rate
//! changes go through the same channel so the UI never blocks on D-Bus.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{ObjectPath, OwnedValue};

const POLL: Duration = Duration::from_millis(400);

/// Playback rates the speed badge cycles through.
pub const RATES: [f64; 5] = [1.0, 1.25, 1.5, 2.0, 0.75];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Stopped,
    Playing,
    Paused,
}

#[derive(Clone, Debug)]
pub struct Track {
    pub player: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art_url: Option<String>,
    pub status: Status,
    pub position: f64,
    pub length: Option<f64>,
    pub can_next: bool,
    pub can_prev: bool,
    pub can_seek: bool,
    pub can_control: bool,
    pub rate: f64,
    /// `mpris:trackid`, needed by `SetPosition`. Opaque to us.
    pub track_id: Option<String>,
}

impl Default for Track {
    fn default() -> Self {
        Self {
            player: String::new(),
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            art_url: None,
            status: Status::Stopped,
            position: 0.0,
            length: None,
            can_next: false,
            can_prev: false,
            can_seek: false,
            can_control: false,
            rate: 1.0,
            track_id: None,
        }
    }
}

pub fn local_art_path(url: &str) -> Option<std::path::PathBuf> {
    if url.starts_with("file://") {
        let url = url::Url::parse(url).ok()?;
        if url.scheme() != "file"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.host().is_some()
        {
            return None;
        }
        return url.to_file_path().ok();
    }
    let path = std::path::Path::new(url);
    (path.is_absolute() && path.is_file()).then(|| path.to_path_buf())
}

impl Track {
    pub fn is_playing(&self) -> bool {
        self.status == Status::Playing
    }

    pub fn progress(&self) -> f32 {
        match self.length {
            Some(len) if len > 0.0 => (self.position / len).clamp(0.0, 1.0) as f32,
            _ => 0.0,
        }
    }

    pub fn remaining(&self) -> Option<f64> {
        self.length.map(|len| (len - self.position).max(0.0))
    }
}

/// MPRIS players may return the same Position on every poll. Anchor their
/// reported position to monotonic time; only reset for a new track, a real
/// seek, or a play/pause transition.
pub struct PlaybackClock {
    anchor: f64,
    at: Instant,
    last_reported: f64,
    identity: (String, String, String),
    playing: bool,
    rate: f64,
}

impl PlaybackClock {
    pub fn new(now: Instant) -> Self {
        Self {
            anchor: 0.0,
            at: now,
            last_reported: 0.0,
            identity: (String::new(), String::new(), String::new()),
            playing: false,
            rate: 1.0,
        }
    }

    pub fn observe(&mut self, track: &Track, now: Instant) {
        let identity = (
            track.player.clone(),
            track.title.clone(),
            track.artist.clone(),
        );
        let rate = if track.rate.is_finite() && track.rate > 0.0 {
            track.rate
        } else {
            1.0
        };
        let expected = self.position(now);
        let moved = (track.position - self.last_reported).abs() > 0.05;
        if identity != self.identity
            || track.is_playing() != self.playing
            || (moved && (track.position - expected).abs() > 1.5)
        {
            self.anchor = track.position.max(0.0);
            self.at = now;
        } else if (rate - self.rate).abs() > 0.001 {
            self.anchor = expected;
            self.at = now;
        }
        self.last_reported = track.position;
        self.playing = track.is_playing();
        self.rate = rate;
        self.identity = identity;
    }

    pub fn position(&self, now: Instant) -> f64 {
        self.anchor
            + if self.playing {
                now.saturating_duration_since(self.at).as_secs_f64() * self.rate
            } else {
                0.0
            }
    }

    /// Force the interpolated position after an intentional jump, so the
    /// progress bar does not snap back while the player catches up.
    pub fn jump_to(&mut self, position: f64, now: Instant) {
        self.anchor = position.max(0.0);
        self.last_reported = position.max(0.0);
        self.at = now;
    }
}

#[derive(Clone, Debug)]
pub enum Cmd {
    PlayPause,
    Next,
    Previous,
    /// Absolute position in seconds.
    SeekTo(f64),
    /// Relative nudge in seconds.
    SeekBy(f64),
    SetRate(f64),
    Configure {
        enabled: bool,
        ignored_players: Vec<String>,
    },
}

pub struct Media {
    rx: Receiver<Track>,
    tx: Sender<Cmd>,
}

impl Media {
    /// Spawn the D-Bus worker. With `offline` the worker never starts and
    /// `poll` returns nothing, which is what the demo mode wants.
    #[allow(dead_code)]
    pub fn start(offline: bool) -> Self {
        Self::start_with_config(offline, true, Vec::new())
    }

    pub fn start_with_config(offline: bool, enabled: bool, ignored_players: Vec<String>) -> Self {
        let (track_tx, track_rx) = channel();
        let (cmd_tx, cmd_rx) = channel();
        if !offline {
            thread::spawn(move || worker(track_tx, cmd_rx, enabled, ignored_players));
        }
        Self {
            rx: track_rx,
            tx: cmd_tx,
        }
    }

    /// Latest snapshot, or `None` when nothing new arrived since the last call.
    pub fn poll(&self) -> Option<Track> {
        let mut latest = None;
        while let Ok(track) = self.rx.try_recv() {
            latest = Some(track);
        }
        latest
    }

    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }
}

fn worker(
    tx: Sender<Track>,
    rx: Receiver<Cmd>,
    mut enabled: bool,
    mut ignored_players: Vec<String>,
) {
    let mut conn: Option<Connection> = None;
    let mut emitted_empty = false;

    loop {
        let mut pending = Vec::new();
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                Cmd::Configure {
                    enabled: next_enabled,
                    ignored_players: next_ignored,
                } => {
                    enabled = next_enabled;
                    ignored_players = next_ignored;
                    if !enabled {
                        emitted_empty = false;
                    }
                }
                other => pending.push(other),
            }
        }

        if !enabled {
            if !emitted_empty {
                let _ = tx.send(Track::default());
                emitted_empty = true;
            }
            thread::sleep(POLL);
            continue;
        }
        emitted_empty = false;

        if conn.is_none() {
            conn = Connection::session().ok();
        }

        if let Some(c) = conn.as_ref() {
            let (track, target) = match snapshot_filtered(c, &ignored_players) {
                Ok((track, name)) => (track, Some(name)),
                Err(_) => (Track::default(), None),
            };
            let _ = tx.send(track.clone());

            for cmd in pending {
                if let Some(dest) = target.as_deref()
                    && let Err(error) = dispatch(c, cmd, dest, &track)
                {
                    eprintln!("Glacier MPRIS command failed: {error}");
                }
            }
        }

        thread::sleep(POLL);
    }
}

fn is_ignored_player(bus_name: &str, identity: &str, ignored: &[String]) -> bool {
    let bus_lower = bus_name.to_lowercase();
    let id_lower = identity.to_lowercase();
    ignored.iter().any(|pattern| {
        let needle = pattern.trim().to_lowercase();
        !needle.is_empty() && (bus_lower.contains(&needle) || id_lower.contains(&needle))
    })
}

/// Pick the player to display: a playing one wins, otherwise the first that
/// exposes the MPRIS player interface.
#[allow(dead_code)]
fn snapshot(conn: &Connection) -> Result<(Track, String), Box<dyn std::error::Error>> {
    snapshot_filtered(conn, &[])
}

fn snapshot_filtered(
    conn: &Connection,
    ignored_players: &[String],
) -> Result<(Track, String), Box<dyn std::error::Error>> {
    let bus = Proxy::new(
        conn,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )?;
    let names: Vec<String> = bus.call("ListNames", &())?;

    let mut candidates: Vec<String> = names
        .into_iter()
        .filter(|n| n.starts_with("org.mpris.MediaPlayer2."))
        .collect();
    candidates.sort();

    let mut best: Option<(Track, String)> = None;

    for name in candidates {
        if is_ignored_player(&name, "", ignored_players) {
            continue;
        }
        let Ok(track) = read_player(conn, &name) else {
            continue;
        };
        if is_ignored_player(&name, &track.player, ignored_players) {
            continue;
        }
        if track.is_playing() {
            return Ok((track, name));
        }
        if best.is_none() {
            best = Some((track, name));
        }
    }

    best.ok_or_else(|| "no MPRIS player".into())
}

fn read_player(conn: &Connection, dest: &str) -> Result<Track, Box<dyn std::error::Error>> {
    let player = Proxy::new(
        conn,
        dest,
        "/org/mpris/MediaPlayer2",
        "org.mpris.MediaPlayer2.Player",
    )?;
    let root = Proxy::new(
        conn,
        dest,
        "/org/mpris/MediaPlayer2",
        "org.mpris.MediaPlayer2",
    )?;

    let mut track = Track {
        player: root.get_property::<String>("Identity").unwrap_or_default(),
        status: match player
            .get_property::<String>("PlaybackStatus")
            .unwrap_or_default()
            .as_str()
        {
            "Playing" => Status::Playing,
            "Paused" => Status::Paused,
            _ => Status::Stopped,
        },
        position: player.get_property::<i64>("Position").unwrap_or(0) as f64 / 1e6,
        can_next: player.get_property::<bool>("CanGoNext").unwrap_or(false),
        can_prev: player
            .get_property::<bool>("CanGoPrevious")
            .unwrap_or(false),
        can_seek: player.get_property::<bool>("CanSeek").unwrap_or(false),
        can_control: player.get_property::<bool>("CanControl").unwrap_or(true),
        rate: player.get_property::<f64>("Rate").unwrap_or(1.0),
        ..Default::default()
    };

    if let Ok(meta) = player.get_property::<HashMap<String, OwnedValue>>("Metadata") {
        track.title = text(meta.get("xesam:title")).unwrap_or_default();
        track.artist = extract_artist(&meta);
        track.album = text(meta.get("xesam:album")).unwrap_or_default();
        track.art_url = text(meta.get("mpris:artUrl"));
        track.length = extract_length(meta.get("mpris:length"));
        track.track_id = meta
            .get("mpris:trackid")
            .and_then(|v| ObjectPath::try_from(v.try_clone().ok()?).ok())
            .map(|path| path.as_str().to_owned());
    }

    Ok(track)
}

fn text(value: Option<&OwnedValue>) -> Option<String> {
    value
        .and_then(|v| v.downcast_ref::<String>().ok())
        .filter(|s| !s.is_empty())
}

fn list(value: Option<&OwnedValue>) -> Option<String> {
    if let Some(parts) = value.and_then(|v| Vec::<String>::try_from(v.try_clone().ok()?).ok()) {
        let joined = parts
            .into_iter()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
        if !joined.is_empty() {
            return Some(joined);
        }
    }
    text(value)
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

fn extract_artist(meta: &HashMap<String, OwnedValue>) -> String {
    list(meta.get("xesam:artist"))
        .or_else(|| list(meta.get("xesam:albumArtist")))
        .unwrap_or_default()
}

fn extract_length(value: Option<&OwnedValue>) -> Option<f64> {
    let value = value?;
    let micros = if let Ok(len) = value.downcast_ref::<i64>() {
        (len > 0).then_some(len as f64)
    } else if let Ok(len) = value.downcast_ref::<u64>() {
        (len > 0).then_some(len as f64)
    } else if let Ok(len) = value.downcast_ref::<i32>() {
        (len > 0).then_some(len as f64)
    } else if let Ok(len) = value.downcast_ref::<u32>() {
        (len > 0).then_some(len as f64)
    } else {
        None
    }?;
    Some(micros / 1e6)
}

/// Seek target clamped to the track, so a click near the edges cannot land on
/// a negative timestamp.
pub fn clamp_seek(target: f64, length: Option<f64>) -> f64 {
    let lower = target.max(0.0);
    match length {
        Some(len) if len > 0.0 => lower.min(len),
        _ => lower,
    }
}

/// Next rate in the cycle, snapping to a known step so a player that reports
/// 0.9999 does not produce a value outside the list.
pub fn next_rate(current: f64) -> f64 {
    let idx = RATES
        .iter()
        .position(|r| (r - current).abs() < 0.01)
        .unwrap_or(0);
    RATES[(idx + 1) % RATES.len()]
}

fn dispatch(conn: &Connection, what: Cmd, dest: &str, last: &Track) -> zbus::Result<()> {
    let player = Proxy::new(
        conn,
        dest,
        "/org/mpris/MediaPlayer2",
        "org.mpris.MediaPlayer2.Player",
    )?;

    match what {
        Cmd::PlayPause => {
            player.call::<_, _, ()>("PlayPause", &())?;
        }
        Cmd::Next => {
            player.call::<_, _, ()>("Next", &())?;
        }
        Cmd::Previous => {
            player.call::<_, _, ()>("Previous", &())?;
        }
        Cmd::SeekTo(seconds) => {
            if !last.can_seek {
                return Ok(());
            }
            let micros = (clamp_seek(seconds, last.length) * 1e6) as i64;
            if let Some(id) = last.track_id.as_deref()
                && let Ok(path) = ObjectPath::try_from(id)
                && player
                    .call::<_, _, ()>("SetPosition", &(path, micros))
                    .is_ok()
            {
                return Ok(());
            }
            // Players without a usable track id still accept a relative seek.
            let delta = micros - (last.position * 1e6) as i64;
            player.call::<_, _, ()>("Seek", &delta)?;
        }
        Cmd::SeekBy(delta) => {
            if !last.can_seek {
                return Ok(());
            }
            player.call::<_, _, ()>("Seek", &((delta * 1e6) as i64))?;
        }
        Cmd::SetRate(rate) => {
            if last.can_control {
                player.set_property("Rate", rate)?;
            }
        }
        Cmd::Configure { .. } => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_local_art_paths_and_rejects_remote_urls() {
        let path = std::env::current_dir().unwrap().join("Cargo.toml");
        assert_eq!(local_art_path(path.to_str().unwrap()), Some(path.clone()));
        let file_url = url::Url::from_file_path(&path).unwrap().to_string();
        assert_eq!(local_art_path(&file_url), Some(path));
        assert!(local_art_path("https://example.invalid/cover.jpg").is_none());
        assert!(local_art_path("relative/cover.jpg").is_none());
    }

    #[test]
    fn repeated_stale_position_does_not_reset_lyrics_clock() {
        let start = std::time::Instant::now();
        let mut clock = PlaybackClock::new(start);
        let mut track = Track {
            title: "Test Song".into(),
            artist: "Test Artist".into(),
            status: Status::Playing,
            position: 60.0,
            length: Some(180.0),
            ..Track::default()
        };
        clock.observe(&track, start);
        clock.observe(&track, start + Duration::from_millis(400));
        clock.observe(&track, start + Duration::from_millis(800));
        assert!((clock.position(start + Duration::from_secs(2)) - 62.0).abs() < 0.001);
        track.position = 93.0;
        clock.observe(&track, start + Duration::from_secs(3));
        assert!((clock.position(start + Duration::from_secs(3)) - 93.0).abs() < 0.001);
        track.status = Status::Paused;
        clock.observe(&track, start + Duration::from_secs(4));
        assert!((clock.position(start + Duration::from_secs(9)) - 93.0).abs() < 0.001);
    }

    #[test]
    fn playback_clock_tracks_reported_rate_for_lyrics() {
        let start = std::time::Instant::now();
        let mut clock = PlaybackClock::new(start);
        let track = Track {
            title: "Fast Track".into(),
            artist: "Test Artist".into(),
            status: Status::Playing,
            position: 30.0,
            length: Some(180.0),
            rate: 1.5,
            ..Track::default()
        };
        clock.observe(&track, start);
        assert!((clock.position(start + Duration::from_secs(4)) - 36.0).abs() < 0.001);
        let mut changed_rate = track.clone();
        changed_rate.rate = 2.0;
        clock.observe(&changed_rate, start + Duration::from_secs(4));
        assert!((clock.position(start + Duration::from_secs(6)) - 40.0).abs() < 0.001);
    }

    #[test]
    fn progress_handles_missing_and_extreme_positions() {
        let mut track = Track::default();
        assert_eq!(track.progress(), 0.0);
        track.length = Some(180.0);
        track.position = 60.0;
        assert!((track.progress() - 1.0 / 3.0).abs() < 0.0001);
        track.position = -100.0;
        assert_eq!(track.progress(), 0.0);
        track.position = 1000.0;
        assert_eq!(track.progress(), 1.0);
    }

    #[test]
    fn seek_targets_are_clamped_to_the_track() {
        assert_eq!(clamp_seek(-12.0, Some(180.0)), 0.0);
        assert_eq!(clamp_seek(90.0, Some(180.0)), 90.0);
        assert_eq!(clamp_seek(400.0, Some(180.0)), 180.0);
        assert_eq!(clamp_seek(30.0, None), 30.0);
    }

    #[test]
    fn rate_cycle_wraps_through_every_step() {
        let mut rate = 1.0;
        let mut seen = Vec::new();
        for _ in 0..RATES.len() {
            rate = next_rate(rate);
            seen.push(rate);
        }
        assert_eq!(seen.len(), RATES.len());
        assert!((rate - 1.0).abs() < 1e-9, "cycle returned {rate}");
        assert_eq!(next_rate(0.9999), 1.25);
    }

    #[test]
    fn jump_moves_the_interpolated_clock() {
        let start = std::time::Instant::now();
        let mut clock = PlaybackClock::new(start);
        let track = Track {
            title: "T".into(),
            artist: "A".into(),
            status: Status::Playing,
            position: 10.0,
            length: Some(100.0),
            ..Track::default()
        };
        clock.observe(&track, start);
        clock.jump_to(70.0, start);
        assert!((clock.position(start) - 70.0).abs() < 0.001);
        assert!((clock.position(start + Duration::from_secs(2)) - 72.0).abs() < 0.001);
    }

    #[test]
    fn artist_extraction_handles_scalar_and_album_artist_fallback() {
        let mut meta = HashMap::new();
        meta.insert(
            "xesam:artist".to_owned(),
            OwnedValue::try_from(zbus::zvariant::Value::from("Solo Artist")).unwrap(),
        );
        assert_eq!(extract_artist(&meta), "Solo Artist");

        meta.insert(
            "xesam:artist".to_owned(),
            OwnedValue::try_from(zbus::zvariant::Value::from(vec![
                "Artist A".to_owned(),
                "Artist B".to_owned(),
            ]))
            .unwrap(),
        );
        assert_eq!(extract_artist(&meta), "Artist A, Artist B");

        meta.insert(
            "xesam:artist".to_owned(),
            OwnedValue::try_from(zbus::zvariant::Value::from(Vec::<String>::new())).unwrap(),
        );
        meta.insert(
            "xesam:albumArtist".to_owned(),
            OwnedValue::try_from(zbus::zvariant::Value::from(vec!["Album Artist".to_owned()]))
                .unwrap(),
        );
        assert_eq!(extract_artist(&meta), "Album Artist");

        meta.remove("xesam:artist");
        meta.insert(
            "xesam:albumArtist".to_owned(),
            OwnedValue::try_from(zbus::zvariant::Value::from("Scalar Album Artist")).unwrap(),
        );
        assert_eq!(extract_artist(&meta), "Scalar Album Artist");
    }

    #[test]
    fn length_extraction_handles_integer_types_and_non_positive_values() {
        assert_eq!(
            extract_length(Some(&OwnedValue::from(180_000_000_i64))),
            Some(180.0)
        );
        assert_eq!(
            extract_length(Some(&OwnedValue::from(215_500_000_u64))),
            Some(215.5)
        );
        assert_eq!(
            extract_length(Some(&OwnedValue::from(120_000_000_i32))),
            Some(120.0)
        );
        assert_eq!(
            extract_length(Some(&OwnedValue::from(90_000_000_u32))),
            Some(90.0)
        );
        assert_eq!(extract_length(Some(&OwnedValue::from(0_i64))), None);
        assert_eq!(extract_length(Some(&OwnedValue::from(-10_000_i64))), None);
        assert_eq!(extract_length(Some(&OwnedValue::from(0_u64))), None);
        assert_eq!(extract_length(Some(&OwnedValue::from(0_i32))), None);
        assert_eq!(extract_length(Some(&OwnedValue::from(0_u32))), None);
        assert_eq!(extract_length(None), None);
    }

    #[test]
    #[ignore = "requires a running session bus and MPRIS player"]
    fn live_mpris_probe() {
        let conn = Connection::session().unwrap();
        let (track, player_name) = snapshot(&conn).unwrap();
        assert_eq!(player_name, "org.mpris.MediaPlayer2.GlacierProbe");
        assert_eq!(track.title, "Real D-Bus Test Track");
        assert_eq!(track.artist, "Test Artist");
        assert_eq!(track.length, Some(180.0));
        assert_eq!(track.position, 60.0);
        assert!(track.can_next && track.can_prev && track.can_seek);
        assert_eq!(track.rate, 1.0);
        assert!(track.track_id.is_some());

        let original_status = track.status;
        dispatch(&conn, Cmd::PlayPause, &player_name, &track).unwrap();
        assert_ne!(
            read_player(&conn, &player_name).unwrap().status,
            original_status
        );
        dispatch(&conn, Cmd::PlayPause, &player_name, &track).unwrap();

        dispatch(&conn, Cmd::SeekTo(30.0), &player_name, &track).unwrap();
        let moved = read_player(&conn, &player_name).unwrap();
        assert!((moved.position - 30.0).abs() < 0.2, "{moved:?}");

        dispatch(&conn, Cmd::SeekBy(5.0), &player_name, &moved).unwrap();
        let nudged = read_player(&conn, &player_name).unwrap();
        assert!((nudged.position - 35.0).abs() < 0.2, "{nudged:?}");

        dispatch(&conn, Cmd::SetRate(1.5), &player_name, &nudged).unwrap();
        let fast = read_player(&conn, &player_name).unwrap();
        assert!((fast.rate - 1.5).abs() < 0.01, "{fast:?}");
        dispatch(&conn, Cmd::SetRate(1.0), &player_name, &fast).unwrap();

        dispatch(&conn, Cmd::Next, &player_name, &track).unwrap();
        dispatch(&conn, Cmd::Previous, &player_name, &track).unwrap();
    }
}
