use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;

const MAX_RESPONSE_BYTES: u64 = 2 * 1024 * 1024;
const MIN_VALID_DURATION_SECS: f64 = 1.0;
const DURATION_DEBOUNCE_SECS: f64 = 0.5;
const TIGHT_DURATION_TOLERANCE_SECS: f64 = 3.0;
const RELAXED_DURATION_TOLERANCE_SECS: f64 = 8.0;
const MAX_DURATION_TOLERANCE_SECS: f64 = 15.0;

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
    generation: u64,
    title: String,
    artist: String,
    album: String,
    duration: Option<f64>,
    online_lrclib: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum LookupOutcome {
    Definitive(LyricState),
    TransientError,
}

#[derive(Default)]
struct LyricsCache {
    entries: HashMap<String, LyricState>,
}

impl LyricsCache {
    fn resolve_with<F>(&mut self, cache_key: &str, lookup_fn: F) -> LyricState
    where
        F: FnOnce() -> LookupOutcome,
    {
        if let Some(cached) = self.entries.get(cache_key) {
            return cached.clone();
        }
        match lookup_fn() {
            LookupOutcome::Definitive(state) => {
                self.entries.insert(cache_key.to_owned(), state.clone());
                state
            }
            LookupOutcome::TransientError => LyricState::Unavailable,
        }
    }
}

pub struct Lyrics {
    tx: Sender<LyricRequest>,
    rx: Receiver<(u64, LyricState)>,
    pub key: String,
    pub state: LyricState,
    raw_title: String,
    raw_artist: String,
    raw_album: String,
    duration: Option<f64>,
    generation: u64,
    enabled: bool,
    online_lrclib: bool,
}

impl Lyrics {
    pub fn new() -> Self {
        let (tx, requests) = channel::<LyricRequest>();
        let (answers, rx) = channel();
        thread::spawn(move || {
            let mut cache = LyricsCache::default();
            while let Ok(request) = requests.recv() {
                let key = track_key(&request.title, &request.artist, &request.album);
                let duration = normalize_duration(request.duration);
                let cache_key = format!(
                    "{key}\0{}\0{}",
                    duration
                        .map(|seconds| format!("{seconds:.0}"))
                        .unwrap_or_default(),
                    u8::from(request.online_lrclib),
                );
                let state = cache.resolve_with(&cache_key, || {
                    lookup_outcome(
                        &request.title,
                        &request.artist,
                        &request.album,
                        duration,
                        request.online_lrclib,
                    )
                });
                if answers.send((request.generation, state)).is_err() {
                    break;
                }
            }
        });
        Self::from_channels(tx, rx)
    }

    fn from_channels(tx: Sender<LyricRequest>, rx: Receiver<(u64, LyricState)>) -> Self {
        Self {
            tx,
            rx,
            key: String::new(),
            state: LyricState::Unavailable,
            raw_title: String::new(),
            raw_artist: String::new(),
            raw_album: String::new(),
            duration: None,
            generation: 0,
            enabled: true,
            online_lrclib: true,
        }
    }

    fn dispatch(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.state = LyricState::Loading;
        let _ = self.tx.send(LyricRequest {
            generation: self.generation,
            title: self.raw_title.clone(),
            artist: self.raw_artist.clone(),
            album: self.raw_album.clone(),
            duration: self.duration,
            online_lrclib: self.online_lrclib,
        });
    }

    pub fn set_config(&mut self, enabled: bool, online_lrclib: bool) {
        let changed = self.enabled != enabled || self.online_lrclib != online_lrclib;
        self.enabled = enabled;
        self.online_lrclib = online_lrclib;
        if !enabled {
            self.generation = self.generation.wrapping_add(1);
            self.state = LyricState::Unavailable;
            return;
        }
        if changed && !self.raw_title.trim().is_empty() {
            self.dispatch();
        }
    }

    pub fn update_track(&mut self, title: &str, artist: &str, album: &str, duration: Option<f64>) {
        let key = track_key(title, artist, album);
        if key == self.key {
            return;
        }
        self.key = key;
        self.raw_title = title.to_owned();
        self.raw_artist = artist.to_owned();
        self.raw_album = album.to_owned();
        self.duration = normalize_duration(duration);
        if title.trim().is_empty() || !self.enabled {
            self.generation = self.generation.wrapping_add(1);
            self.state = LyricState::Unavailable;
        } else {
            self.dispatch();
        }
    }

    pub fn poll(&mut self) {
        while let Ok((generation, state)) = self.rx.try_recv() {
            if self.enabled && generation == self.generation {
                self.state = state;
            }
        }
    }

