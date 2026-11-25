//! X11 clipboard owner and history, without depending on `xclip`/`xsel`.
//!
//! Glacier owns the CLIPBOARD selection while it is running, so `Copy` actions
//! from the Shelf and Clips tabs are pasteable in other applications. While it
//! does *not* own the selection it polls the owner and pulls the text across, so
//! copies made anywhere else show up in the history.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use x11rb::COPY_DEPTH_FROM_PARENT;
use x11rb::CURRENT_TIME;
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, PropMode, SelectionNotifyEvent,
    SelectionRequestEvent, WindowClass,
};

pub const MAX_CLIPS: usize = 40;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Clip {
    pub text: String,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Clone, Debug)]
pub enum Action {
    Copy(String),
    TogglePin(String),
    Remove(String),
    Clear,
    Configure { enabled: bool, max_clips: usize },
}

enum Request {
    Copy(String),
    TogglePin(String),
    Remove(String),
    Clear,
    Configure { enabled: bool, max_clips: usize },
}

enum Update {
    History(Vec<Clip>),
    Status(String),
}

pub struct Clips {
    tx: Sender<Request>,
    rx: Receiver<Update>,
    pub items: Vec<Clip>,
    pub status: String,
    pub available: bool,
}

impl Clips {
    #[allow(dead_code)]
    pub fn start() -> Self {
        Self::start_with_config(true, MAX_CLIPS)
    }

    pub fn start_with_config(enabled: bool, max_clips: usize) -> Self {
        let (request_tx, requests) = channel::<Request>();
        let (updates, update_rx) = channel::<Update>();
        thread::spawn(move || run(requests, updates, enabled, max_clips.clamp(5, 200)));
        Self {
            tx: request_tx,
            rx: update_rx,
            items: Vec::new(),
            status: "reading clipboard".to_owned(),
            available: true,
        }
    }

    pub fn poll(&mut self) {
        while let Ok(update) = self.rx.try_recv() {
            match update {
                Update::History(items) => self.items = items,
                Update::Status(message) => {
                    self.status = message;
                    self.available = false;
                }
            }
        }
    }

    pub fn act(&self, action: Action) {
        let request = match action {
            Action::Copy(text) => Request::Copy(text),
            Action::TogglePin(text) => Request::TogglePin(text),
            Action::Remove(text) => Request::Remove(text),
            Action::Clear => Request::Clear,
            Action::Configure { enabled, max_clips } => Request::Configure { enabled, max_clips },
        };
        let _ = self.tx.send(request);
    }

    /// Newest first, pinned entries kept on top.
    pub fn ordered(&self) -> Vec<&Clip> {
        let mut items: Vec<&Clip> = self.items.iter().collect();
        items.sort_by_key(|clip| !clip.pinned);
        items
    }
}

fn store_path() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("glacier/clips.json")
}

fn load(max_clips: usize) -> Vec<Clip> {
    let Ok(raw) = std::fs::read(store_path()) else {
        return Vec::new();
    };
    let mut items: Vec<Clip> = serde_json::from_slice(&raw).unwrap_or_default();
    items.retain(|clip| !clip.text.trim().is_empty());
    items.truncate(max_clips);
    items
}

fn save(items: &[Clip]) {
    let _ = glacier::storage::write_at(&store_path(), items);
}

#[allow(dead_code)]
fn remember(items: &mut Vec<Clip>, text: &str) -> bool {
    remember_with_limit(items, text, MAX_CLIPS)
}

fn remember_with_limit(items: &mut Vec<Clip>, text: &str, max_clips: usize) -> bool {
    let text = text.trim_end_matches('\u{0}');
    if text.trim().is_empty() || text.chars().count() > 20_000 {
        return false;
    }
    if items.first().is_some_and(|clip| clip.text == text) {
        return false;
    }
    let pinned = items.iter().any(|clip| clip.text == text && clip.pinned);
    items.retain(|clip| clip.text != text);
    items.insert(
        0,
        Clip {
            text: text.to_owned(),
            pinned,
        },
    );
    items.truncate(max_clips);
    true
}

fn atom(conn: &impl Connection, name: &[u8]) -> Result<Atom, Box<dyn std::error::Error>> {
    Ok(conn.intern_atom(false, name)?.reply()?.atom)
}

fn run(requests: Receiver<Request>, updates: Sender<Update>, enabled: bool, max_clips: usize) {
    match session(requests, updates.clone(), enabled, max_clips) {
        Ok(()) => {}
        Err(error) => {
            let _ = updates.send(Update::Status(format!("clipboard unavailable: {error}")));
        }
    }
}

