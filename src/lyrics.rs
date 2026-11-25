use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::Duration;

use serde::Deserialize;

#[derive(Clone, Debug, PartialEq)]
pub enum LyricState {
    Loading,
    Synced(Vec<Line>),
    Plain(String),
    Instrumental,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub at: f64,
    pub text: String,
}

struct LyricRequest {
    title: String,
    artist: String,
    duration: Option<f64>,
    online_lrclib: bool,
}

pub struct Lyrics {
    tx: Sender<LyricRequest>,
    rx: Receiver<(String, LyricState)>,
    pub key: String,
    pub state: LyricState,
    raw_title: String,
    raw_artist: String,
    duration: Option<f64>,
    enabled: bool,
    online_lrclib: bool,
}

impl Lyrics {
    pub fn new() -> Self {
        let (tx, requests) = channel::<LyricRequest>();
        let (answers, rx) = channel();
        thread::spawn(move || {
            let mut cache = HashMap::<String, LyricState>::new();
            while let Ok(request) = requests.recv() {
                let key = track_key(&request.title, &request.artist);
                let cache_key = format!(
                    "{key}\0{}\0{}",
                    request
                        .duration
                        .map(|seconds| format!("{seconds:.0}"))
                        .unwrap_or_default(),
                    u8::from(request.online_lrclib),
                );
                let state = cache.entry(cache_key).or_insert_with(|| {
                    lookup_with_policy(
                        &request.title,
                        &request.artist,
                        request.duration,
                        request.online_lrclib,
                    )
                });
                if answers.send((key, state.clone())).is_err() {
                    break;
                }
            }
        });
        Self {
            tx,
            rx,
            key: String::new(),
            state: LyricState::Unavailable,
            raw_title: String::new(),
            raw_artist: String::new(),
            duration: None,
            enabled: true,
            online_lrclib: true,
        }
    }

    pub fn set_config(&mut self, enabled: bool, online_lrclib: bool) {
        let changed = self.enabled != enabled || self.online_lrclib != online_lrclib;
        self.enabled = enabled;
        self.online_lrclib = online_lrclib;
        if !enabled {
            self.state = LyricState::Unavailable;
            return;
        }
        if changed && !self.raw_title.is_empty() {
            self.state = LyricState::Loading;
            let _ = self.tx.send(LyricRequest {
                title: self.raw_title.clone(),
                artist: self.raw_artist.clone(),
                duration: self.duration,
                online_lrclib: self.online_lrclib,
            });
        }
    }

    pub fn update_track(&mut self, title: &str, artist: &str, duration: Option<f64>) {
        let key = track_key(title, artist);
        if key == self.key {
            return;
        }
        self.key = key;
        self.raw_title = title.to_owned();
        self.raw_artist = artist.to_owned();
        self.duration = duration;
        if title.is_empty() || !self.enabled {
            self.state = LyricState::Unavailable;
        } else {
            self.state = LyricState::Loading;
            let _ = self.tx.send(LyricRequest {
                title: title.to_owned(),
                artist: artist.to_owned(),
                duration,
                online_lrclib: self.online_lrclib,
            });
        }
    }

    pub fn poll(&mut self) {
        while let Ok((key, state)) = self.rx.try_recv() {
            if self.enabled && key == self.key {
                self.state = state;
            }
        }
    }

    pub fn update_duration(&mut self, duration: Option<f64>) {
        if self.duration == duration || self.raw_title.is_empty() {
            return;
        }
        self.duration = duration;
        if !self.enabled {
            return;
        }
        self.state = LyricState::Loading;
        let _ = self.tx.send(LyricRequest {
            title: self.raw_title.clone(),
            artist: self.raw_artist.clone(),
            duration,
            online_lrclib: self.online_lrclib,
        });
    }

    pub fn synced(&self) -> Option<&[Line]> {
        match &self.state {
            LyricState::Synced(lines) => Some(lines),
            _ => None,
        }
    }

    /// Index of the active line and of the line after it, for the same
    /// partition the UI paints and the tests assert on.
    pub fn window(&self, pos: f64) -> Option<(usize, Option<usize>)> {
        let lines = self.synced()?;
        let index = lines
            .partition_point(|line| line.at <= pos.max(0.0))
            .checked_sub(1)?;
        let next = (index + 1 < lines.len()).then_some(index + 1);
        Some((index, next))
    }
}