    pub fn update_duration(&mut self, duration: Option<f64>) {
        let duration = normalize_duration(duration);
        if self.raw_title.trim().is_empty() {
            return;
        }
        let unchanged = match (self.duration, duration) {
            (None, None) => true,
            (Some(prev), Some(next)) => (prev - next).abs() < DURATION_DEBOUNCE_SECS,
            _ => false,
        };
        if unchanged {
            return;
        }
        self.duration = duration;
        if !self.enabled {
            return;
        }
        self.dispatch();
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

fn normalize_duration(duration: Option<f64>) -> Option<f64> {
    duration.filter(|seconds| seconds.is_finite() && *seconds > MIN_VALID_DURATION_SECS)
}

fn track_key(title: &str, artist: &str, album: &str) -> String {
    format!(
        "{}\0{}\0{}",
        title.trim().to_lowercase(),
        artist.trim().to_lowercase(),
        album.trim().to_lowercase(),
    )
}

fn parse_lrc(input: &str) -> Vec<Line> {
    let input = input.strip_prefix('\u{feff}').unwrap_or(input);
    let mut lines = Vec::new();
    let offset = parse_offset(input);
    for row in input.lines().take(2000) {
        let mut rest = row.trim();
        let mut stamps = Vec::new();
        while let Some(after) = rest.strip_prefix('[') {
            let Some((token, tail)) = after.split_once(']') else {
                break;
            };
            if let Some(at) = parse_stamp(token.trim()) {
                stamps.push((at - offset).max(0.0));
                rest = tail.trim_start();
            } else if stamps.is_empty() {
                rest = tail.trim_start();
            } else {
                break;
            }
        }
        let cleaned = strip_inline_timestamps(rest);
        let text = cleaned.trim();
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

fn parse_offset(input: &str) -> f64 {
    input
        .lines()
        .find_map(|line| {
            let inner = line.trim().strip_prefix('[')?.split_once(']')?.0;
            let (tag, value) = inner.split_once(':')?;
            if !tag.trim().eq_ignore_ascii_case("offset") {
                return None;
            }
            value.trim().parse::<i64>().ok()
        })
        .unwrap_or(0) as f64
        / 1000.0
}

fn strip_inline_timestamps(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let after_open = &rest[open + 1..];
        if let Some(close) = after_open.find('>') {
            let token = &after_open[..close];
            if parse_stamp(token.trim()).is_some() {
                rest = &after_open[close + 1..];
                continue;
            }
        }
        out.push('<');
        rest = after_open;
    }
    out.push_str(rest);
    collapse_spaces(&out)
}

fn parse_fraction(fraction: &str) -> Option<f64> {
    if fraction.is_empty() {
        return Some(0.0);
    }
    if !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let digits = &fraction[..fraction.len().min(9)];
    let value = digits.parse::<u64>().ok()? as f64;
    Some(value / 10_f64.powi(digits.len() as i32))
}

fn parse_stamp(s: &str) -> Option<f64> {
    let mut parts = s.split(':');
    let first = parts.next()?;
    let second = parts.next()?;
    let third = parts.next();
    if parts.next().is_some() {
        return None;
    }

    match third {
        None => {
            if first.is_empty() || !first.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let (secs, fraction) = second.split_once('.').unwrap_or((second, ""));
            if secs.is_empty() || secs.len() > 2 || !secs.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let sec = secs.parse::<u32>().ok()?;
            if sec > 99 {
                return None;
            }
            let frac = parse_fraction(fraction)?;
            Some(first.parse::<u32>().ok()? as f64 * 60.0 + sec as f64 + frac)
        }
        Some(last) => {
            if first.is_empty()
                || second.is_empty()
                || last.is_empty()
                || !first.bytes().all(|b| b.is_ascii_digit())
                || !second.bytes().all(|b| b.is_ascii_digit())
            {
                return None;
            }
            if let Some((secs, fraction)) = last.split_once('.') {
                // hh:mm:ss.frac
                if second.len() > 2
                    || secs.is_empty()
                    || secs.len() > 2
                    || !secs.bytes().all(|b| b.is_ascii_digit())
                {
                    return None;
                }
                let hours = first.parse::<u32>().ok()?;
                let mins = second.parse::<u32>().ok()?;
                let sec = secs.parse::<u32>().ok()?;
                if mins > 59 || sec > 59 {
                    return None;
                }
                let frac = parse_fraction(fraction)?;
                Some(hours as f64 * 3600.0 + mins as f64 * 60.0 + sec as f64 + frac)
            } else {
                // mm:ss:xx (colon-separated fractional seconds)
                if second.len() > 2 {
                    return None;
                }
                let mins = first.parse::<u32>().ok()?;
                let sec = second.parse::<u32>().ok()?;
                if sec > 99 {
                    return None;
                }
                let frac = parse_fraction(last)?;
                Some(mins as f64 * 60.0 + sec as f64 + frac)
            }
        }
    }
}

fn lyrics_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
        })
        .join("glacier/lyrics")
}

fn local_filenames(title: &str, artist: &str) -> Vec<String> {
    let safe = |text: &str| -> String {
        text.chars()
            .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.'))
            .take(120)
            .collect::<String>()
            .trim_matches(['.', ' '])
            .to_owned()
    };
    let mut names = Vec::new();
    let mut push_pair = |t: &str, a: &str| {
        let st = safe(t);
        let sa = safe(a);
        if !sa.is_empty() && !st.is_empty() {
            let full = format!("{sa} - {st}.lrc");
            if !names.contains(&full) {
                names.push(full);
            }
        }
        if !st.is_empty() {
            let solo = format!("{st}.lrc");
            if !names.contains(&solo) {
                names.push(solo);
            }
        }
    };
    push_pair(title, artist);
    let cleaned_title = clean_title(title);
    let cleaned_artist = primary_artist(artist);
    push_pair(&cleaned_title, &cleaned_artist);
    names
}

fn find_local_lrc_in_dir(root: &Path, title: &str, artist: &str) -> Option<PathBuf> {
    let names = local_filenames(title, artist);
    for name in &names {
        let exact = root.join(name);
        if exact.is_file() {
            return Some(exact);
        }
    }
    let lowered: Vec<String> = names.iter().map(|n| n.to_lowercase()).collect();
    let entries = std::fs::read_dir(root).ok()?;
    let mut dir_files: Vec<(String, PathBuf)> = entries
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if !path.is_file() {
                return None;
            }
            let file_name = path.file_name()?.to_str()?.to_lowercase();
            Some((file_name, path))
        })
        .collect();
    dir_files.sort_by(|a, b| a.0.cmp(&b.0));
    for target in &lowered {
        if let Some((_, path)) = dir_files.iter().find(|(name, _)| name == target) {
            return Some(path.clone());
        }
    }
    None
}

fn collapse_spaces(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_space = false;
    for ch in input.trim().chars() {
        if ch.is_whitespace() {
            if !in_space {
                out.push(' ');
                in_space = true;
            }
        } else {
            out.push(ch);
            in_space = false;
        }
    }
    out
}

