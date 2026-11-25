//! Minimal X11 clipboard client used by the Glacier integration tests.
//!
//! `clip-probe set <text> <seconds>` owns CLIPBOARD for a while and serves it,
//! so Glacier can pull the text into its history. `clip-probe get` asks the
//! current owner for the text, which proves Glacier serves what it copied.

use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

use x11rb::COPY_DEPTH_FROM_PARENT;
use x11rb::CURRENT_TIME;
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, PropMode, SelectionNotifyEvent,
    WindowClass,
};

fn atom(conn: &impl Connection, name: &[u8]) -> Result<Atom, Box<dyn std::error::Error>> {
    Ok(conn.intern_atom(false, name)?.reply()?.atom)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("set") => match serve(
            args.get(2).cloned().unwrap_or_default(),
            args.get(3).and_then(|s| s.parse().ok()).unwrap_or(6),
        ) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("clip-probe set failed: {error}");
                ExitCode::FAILURE
            }
        },
        Some("get") => match read() {
            Ok(text) => {
                println!("{text}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("clip-probe get failed: {error}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("usage: clip-probe set <text> [seconds] | clip-probe get");
            ExitCode::FAILURE
        }
    }
}

fn serve(text: String, seconds: u64) -> Result<(), Box<dyn std::error::Error>> {
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

    let clipboard = atom(&conn, b"CLIPBOARD")?;
    let utf8 = atom(&conn, b"UTF8_STRING")?;
    let targets = atom(&conn, b"TARGETS")?;
    let text_atom = atom(&conn, b"TEXT")?;
    let property = atom(&conn, b"CLIP_PROBE")?;
    let string_atom: Atom = AtomEnum::STRING.into();

    conn.set_selection_owner(window, clipboard, CURRENT_TIME)?;
    conn.flush()?;
    println!("CLAIMED");

    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        while let Some(event) = conn.poll_for_event()? {
            let Event::SelectionRequest(request) = event else {
                continue;
            };
            let mut notify = SelectionNotifyEvent {
                response_type: 31,
                sequence: 0,
                time: request.time,
                requestor: request.requestor,
                selection: request.selection,
                target: request.target,
                property: AtomEnum::NONE.into(),
            };
            let payload: Option<(Atom, u8, Vec<u8>)> = if request.target == targets {
                let mut data = Vec::new();
                for candidate in [utf8, string_atom, text_atom] {
                    data.extend_from_slice(&candidate.to_le_bytes());
                }
                Some((AtomEnum::ATOM.into(), 32, data))
            } else if request.target == utf8
                || request.target == text_atom
                || request.target == string_atom
            {
                Some((request.target, 8, text.as_bytes().to_vec()))
            } else {
                None
            };
            if let Some((type_atom, format, data)) = payload {
                let units = if format == 32 {
                    data.len() as u32 / 4
                } else {
                    data.len() as u32
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
            conn.send_event(false, request.requestor, EventMask::NO_EVENT, notify)?;
            conn.flush()?;
        }
        thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

fn read() -> Result<String, Box<dyn std::error::Error>> {
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

    let clipboard = atom(&conn, b"CLIPBOARD")?;
    let utf8 = atom(&conn, b"UTF8_STRING")?;
    let property = atom(&conn, b"CLIP_PROBE")?;

    conn.convert_selection(window, clipboard, utf8, property, CURRENT_TIME)?;
    conn.flush()?;

    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if let Some(Event::SelectionNotify(notify)) = conn.poll_for_event()? {
            if notify.property == 0 {
                return Err("owner refused the conversion".into());
            }
            if notify.property != property {
                return Err("owner did not honor the requestor property".into());
            }
            let reply = conn.get_property(true, window, property, AtomEnum::ANY, 0, 1 << 20)?;
            let reply = reply.reply()?;
            return Ok(String::from_utf8_lossy(&reply.value).into_owned());
        }
        thread::sleep(Duration::from_millis(20));
    }
    Err("timed out waiting for SelectionNotify".into())
}