/// Atoms shared by the owner and the reader paths.
struct Atoms {
    clipboard: Atom,
    utf8: Atom,
    targets: Atom,
    text: Atom,
    string: Atom,
    property: Atom,
}

fn session(
    requests: Receiver<Request>,
    updates: Sender<Update>,
    mut enabled: bool,
    mut max_clips: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let (conn, screen_num) = x11rb::connect(None)?;
    let screen = &conn.setup().roots[screen_num];
    let window = conn.generate_id()?;
    conn.create_window(
        COPY_DEPTH_FROM_PARENT,
        window,
        screen.root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
    )?;

    let atoms = Atoms {
        clipboard: atom(&conn, b"CLIPBOARD")?,
        utf8: atom(&conn, b"UTF8_STRING")?,
        targets: atom(&conn, b"TARGETS")?,
        text: atom(&conn, b"TEXT")?,
        string: AtomEnum::STRING.into(),
        property: atom(&conn, b"GLACIER_CLIP")?,
    };

    let mut history = load(max_clips);
    let mut owned: Option<String> = None;
    // A Clear action must not immediately re-import the same selection that
    // was present before it. Stop suppressing as soon as its text changes.
    let mut cleared_selection: Option<String> = None;
    let mut ticks = 0u32;
    let _ = updates.send(Update::History(history.clone()));

    loop {
        while let Ok(request) = requests.try_recv() {
            match request {
                Request::Copy(text) => {
                    owned = Some(text.clone());
                    cleared_selection = None;
                    conn.set_selection_owner(window, atoms.clipboard, CURRENT_TIME)?;
                    conn.flush()?;
                    if enabled && remember_with_limit(&mut history, &text, max_clips) {
                        save(&history);
                        let _ = updates.send(Update::History(history.clone()));
                    }
                }
                Request::TogglePin(text) => {
                    if let Some(clip) = history.iter_mut().find(|clip| clip.text == text) {
                        clip.pinned = !clip.pinned;
                    }
                    save(&history);
                    let _ = updates.send(Update::History(history.clone()));
                }
                Request::Remove(text) => {
                    let owner = conn.get_selection_owner(atoms.clipboard)?.reply()?.owner;
                    if owner != 0 && owner != window {
                        cleared_selection = fetch(&conn, window, &atoms, &mut owned)?
                            .filter(|selection| selection == &text);
                    } else if owned.as_deref() == Some(text.as_str()) {
                        cleared_selection = Some(text.clone());
                    }
                    history.retain(|clip| clip.text != text);
                    save(&history);
                    let _ = updates.send(Update::History(history.clone()));
                }
                Request::Clear => {
                    // Snapshot the current selection. A clear is a user action
                    // against existing history, not permission to re-add the
                    // same external clipboard value on the next poll.
                    let owner = conn.get_selection_owner(atoms.clipboard)?.reply()?.owner;
                    cleared_selection = if owner != 0 && owner != window {
                        fetch(&conn, window, &atoms, &mut owned)?
                    } else {
                        owned.clone()
                    };
                    history.clear();
                    save(&history);
                    let _ = updates.send(Update::History(history.clone()));
                }
                Request::Configure {
                    enabled: next_enabled,
                    max_clips: next_max,
                } => {
                    enabled = next_enabled;
                    max_clips = next_max.clamp(5, 200);
                    if history.len() > max_clips {
                        history.truncate(max_clips);
                        save(&history);
                        let _ = updates.send(Update::History(history.clone()));
                    }
                }
            }
        }

        while let Some(event) = conn.poll_for_event()? {
            if let Event::SelectionRequest(request) = event {
                serve(&conn, &atoms, &request, owned.as_deref())?;
            }
        }

        ticks = ticks.wrapping_add(1);
        if enabled && ticks.is_multiple_of(6) {
            let owner = conn.get_selection_owner(atoms.clipboard)?.reply()?.owner;
            if owner != 0
                && owner != window
                && let Some(text) = fetch(&conn, window, &atoms, &mut owned)?
                && cleared_selection.as_deref() != Some(text.as_str())
            {
                cleared_selection = None;
                if remember_with_limit(&mut history, &text, max_clips) {
                    save(&history);
                    let _ = updates.send(Update::History(history.clone()));
                }
            }
        }

        thread::sleep(Duration::from_millis(100));
    }
}