fn normalize_typography(input: &str) -> String {
    let mapped: String = input
        .chars()
        .map(|ch| match ch {
            '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' | '\u{2032}' | '\u{02BC}' | '`' => {
                '\''
            }
            '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' => '"',
            '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2015}'
            | '\u{2212}' => '-',
            '\u{00A0}' => ' ',
            other => other,
        })
        .collect();
    collapse_spaces(&mapped)
}

fn fold_key(input: &str) -> String {
    let normalized = normalize_typography(input).to_lowercase();
    let mut out = String::with_capacity(normalized.len());
    let mut last_space = true;
    for ch in normalized.chars() {
        if ch == '\'' {
            continue;
        }
        if ch.is_alphanumeric() {
            out.push(ch);
            last_space = false;
        } else if !last_space {
            out.push(' ');
            last_space = true;
        }
    }
    out.trim().to_owned()
}

fn is_noise_tag(tag: &str) -> bool {
    let lower = collapse_spaces(tag).to_lowercase();
    if lower.is_empty() {
        return false;
    }
    const PREFIXES: &[&str] = &["feat.", "feat ", "ft.", "ft ", "featuring ", "with "];
    if PREFIXES.iter().any(|p| lower.starts_with(p)) {
        return true;
    }
    const KEYWORDS: &[&str] = &[
        "official",
        "lyric",
        "audio",
        "music video",
        "visualizer",
        "visualiser",
        "remaster",
        "radio edit",
        "single version",
        "album version",
        "original mix",
        "explicit",
        "clean version",
        "bonus track",
        "deluxe",
        "video",
    ];
    if KEYWORDS.iter().any(|k| lower.contains(k)) {
        return true;
    }
    matches!(lower.as_str(), "hd" | "hq" | "4k" | "mv" | "m/v" | "clean")
}

fn strip_bracketed_noise(input: &str, open: char, close: char) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find(open) {
        out.push_str(&rest[..start]);
        let after_open = &rest[start + open.len_utf8()..];
        if let Some(end) = after_open.find(close) {
            let inner = &after_open[..end];
            if is_noise_tag(inner) {
                rest = &after_open[end + close.len_utf8()..];
                continue;
            }
            out.push(open);
            out.push_str(inner);
            out.push(close);
            rest = &after_open[end + close.len_utf8()..];
        } else {
            out.push(open);
            rest = after_open;
        }
    }
    out.push_str(rest);
    collapse_spaces(&out)
}

fn is_dash_edition_suffix(suffix: &str) -> bool {
    let lower = collapse_spaces(suffix).to_lowercase();
    const SUFFIX_KEYWORDS: &[&str] = &[
        "remaster",
        "radio edit",
        "single version",
        "album version",
        "original mix",
        "explicit",
        "official",
        "lyric",
        "audio",
        "music video",
        "visualizer",
        "bonus track",
        "deluxe",
        "feat.",
        "feat ",
        "ft.",
        "ft ",
        "featuring ",
    ];
    SUFFIX_KEYWORDS.iter().any(|k| lower.contains(k))
        || matches!(lower.as_str(), "clean" | "hd" | "hq" | "4k" | "mv")
}

fn strip_inline_feat_suffix(input: &str) -> String {
    let lower = input.to_lowercase();
    for marker in [" feat. ", " feat ", " ft. ", " ft ", " featuring "] {
        if let Some(idx) = lower.find(marker) {
            // Find matching char boundary safely via ASCII/lowercase prefix length when identical,
            // or scan char indices.
            if let Some((byte_idx, _)) = input
                .char_indices()
                .find(|(i, _)| input[*i..].to_lowercase().starts_with(marker))
            {
                let prefix = input[..byte_idx].trim();
                if !prefix.is_empty() {
                    return prefix.to_owned();
                }
            }
            let _ = idx;
        }
    }
    input.to_owned()
}

fn clean_title(raw: &str) -> String {
    let base = normalize_typography(raw);
    if base.is_empty() {
        return base;
    }
    let mut cleaned = strip_bracketed_noise(&base, '(', ')');
    cleaned = strip_bracketed_noise(&cleaned, '[', ']');
    loop {
        if let Some((head, tail)) = cleaned.rsplit_once(" - ")
            && !head.trim().is_empty()
            && is_dash_edition_suffix(tail)
        {
            cleaned = head.trim().to_owned();
            continue;
        }
        break;
    }
    cleaned = strip_inline_feat_suffix(&cleaned);
    cleaned = collapse_spaces(&cleaned);
    if cleaned.is_empty() { base } else { cleaned }
}

fn clean_artist(raw: &str) -> String {
    let mut cleaned = normalize_typography(raw);
    if cleaned.len() >= 8 && cleaned[cleaned.len() - 8..].eq_ignore_ascii_case(" - topic") {
        cleaned.truncate(cleaned.len() - 8);
        cleaned = cleaned.trim().to_owned();
    }
    if cleaned.len() >= 5 && cleaned[cleaned.len() - 5..].eq_ignore_ascii_case(" vevo") {
        cleaned.truncate(cleaned.len() - 5);
        cleaned = cleaned.trim().to_owned();
    } else if cleaned.len() > 4 && cleaned[cleaned.len() - 4..].eq_ignore_ascii_case("vevo") {
        cleaned.truncate(cleaned.len() - 4);
        cleaned = cleaned.trim().to_owned();
    }
    cleaned
}

fn is_channel_like_artist(raw: &str) -> bool {
    let lower = normalize_typography(raw).to_lowercase();
    lower.is_empty()
        || lower.ends_with(" - topic")
        || lower.ends_with("vevo")
        || lower.ends_with(" official")
        || lower.ends_with(" music")
        || lower.ends_with(" records")
        || lower.ends_with(" channel")
        || lower.ends_with(" tv")
        || lower.ends_with(" lyrics")
}

