use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use x11rb::connection::Connection;
use x11rb::protocol::shape::{ConnectionExt as _, SK, SO};
use x11rb::protocol::xproto::{ClipOrdering, Rectangle};

/// Restrict pointer hits to the visible notch. The transparent portion of the
/// permanently-sized X11 window must never steal clicks from other apps.
pub struct InputShape {
    conn: x11rb::rust_connection::RustConnection,
    window: u32,
    expanded: bool,
}

impl InputShape {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Result<Self, String> {
        let window = match cc.window_handle().map_err(|e| e.to_string())?.as_raw() {
            RawWindowHandle::Xlib(h) => h.window as u32,
            RawWindowHandle::Xcb(h) => h.window.get(),
            _ => return Err("X11 window required for Glacier".into()),
        };
        let (conn, _) = x11rb::connect(None).map_err(|e| e.to_string())?;
        let mut input = Self {
            conn,
            window,
            expanded: true,
        };
        input.set(false)?;
        Ok(input)
    }

    pub fn set(&mut self, expanded: bool) -> Result<(), String> {
        if expanded == self.expanded {
            return Ok(());
        }
        let width: u16 = if expanded { 424 } else { 214 };
        let height: u16 = if expanded {
            crate::ui::H_EXPANDED as u16
        } else {
            34
        };
        let x: u16 = (424 - width) / 2;
        self.conn
            .shape_rectangles(
                SO::SET,
                SK::INPUT,
                ClipOrdering::UNSORTED,
                self.window,
                0,
                0,
                &[Rectangle {
                    x: x as i16,
                    y: 0,
                    width,
                    height,
                }],
            )
            .map_err(|e| e.to_string())?;
        self.conn.flush().map_err(|e| e.to_string())?;
        self.expanded = expanded;
        Ok(())
    }
}