/// Ask the current owner for the text. Bounded wait so a hung owner cannot
/// stall the notch.
fn fetch(
    conn: &impl Connection,
    window: Atom,
    atoms: &Atoms,
    owned: &mut Option<String>,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    conn.convert_selection(
        window,
        atoms.clipboard,
        atoms.utf8,
        atoms.property,
        CURRENT_TIME,
    )?;
    conn.flush()?;
    let deadline = Instant::now() + Duration::from_millis(600);
    while Instant::now() < deadline {
        match conn.poll_for_event()? {
            Some(Event::SelectionNotify(notify)) => {
                if notify.property == 0 {
                    return Ok(None);
                }
                let reply =
                    conn.get_property(true, window, notify.property, AtomEnum::ANY, 0, 1 << 21)?;
                let reply = reply.reply()?;
                if reply.format != 8 || reply.value.is_empty() {
                    return Ok(None);
                }
                return Ok(Some(String::from_utf8_lossy(&reply.value).into_owned()));
            }
            Some(Event::SelectionRequest(request)) => {
                serve(conn, atoms, &request, owned.as_deref())?;
            }
            Some(_) => {}
            None => thread::sleep(Duration::from_millis(10)),
        }
    }
    Ok(None)
}

fn serve(
    conn: &impl Connection,
    atoms: &Atoms,
    request: &SelectionRequestEvent,
    owned: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut notify = SelectionNotifyEvent {
        response_type: 31,
        sequence: 0,
        time: request.time,
        requestor: request.requestor,
        selection: request.selection,
        target: request.target,
        property: AtomEnum::NONE.into(),
    };

    if let Some(text) = owned {
        let target = request.target;
        let value: Option<(Atom, u8, Vec<u8>)> = if target == atoms.targets {
            let mut data = Vec::with_capacity(16);
            for candidate in [atoms.utf8, atoms.string, atoms.text] {
                data.extend_from_slice(&candidate.to_le_bytes());
            }
            Some((AtomEnum::ATOM.into(), 32, data))
        } else if target == atoms.utf8 || target == atoms.text || target == atoms.string {
            Some((target, 8, text.as_bytes().to_vec()))
        } else {
            None
        };

        if let Some((type_atom, format, data)) = value {
            let units = match format {
                32 => data.len() as u32 / 4,
                _ => data.len() as u32,
            };
            let property = if request.property == u32::from(AtomEnum::NONE) {
                request.target
            } else {
                request.property
            };
            conn.change_property(
                PropMode::REPLACE,
                request.requestor,
                property,
                type_atom,
                format,
                units,
                &data,
            )?;
            notify.property = property;
        }
    }

    conn.send_event(false, request.requestor, EventMask::NO_EVENT, notify)?;
    conn.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_is_deduped_capped_and_newest_first() {
        let mut items: Vec<Clip> = Vec::new();
        remember(&mut items, "first");
        remember(&mut items, "second");
        assert_eq!(items[0].text, "second");
        remember(&mut items, "first");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].text, "first");
        remember(&mut items, "first");
        assert_eq!(items.len(), 2);

        for i in 0..MAX_CLIPS + 5 {
            remember(&mut items, &format!("bulk {i}"));
        }
        assert_eq!(items.len(), MAX_CLIPS);
    }

    #[test]
    fn promoting_a_pinned_clip_preserves_its_pin() {
        let mut items = vec![
            Clip {
                text: "newer".into(),
                pinned: false,
            },
            Clip {
                text: "kept".into(),
                pinned: true,
            },
        ];
        remember(&mut items, "kept");
        assert_eq!(items[0].text, "kept");
        assert!(items[0].pinned);
    }

    #[test]
    fn blank_and_oversized_clips_are_dropped() {
        let mut items: Vec<Clip> = Vec::new();
        remember(&mut items, "   ");
        remember(&mut items, &"x".repeat(20_001));
        assert!(items.is_empty());
    }

    #[test]
    fn pinned_clips_sort_before_the_rest() {
        let clips = Clips {
            tx: channel().0,
            rx: channel().1,
            items: vec![
                Clip {
                    text: "fresh".into(),
                    pinned: false,
                },
                Clip {
                    text: "kept".into(),
                    pinned: true,
                },
            ],
            status: String::new(),
            available: true,
        };
        let ordered = clips.ordered();
        assert_eq!(ordered[0].text, "kept");
    }
}