fn split_artists(raw: &str) -> Vec<String> {
    let cleaned = clean_artist(raw);
    if cleaned.is_empty() {
        return Vec::new();
    }
    let mut parts = vec![cleaned];
    for sep in [
        ", ",
        " & ",
        " / ",
        "; ",
        ";",
        " x ",
        " X ",
        " × ",
        " feat. ",
        " feat ",
        " ft. ",
        " ft ",
        " featuring ",
        " with ",
    ] {
        parts = parts
            .into_iter()
            .flat_map(|part| split_case_insensitive(&part, sep))
            .collect();
    }
    // Also split tight slash lists like "Artist A/Artist B", avoiding "AC/DC".
    parts = parts
        .into_iter()
        .flat_map(|part| {
            if part.contains('/') && !part.eq_ignore_ascii_case("ac/dc") {
                part.split('/')
                    .map(|s| s.trim().to_owned())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
            } else {
                vec![part]
            }
        })
        .collect();
    parts
        .into_iter()
        .map(|s| collapse_spaces(&s))
        .filter(|s| !s.is_empty())
        .collect()
}

fn split_case_insensitive(input: &str, sep: &str) -> Vec<String> {
    let sep_lower = sep.to_lowercase();
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < input.len() {
        if input.is_char_boundary(i) && input[i..].to_lowercase().starts_with(&sep_lower) {
            let chunk = input[start..i].trim();
            if !chunk.is_empty() {
                out.push(chunk.to_owned());
            }
            // Advance by the matching char count of sep.
            let adv: usize = input[i..]
                .chars()
                .take(sep.chars().count())
                .map(char::len_utf8)
                .sum();
            i += adv.max(1);
            start = i;
        } else {
            let ch_len = input[i..].chars().next().map_or(1, char::len_utf8);
            i += ch_len;
        }
    }
    let tail = input[start..].trim();
    if !tail.is_empty() {
        out.push(tail.to_owned());
    }
    out
}

fn primary_artist(raw: &str) -> String {
    split_artists(raw)
        .into_iter()
        .next()
        .unwrap_or_else(|| clean_artist(raw))
}

fn split_title_artist_pair(title: &str) -> Option<(String, String)> {
    let cleaned = clean_title(title);
    let (left, right) = cleaned.split_once(" - ")?;
    let artist = clean_artist(left);
    let song = clean_title(right);
    if artist.is_empty() || song.is_empty() {
        return None;
    }
    Some((artist, song))
}