fn track_key(title: &str, artist: &str) -> String {
    format!(
        "{}\0{}",
        title.trim().to_lowercase(),
        artist.trim().to_lowercase()
    )
}

fn parse_lrc(input: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    let offset = input
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("[offset:")?
                .split(']')
                .next()?
                .parse::<i64>()
                .ok()
        })
        .unwrap_or(0) as f64
        / 1000.0;
    for row in input.lines().take(2000) {
        let mut rest = row.trim();
        let mut stamps = Vec::new();
        while let Some(after) = rest.strip_prefix('[') {
            let Some((token, tail)) = after.split_once(']') else {
                break;
            };
            if let Some(at) = parse_stamp(token) {
                stamps.push((at - offset).max(0.0));
            }
            rest = tail;
        }
        let text = rest.trim();
        if text.is_empty() {
            continue;
        }
        for at in stamps {
            lines.push(Line {
                at,
                text: text.chars().take(240).collect(),
            });
        }
    }
    lines.sort_by(|a, b| a.at.total_cmp(&b.at));
    lines
}

fn parse_stamp(s: &str) -> Option<f64> {
    let (minutes, seconds) = s.split_once(':')?;
    if minutes.is_empty() || !minutes.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (secs, fraction) = seconds.split_once('.').unwrap_or((seconds, ""));
    if secs.is_empty()
        || secs.len() > 2
        || !secs.bytes().all(|b| b.is_ascii_digit())
        || fraction.len() > 3
        || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let sec = secs.parse::<u32>().ok()?;
    if sec > 99 {
        return None;
    }
    let frac = if fraction.is_empty() {
        0.0
    } else {
        fraction.parse::<u32>().ok()? as f64 / 10_f64.powi(fraction.len() as i32)
    };
    Some(minutes.parse::<u32>().ok()? as f64 * 60.0 + sec as f64 + frac)
}

fn local_path(title: &str, artist: &str) -> Vec<PathBuf> {
    let root = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
        })
        .join("glacier/lyrics");
    let safe = |text: &str| -> String {
        text.chars()
            .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.'))
            .take(120)
            .collect::<String>()
            .trim_matches(['.', ' '])
            .to_owned()
    };
    let title = safe(title);
    let artist = safe(artist);
    let mut paths = vec![root.join(format!("{artist} - {title}.lrc"))];
    paths.push(root.join(format!("{title}.lrc")));
    paths
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Candidate {
    track_name: String,
    artist_name: String,
    synced_lyrics: Option<String>,
    plain_lyrics: Option<String>,
    instrumental: bool,
    #[serde(default)]
    duration: Option<f64>,
}

fn choose_candidate(
    candidates: Vec<Candidate>,
    title: &str,
    artist: &str,
    duration: Option<f64>,
) -> Option<Candidate> {
    let title = title.trim();
    let artist = artist.trim();
    let tolerance = duration.map(|_| 2.0);
    candidates
        .into_iter()
        .filter(|item| {
            item.track_name.trim().eq_ignore_ascii_case(title)
                && item.artist_name.trim().eq_ignore_ascii_case(artist)
        })
        .filter(|item| match (duration, item.duration, tolerance) {
            (Some(expected), Some(candidate), Some(tolerance)) => {
                expected.is_finite()
                    && candidate.is_finite()
                    && (expected - candidate).abs() <= tolerance
            }
            _ => true,
        })
        .min_by(|a, b| {
            let diff = |item: &Candidate| match (duration, item.duration) {
                (Some(expected), Some(candidate)) => (expected - candidate).abs(),
                _ => f64::MAX,
            };
            diff(a).total_cmp(&diff(b))
        })
}

#[allow(dead_code)]
fn lookup(title: &str, artist: &str, duration: Option<f64>) -> LyricState {
    lookup_with_policy(title, artist, duration, true)
}