fn prepare_cleaned_query(raw_title: &str, raw_artist: &str) -> (String, String) {
    let artist_cleaned = clean_artist(raw_artist);
    let mut title_cleaned = clean_title(raw_title);
    if !artist_cleaned.is_empty()
        && let Some((left, right)) = title_cleaned.split_once(" - ")
    {
        let left_fold = fold_key(left);
        let artist_fold = fold_key(&artist_cleaned);
        let primary_fold = fold_key(&primary_artist(&artist_cleaned));
        if !left_fold.is_empty() && (left_fold == artist_fold || left_fold == primary_fold) {
            let stripped = clean_title(right);
            if !stripped.is_empty() {
                title_cleaned = stripped;
            }
        }
    }
    if is_channel_like_artist(raw_artist)
        && let Some((split_artist, split_title)) = split_title_artist_pair(&title_cleaned)
    {
        return (split_title, primary_artist(&split_artist));
    }
    (title_cleaned, primary_artist(&artist_cleaned))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Candidate {
    #[serde(default)]
    track_name: String,
    #[serde(default)]
    artist_name: String,
    #[serde(default)]
    album_name: Option<String>,
    #[serde(default)]
    synced_lyrics: Option<String>,
    #[serde(default)]
    plain_lyrics: Option<String>,
    #[serde(default)]
    instrumental: bool,
    #[serde(default)]
    duration: Option<f64>,
}

fn title_match_score(expected: &str, candidate: &str) -> Option<u32> {
    let exp_norm = normalize_typography(expected).to_lowercase();
    let cand_norm = normalize_typography(candidate).to_lowercase();
    if exp_norm.is_empty() || cand_norm.is_empty() {
        return None;
    }
    if exp_norm == cand_norm {
        return Some(100);
    }
    let exp_fold = fold_key(expected);
    let cand_fold = fold_key(candidate);
    if !exp_fold.is_empty() && exp_fold == cand_fold {
        return Some(95);
    }
    let exp_clean = fold_key(&clean_title(expected));
    let cand_clean = fold_key(&clean_title(candidate));
    if !exp_clean.is_empty() && exp_clean == cand_clean {
        return Some(85);
    }
    None
}

fn artist_tokens(input: &str) -> Vec<String> {
    fold_key(input)
        .split_whitespace()
        .filter(|tok| !matches!(*tok, "the" | "and" | "a"))
        .map(str::to_owned)
        .collect()
}

fn artist_match_score(expected: &str, candidate: &str) -> Option<u32> {
    let exp_cleaned = clean_artist(expected);
    let cand_cleaned = clean_artist(candidate);
    if exp_cleaned.is_empty() {
        return (!cand_cleaned.is_empty()).then_some(20);
    }
    if cand_cleaned.is_empty() {
        return None;
    }
    let exp_norm = normalize_typography(expected).to_lowercase();
    let cand_norm = normalize_typography(candidate).to_lowercase();
    if exp_norm == cand_norm {
        return Some(100);
    }
    let exp_fold = fold_key(expected);
    let cand_fold = fold_key(candidate);
    if !exp_fold.is_empty() && exp_fold == cand_fold {
        return Some(95);
    }
    let exp_clean_fold = fold_key(&exp_cleaned);
    let cand_clean_fold = fold_key(&cand_cleaned);
    if !exp_clean_fold.is_empty() && exp_clean_fold == cand_clean_fold {
        return Some(90);
    }
    let exp_compact = exp_clean_fold.replace(' ', "");
    let cand_compact = cand_clean_fold.replace(' ', "");
    if !exp_compact.is_empty() && exp_compact == cand_compact {
        return Some(85);
    }

    let exp_list: Vec<String> = split_artists(expected)
        .into_iter()
        .map(|s| fold_key(&s))
        .filter(|s| !s.is_empty())
        .collect();
    let cand_list: Vec<String> = split_artists(candidate)
        .into_iter()
        .map(|s| fold_key(&s))
        .filter(|s| !s.is_empty())
        .collect();

    if let (Some(exp_primary), Some(cand_primary)) = (exp_list.first(), cand_list.first()) {
        if exp_primary == cand_primary {
            let overlap = exp_list.iter().filter(|a| cand_list.contains(a)).count();
            if overlap == exp_list.len() && overlap == cand_list.len() {
                return Some(85);
            }
            return Some(75);
        }
        if exp_primary.replace(' ', "") == cand_primary.replace(' ', "") {
            return Some(72);
        }
    }
    if exp_list.iter().any(|a| cand_list.contains(a)) {
        return Some(60);
    }

    let exp_tokens = artist_tokens(&exp_cleaned);
    let cand_tokens = artist_tokens(&cand_cleaned);
    if !exp_tokens.is_empty() && !cand_tokens.is_empty() {
        let (smaller, larger) = if exp_tokens.len() <= cand_tokens.len() {
            (&exp_tokens, &cand_tokens)
        } else {
            (&cand_tokens, &exp_tokens)
        };
        if smaller.iter().all(|tok| larger.contains(tok)) {
            return Some(65);
        }
    }
    None
}

fn album_bonus(expected_album: &str, candidate_album: Option<&str>) -> u32 {
    let exp = fold_key(expected_album);
    let Some(cand) = candidate_album.map(fold_key) else {
        return 0;
    };
    if !exp.is_empty() && exp == cand {
        10
    } else {
        0
    }
}

fn duration_match(expected: Option<f64>, candidate: Option<f64>) -> Option<(u8, f64)> {
    let expected = normalize_duration(expected);
    let candidate = normalize_duration(candidate);
    match (expected, candidate) {
        (Some(exp), Some(cand)) => {
            let diff = (exp - cand).abs();
            if diff > MAX_DURATION_TOLERANCE_SECS {
                return None;
            }
            let tier = if diff <= TIGHT_DURATION_TOLERANCE_SECS {
                3
            } else if diff <= RELAXED_DURATION_TOLERANCE_SECS {
                2
            } else {
                1
            };
            Some((tier, diff))
        }
        (None, _) => Some((2, f64::INFINITY)),
        (Some(_), None) => Some((1, f64::INFINITY)),
    }
}

fn candidate_state(item: &Candidate) -> Option<(u8, LyricState)> {
    if let Some(synced) = item.synced_lyrics.as_deref() {
        let lines = parse_lrc(synced);
        if !lines.is_empty() {
            return Some((3, LyricState::Synced(lines)));
        }
    }
    if item.instrumental {
        return Some((2, LyricState::Instrumental));
    }
    if let Some(plain) = item.plain_lyrics.as_deref()
        && !plain.trim().is_empty()
    {
        return Some((1, LyricState::Plain(plain.to_owned())));
    }
    None
}

fn choose_candidate_with_album(
    candidates: Vec<Candidate>,
    title: &str,
    artist: &str,
    album: &str,
    duration: Option<f64>,
) -> Option<Candidate> {
    let mut best: Option<(u8, u8, u32, f64, Candidate)> = None;
    for item in candidates {
        let Some((content_rank, _)) = candidate_state(&item) else {
            continue;
        };
        let Some(t_score) = title_match_score(title, &item.track_name) else {
            continue;
        };
        let Some(a_score) = artist_match_score(artist, &item.artist_name) else {
            continue;
        };
        let Some((dur_tier, dur_diff)) = duration_match(duration, item.duration) else {
            continue;
        };
        let meta_score = t_score + a_score + album_bonus(album, item.album_name.as_deref());
        let is_better = match &best {
            None => true,
            Some((best_content, best_tier, best_meta, best_diff, _)) => {
                (content_rank, dur_tier, meta_score) > (*best_content, *best_tier, *best_meta)
                    || ((content_rank, dur_tier, meta_score)
                        == (*best_content, *best_tier, *best_meta)
                        && dur_diff < *best_diff)
            }
        };
        if is_better {
            best = Some((content_rank, dur_tier, meta_score, dur_diff, item));
        }
    }
    best.map(|(_, _, _, _, item)| item)
}

#[allow(dead_code)]
fn choose_candidate(
    candidates: Vec<Candidate>,
    title: &str,
    artist: &str,
    duration: Option<f64>,
) -> Option<Candidate> {
    choose_candidate_with_album(candidates, title, artist, "", duration)
}

enum HttpOutcome<T> {
    Ok(T),
    NotFound,
    TransientError,
}

fn fetch_json<T: DeserializeOwned>(agent: &ureq::Agent, url: &str) -> HttpOutcome<T> {
    let mut response = match agent
        .get(url)
        .header("User-Agent", "Glacier/0.2 (personal Linux media overlay)")
        .call()
    {
        Ok(resp) => resp,
        Err(ureq::Error::StatusCode(404)) => return HttpOutcome::NotFound,
        Err(ureq::Error::StatusCode(code))
            if (400..500).contains(&code) && code != 408 && code != 429 =>
        {
            return HttpOutcome::NotFound;
        }
        Err(_) => return HttpOutcome::TransientError,
    };
    let mut body = Vec::new();
    if response
        .body_mut()
        .as_reader()
        .take(MAX_RESPONSE_BYTES)
        .read_to_end(&mut body)
        .is_err()
    {
        return HttpOutcome::TransientError;
    }
    match serde_json::from_slice::<T>(&body) {
        Ok(parsed) => HttpOutcome::Ok(parsed),
        Err(_) => HttpOutcome::TransientError,
    }
}

fn record_fallback(slot: &mut Option<LyricState>, state: LyricState) {
    if slot.is_none()
        || matches!(
            (&slot, &state),
            (Some(LyricState::Plain(_)), LyricState::Instrumental)
        )
    {
        *slot = Some(state);
    }
}

fn evaluate_search_results(
    candidates: Vec<Candidate>,
    title: &str,
    artist: &str,
    album: &str,
    duration: Option<f64>,
    fallback: &mut Option<LyricState>,
) -> Option<LyricState> {
    let chosen = choose_candidate_with_album(candidates, title, artist, album, duration)?;
    let (_, state) = candidate_state(&chosen)?;
    if matches!(state, LyricState::Synced(_)) {
        return Some(state);
    }
    record_fallback(fallback, state);
    None
}

#[allow(dead_code)]
fn lookup(title: &str, artist: &str, duration: Option<f64>) -> LyricState {
    match lookup_outcome(title, artist, "", normalize_duration(duration), true) {
        LookupOutcome::Definitive(state) => state,
        LookupOutcome::TransientError => LyricState::Unavailable,
    }
}

fn lookup_outcome(
    title: &str,
    artist: &str,
    album: &str,
    duration: Option<f64>,
    online_lrclib: bool,
) -> LookupOutcome {
    if let Some(path) = find_local_lrc_in_dir(&lyrics_dir(), title, artist)
        && let Ok(raw) = std::fs::read_to_string(path)
    {
        let lines = parse_lrc(&raw);
        if !lines.is_empty() {
            return LookupOutcome::Definitive(LyricState::Synced(lines));
        }
        if !raw.trim().is_empty() {
            return LookupOutcome::Definitive(LyricState::Plain(raw));
        }
    }
    if !online_lrclib {
        return LookupOutcome::Definitive(LyricState::Unavailable);
    }

    let duration = normalize_duration(duration);
    let raw_title = title.trim();
    let raw_artist = artist.trim();
    let raw_album = album.trim();
    if raw_title.is_empty() {
        return LookupOutcome::Definitive(LyricState::Unavailable);
    }

    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(8)))
        .build()
        .new_agent();

    let mut best_fallback: Option<LyricState> = None;
    let mut saw_transient_error = false;

    // Stage 1: Exact /api/get fast-path bypassing /api/search's 20-item cap.
    if !raw_artist.is_empty()
        && let Some(dur) = duration
    {
        let url = format!(
            "https://lrclib.net/api/get?track_name={}&artist_name={}&album_name={}&duration={}",
            urlencoding::encode(raw_title),
            urlencoding::encode(raw_artist),
            urlencoding::encode(raw_album),
            dur.round() as u64,
        );
        match fetch_json::<Candidate>(&agent, &url) {
            HttpOutcome::Ok(item) => {
                if let Some((_, state)) = candidate_state(&item) {
                    if matches!(state, LyricState::Synced(_)) {
                        return LookupOutcome::Definitive(state);
                    }
                    record_fallback(&mut best_fallback, state);
                }
            }
            HttpOutcome::NotFound => {}
            HttpOutcome::TransientError => saw_transient_error = true,
        }
    }

    // Stage 2: /api/search with raw track_name and artist_name.
    let mut queried_raw = false;
    if !raw_artist.is_empty() {
        queried_raw = true;
        let url = format!(
            "https://lrclib.net/api/search?track_name={}&artist_name={}",
            urlencoding::encode(raw_title),
            urlencoding::encode(raw_artist),
        );
        match fetch_json::<Vec<Candidate>>(&agent, &url) {
            HttpOutcome::Ok(candidates) => {
                if let Some(synced) = evaluate_search_results(
                    candidates,
                    raw_title,
                    raw_artist,
                    raw_album,
                    duration,
                    &mut best_fallback,
                ) {
                    return LookupOutcome::Definitive(synced);
                }
            }
            HttpOutcome::NotFound => {}
            HttpOutcome::TransientError => saw_transient_error = true,
        }
    }

    // Stage 3: /api/search with cleaned title and primary artist (or free-text ?q=).
    let (cleaned_title, cleaned_artist) = prepare_cleaned_query(raw_title, raw_artist);
    let same_as_raw = queried_raw
        && cleaned_title.eq_ignore_ascii_case(raw_title)
        && cleaned_artist.eq_ignore_ascii_case(raw_artist);

    if !cleaned_title.is_empty() && !same_as_raw {
        let url = if !cleaned_artist.is_empty() {
            format!(
                "https://lrclib.net/api/search?track_name={}&artist_name={}",
                urlencoding::encode(&cleaned_title),
                urlencoding::encode(&cleaned_artist),
            )
        } else {
            format!(
                "https://lrclib.net/api/search?q={}",
                urlencoding::encode(&cleaned_title),
            )
        };
        match fetch_json::<Vec<Candidate>>(&agent, &url) {
            HttpOutcome::Ok(candidates) => {
                if let Some(synced) = evaluate_search_results(
                    candidates,
                    &cleaned_title,
                    &cleaned_artist,
                    raw_album,
                    duration,
                    &mut best_fallback,
                ) {
                    return LookupOutcome::Definitive(synced);
                }
            }
            HttpOutcome::NotFound => {}
            HttpOutcome::TransientError => saw_transient_error = true,
        }
    }

    // Extra Stage 3 fallback: if title still looks like "Artist - Title" under an
    // unrecognized channel name, try the split pair too.
    if let Some((split_artist, split_title)) = split_title_artist_pair(&cleaned_title) {
        let split_primary = primary_artist(&split_artist);
        let already_tried = split_title.eq_ignore_ascii_case(&cleaned_title)
            && split_primary.eq_ignore_ascii_case(&cleaned_artist);
        if !already_tried && !split_title.is_empty() && !split_primary.is_empty() {
            let url = format!(
                "https://lrclib.net/api/search?track_name={}&artist_name={}",
                urlencoding::encode(&split_title),
                urlencoding::encode(&split_primary),
            );
            match fetch_json::<Vec<Candidate>>(&agent, &url) {
                HttpOutcome::Ok(candidates) => {
                    if let Some(synced) = evaluate_search_results(
                        candidates,
                        &split_title,
                        &split_primary,
                        raw_album,
                        duration,
                        &mut best_fallback,
                    ) {
                        return LookupOutcome::Definitive(synced);
                    }
                }
                HttpOutcome::NotFound => {}
                HttpOutcome::TransientError => saw_transient_error = true,
            }
        }
    }

    if let Some(fallback) = best_fallback {
        return LookupOutcome::Definitive(fallback);
    }
    if saw_transient_error {
        return LookupOutcome::TransientError;
    }
    LookupOutcome::Definitive(LyricState::Unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_candidate(
        track_name: &str,
        artist_name: &str,
        duration: Option<f64>,
        synced_lyrics: Option<&str>,
        plain_lyrics: Option<&str>,
    ) -> Candidate {
        Candidate {
            track_name: track_name.into(),
            artist_name: artist_name.into(),
            album_name: None,
            duration,
            synced_lyrics: synced_lyrics.map(str::to_owned),
            plain_lyrics: plain_lyrics.map(str::to_owned),
            instrumental: false,
        }
    }

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
    fn lrc_parser_handles_bom_colons_spacing_uppercase_offset_and_enhanced_tags() {
        let raw = "\u{feff}[OFFSET: 500]\r\n\
                   [00:10:50] [01:20.2500] <00:10.50>Hello <00:11.25>world\r\n\
                   [00:15.00] Keep <3 literal";
        let lines = parse_lrc(raw);
        assert_eq!(lines.len(), 3);
        assert!((lines[0].at - 10.0).abs() < 1e-6, "{lines:?}");
        assert_eq!(lines[0].text, "Hello world");
        assert!((lines[1].at - 14.5).abs() < 1e-6, "{lines:?}");
        assert_eq!(lines[1].text, "Keep <3 literal");
        assert!((lines[2].at - 79.75).abs() < 1e-6, "{lines:?}");
        assert_eq!(lines[2].text, "Hello world");
        assert!((parse_stamp("01:02:03.50").unwrap() - 3723.5).abs() < 1e-6);
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
            mk_candidate(
                "Song",
                "Artist",
                Some(240.0),
                Some("[00:01.00]Wrong version"),
                None,
            ),
            mk_candidate(
                "Song",
                "Artist",
                Some(181.0),
                Some("[00:01.00]Right version"),
                None,
            ),
        ];
        let selected = choose_candidate(candidates, "Song", "Artist", Some(180.0)).unwrap();
        assert_eq!(selected.duration, Some(181.0));
    }

    #[test]
    fn rejects_lyrics_for_a_different_recording_duration() {
        let candidates = vec![mk_candidate(
            "Song",
            "Artist",
            Some(240.0),
            Some("[00:01.00]Wrong version"),
            None,
        )];
        assert!(choose_candidate(candidates, "Song", "Artist", Some(180.0)).is_none());
    }

    #[test]
    fn prefers_synced_candidate_over_closer_plain_or_empty_candidate() {
        let candidates = vec![
            mk_candidate("Song", "Artist", Some(180.0), None, None),
            mk_candidate("Song", "Artist", Some(180.1), None, Some("Plain lyrics")),
            mk_candidate(
                "Song",
                "Artist",
                Some(180.2),
                Some("not valid lrc timestamps"),
                Some("Fallback plain"),
            ),
            mk_candidate(
                "Song",
                "Artist",
                Some(184.5),
                Some("[00:01.00]Real synced line"),
                Some("Real synced line"),
            ),
        ];
        let selected = choose_candidate(candidates, "Song", "Artist", Some(180.0)).unwrap();
        assert_eq!(
            selected.synced_lyrics.as_deref(),
            Some("[00:01.00]Real synced line")
        );
    }

    #[test]
    fn matches_multi_artist_remasters_youtube_noise_apostrophes_and_unicode_case() {
        // Multi-artist: "Artist A, Artist B" vs "Artist A" and "Artist A & Artist B"
        let multi = vec![mk_candidate(
            "Starboy",
            "The Weeknd",
            Some(230.0),
            Some("[00:01.00]Synced"),
            None,
        )];
        assert!(
            choose_candidate(
                multi,
                "Starboy (feat. Daft Punk)",
                "The Weeknd, Daft Punk",
                Some(230.0)
            )
            .is_some()
        );

        let ampersand = vec![mk_candidate(
            "Track",
            "Artist A & Artist B",
            Some(200.0),
            Some("[00:01.00]Synced"),
            None,
        )];
        assert!(choose_candidate(ampersand, "Track", "Artist A, Artist B", Some(200.0)).is_some());

        // Remastered + curly apostrophe
        let remaster = vec![mk_candidate(
            "Don't Stop Me Now (Remastered 2011)",
            "Queen",
            Some(209.0),
            Some("[00:01.00]Tonight"),
            None,
        )];
        assert!(
            choose_candidate(
                remaster,
                "Don\u{2019}t Stop Me Now - Remastered 2011",
                "Queen - Topic",
                Some(209.0)
            )
            .is_some()
        );

        // Unicode case folding (BEYONCÉ vs Beyoncé)
        let unicode = vec![mk_candidate(
            "Halo",
            "Beyonc\u{00e9}",
            Some(261.0),
            Some("[00:01.00]Remember"),
            None,
        )];
        assert!(choose_candidate(unicode, "HALO", "BEYONC\u{00c9}", Some(261.0)).is_some());

        // YouTube title/channel cleanup & Artist - Title splitting
        let (clean_t, clean_a) =
            prepare_cleaned_query("Linkin Park - Numb (Official Music Video)", "");
        assert_eq!(clean_t, "Numb");
        assert_eq!(clean_a, "Linkin Park");

        let (topic_t, topic_a) = prepare_cleaned_query("Numb [Lyrics]", "Linkin Park - Topic");
        assert_eq!(topic_t, "Numb");
        assert_eq!(topic_a, "Linkin Park");

        let (vevo_t, vevo_a) =
            prepare_cleaned_query("Never Gonna Give You Up (Official Video)", "RickAstleyVEVO");
        assert_eq!(vevo_t, "Never Gonna Give You Up");
        assert_eq!(vevo_a, "RickAstley");
    }

    #[test]
    fn duration_tiers_and_zero_duration_handling() {
        // Tight tier (<= 3s) preferred over relaxed tier (<= 8s)
        let tiered = vec![
            mk_candidate(
                "Song",
                "Artist",
                Some(186.0),
                Some("[00:01.00]Relaxed"),
                None,
            ),
            mk_candidate("Song", "Artist", Some(182.5), Some("[00:01.00]Tight"), None),
        ];
        let picked = choose_candidate(tiered, "Song", "Artist", Some(180.0)).unwrap();
        assert_eq!(picked.duration, Some(182.5));

        // Relaxed tier (<= 8s) still accepted when no tighter match exists
        let relaxed_only = vec![mk_candidate(
            "Song",
            "Artist",
            Some(187.5),
            Some("[00:01.00]Relaxed"),
            None,
        )];
        assert!(choose_candidate(relaxed_only, "Song", "Artist", Some(180.0)).is_some());

        // 0.0 / <= 1.0 duration is treated as None (unknown) and does not reject candidates
        let zero_dur = vec![mk_candidate(
            "Song",
            "Artist",
            Some(210.0),
            Some("[00:01.00]Valid"),
            None,
        )];
        assert!(choose_candidate(zero_dur, "Song", "Artist", Some(0.0)).is_some());
        assert_eq!(normalize_duration(Some(0.0)), None);
        assert_eq!(normalize_duration(Some(0.8)), None);
        assert_eq!(normalize_duration(Some(180.0)), Some(180.0));
    }

    #[test]
    fn transient_error_is_not_cached_permanently_and_stale_generations_are_ignored() {
        let mut cache = LyricsCache::default();
        let mut calls = 0;

        let first = cache.resolve_with("song\0artist", || {
            calls += 1;
            LookupOutcome::TransientError
        });
        assert_eq!(first, LyricState::Unavailable);
        assert_eq!(calls, 1);

        // Retry after transient error invokes lookup again and caches definitive synced result.
        let second = cache.resolve_with("song\0artist", || {
            calls += 1;
            LookupOutcome::Definitive(LyricState::Synced(parse_lrc("[00:01.00]Recovered")))
        });
        assert!(matches!(second, LyricState::Synced(_)));
        assert_eq!(calls, 2);

        // Cached definitive hit does not invoke lookup again.
        let third = cache.resolve_with("song\0artist", || {
            calls += 1;
            LookupOutcome::Definitive(LyricState::Unavailable)
        });
        assert_eq!(third, second);
        assert_eq!(calls, 2);

        // Stale generation IDs are ignored and < 0.5s duration jitter is debounced.
        let (req_tx, req_rx) = channel();
        let (ans_tx, ans_rx) = channel();
        let mut lyrics = Lyrics::from_channels(req_tx, ans_rx);

        lyrics.update_track("Song", "Artist", "Album", Some(180.0));
        let req1 = req_rx.try_recv().unwrap();
        assert_eq!(req1.generation, 1);
        assert_eq!(req1.album, "Album");

        // Sub-0.5s jitter should not dispatch a new request.
        lyrics.update_duration(Some(180.2));
        assert!(req_rx.try_recv().is_err());

        // Real duration update bumps generation.
        lyrics.update_duration(Some(185.0));
        let req2 = req_rx.try_recv().unwrap();
        assert_eq!(req2.generation, 2);

        // Stale reply for generation 1 is ignored.
        ans_tx
            .send((req1.generation, LyricState::Unavailable))
            .unwrap();
        lyrics.poll();
        assert_eq!(lyrics.state, LyricState::Loading);

        // Current reply for generation 2 is applied.
        let synced = LyricState::Synced(parse_lrc("[00:01.00]Fresh"));
        ans_tx.send((req2.generation, synced.clone())).unwrap();
        lyrics.poll();
        assert_eq!(lyrics.state, synced);

        // Even a late reply for generation 1 cannot overwrite generation 2.
        ans_tx
            .send((req1.generation, LyricState::Plain("Stale".into())))
            .unwrap();
        lyrics.poll();
        assert_eq!(lyrics.state, synced);
    }

    #[test]
    fn local_lookup_finds_case_insensitive_lrc_files() {
        let dir = std::env::temp_dir().join(format!("glacier-lrc-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("rick astley - never gonna give you up.lrc");
        std::fs::write(&file, "[00:01.00]We're no strangers to love").unwrap();

        let found = find_local_lrc_in_dir(&dir, "Never Gonna Give You Up", "Rick Astley");
        assert_eq!(found.as_deref(), Some(file.as_path()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore = "needs LRCLIB network access"]
    fn lrclib_returns_known_synced_song() {
        let result = lookup("Never Gonna Give You Up", "Rick Astley", None);
        assert!(matches!(result, LyricState::Synced(_)), "{result:?}");
    }
}