fn lookup_with_policy(
    title: &str,
    artist: &str,
    duration: Option<f64>,
    online_lrclib: bool,
) -> LyricState {
    for path in local_path(title, artist) {
        if let Ok(raw) = std::fs::read_to_string(path) {
            let lines = parse_lrc(&raw);
            if !lines.is_empty() {
                return LyricState::Synced(lines);
            }
            if !raw.trim().is_empty() {
                return LyricState::Plain(raw);
            }
        }
    }
    if !online_lrclib {
        return LyricState::Unavailable;
    }
    let url = format!(
        "https://lrclib.net/api/search?track_name={}&artist_name={}",
        urlencoding::encode(title),
        urlencoding::encode(artist)
    );
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(8)))
        .build()
        .new_agent();
    let Ok(mut response) = agent
        .get(&url)
        .header("User-Agent", "Glacier/0.2 (personal Linux media overlay)")
        .call()
    else {
        return LyricState::Unavailable;
    };
    let mut body = Vec::new();
    if response
        .body_mut()
        .as_reader()
        .take(512 * 1024)
        .read_to_end(&mut body)
        .is_err()
    {
        return LyricState::Unavailable;
    }
    let Ok(candidates) = serde_json::from_slice::<Vec<Candidate>>(&body) else {
        return LyricState::Unavailable;
    };
    let Some(item) = choose_candidate(candidates, title, artist, duration) else {
        return LyricState::Unavailable;
    };
    if let Some(synced) = item.synced_lyrics {
        let lines = parse_lrc(&synced);
        if !lines.is_empty() {
            return LyricState::Synced(lines);
        }
    }
    if item.instrumental {
        LyricState::Instrumental
    } else if let Some(plain) = item.plain_lyrics {
        LyricState::Plain(plain)
    } else {
        LyricState::Unavailable
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timestamps_offsets_and_multi_stamp() {
        let lines = parse_lrc("[offset:500]\n[00:02.50][00:03.000]Hello\n[00:05.00]World");
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].at, 2.0);
        assert_eq!(lines[1].at, 2.5);
        assert_eq!(lines[2].text, "World");
        assert_eq!(parse_stamp("garbage"), None);
    }
    #[test]
    fn selection_follows_playback() {
        let mut lyrics = Lyrics::new();
        lyrics.state = LyricState::Synced(parse_lrc("[00:01.00]One\n[00:04.00]Two"));
        assert_eq!(lyrics.window(0.0), None);
        assert_eq!(lyrics.window(2.0), Some((0, Some(1))));
        assert_eq!(lyrics.window(5.0), Some((1, None)));
        let lines = lyrics.synced().unwrap();
        assert_eq!(lines[0].text, "One");
        assert_eq!(lines[1].at, 4.0);
    }
    #[test]
    fn chooses_synced_lyrics_for_matching_recording_duration() {
        let candidates = vec![
            Candidate {
                track_name: "Song".into(),
                artist_name: "Artist".into(),
                duration: Some(240.0),
                synced_lyrics: Some("[00:01.00]Wrong version".into()),
                plain_lyrics: None,
                instrumental: false,
            },
            Candidate {
                track_name: "Song".into(),
                artist_name: "Artist".into(),
                duration: Some(181.0),
                synced_lyrics: Some("[00:01.00]Right version".into()),
                plain_lyrics: None,
                instrumental: false,
            },
        ];
        let selected = choose_candidate(candidates, "Song", "Artist", Some(180.0)).unwrap();
        assert_eq!(selected.duration, Some(181.0));
    }

    #[test]
    fn rejects_lyrics_for_a_different_recording_duration() {
        let candidates = vec![Candidate {
            track_name: "Song".into(),
            artist_name: "Artist".into(),
            duration: Some(240.0),
            synced_lyrics: Some("[00:01.00]Wrong version".into()),
            plain_lyrics: None,
            instrumental: false,
        }];
        assert!(choose_candidate(candidates, "Song", "Artist", Some(180.0)).is_none());
    }

    #[test]
    #[ignore = "needs LRCLIB network access"]
    fn lrclib_returns_known_synced_song() {
        let result = lookup("Never Gonna Give You Up", "Rick Astley", None);
        assert!(matches!(result, LyricState::Synced(_)), "{result:?}");
    }
}
