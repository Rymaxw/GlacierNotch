use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use x11rb::CURRENT_TIME;
use x11rb::connection::Connection;
use x11rb::errors::ReplyError;
use x11rb::protocol::shape::{ConnectionExt as _, SK, SO};
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ClipOrdering, ConnectionExt as _, InputFocus, Rectangle,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SavedFocus {
    window: u32,
    revert_to: InputFocus,
}

/// Restrict pointer hits to the visible notch and manage X11 keyboard focus.
/// The transparent portion of the permanently-sized X11 window must never steal
/// clicks from other apps, and because the notch is override-redirect the window
/// manager never gives it keyboard focus on click.
pub struct InputShape {
    conn: x11rb::rust_connection::RustConnection,
    root: u32,
    window: u32,
    net_active_window: Option<Atom>,
    expanded: bool,
    holds_focus: bool,
    previous_focus: Option<SavedFocus>,
}

impl InputShape {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Result<Self, String> {
        let window = match cc.window_handle().map_err(|e| e.to_string())?.as_raw() {
            RawWindowHandle::Xlib(h) => h.window as u32,
            RawWindowHandle::Xcb(h) => h.window.get(),
            _ => return Err("X11 window required for Glacier".into()),
        };
        let (conn, screen_num) = x11rb::connect(None).map_err(|e| e.to_string())?;
        let root = conn.setup().roots[screen_num].root;
        let net_active_window = conn
            .intern_atom(false, b"_NET_ACTIVE_WINDOW")
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .map(|reply| reply.atom);
        let mut input = Self {
            conn,
            root,
            window,
            net_active_window,
            expanded: true,
            holds_focus: false,
            previous_focus: None,
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

    /// Take X11 keyboard focus for the override-redirect notch window,
    /// remembering the previously focused window so it can be restored.
    pub fn acquire_focus(&mut self) -> Result<(), String> {
        let current = self
            .conn
            .get_input_focus()
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        if current.focus == self.window {
            self.holds_focus = true;
            return Ok(());
        }
        self.previous_focus = Some(SavedFocus {
            window: current.focus,
            revert_to: sanitize_revert(current.revert_to),
        });
        self.conn
            .set_input_focus(InputFocus::POINTER_ROOT, self.window, CURRENT_TIME)
            .map_err(|e| e.to_string())?
            .check()
            .map_err(|e| e.to_string())?;
        self.conn.flush().map_err(|e| e.to_string())?;
        self.holds_focus = true;
        Ok(())
    }

    /// Return X11 keyboard focus to the previously focused window if the notch
    /// still owns focus. If another application already took focus, leave it
    /// untouched.
    pub fn release_focus(&mut self) -> Result<(), String> {
        let previous = self.previous_focus.take();
        if !self.holds_focus {
            return Ok(());
        }
        self.holds_focus = false;

        let current = self
            .conn
            .get_input_focus()
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        if current.focus != self.window {
            return Ok(());
        }

        if let Some(saved) = previous
            && saved.window != self.window
            && self.try_set_focus(saved.window, saved.revert_to)?
        {
            return Ok(());
        }

        if let Some(active) = self.active_window()
            && active != self.window
            && self.try_set_focus(active, InputFocus::POINTER_ROOT)?
        {
            return Ok(());
        }

        let _ = self.try_set_focus(
            u32::from(InputFocus::POINTER_ROOT),
            InputFocus::POINTER_ROOT,
        )?;
        Ok(())
    }

    fn try_set_focus(&self, window: u32, revert_to: InputFocus) -> Result<bool, String> {
        let cookie = self
            .conn
            .set_input_focus(revert_to, window, CURRENT_TIME)
            .map_err(|e| e.to_string())?;
        self.conn.flush().map_err(|e| e.to_string())?;
        match cookie.check() {
            Ok(()) => Ok(true),
            Err(ReplyError::X11Error(_)) => Ok(false),
            Err(error) => Err(error.to_string()),
        }
    }

    fn active_window(&self) -> Option<u32> {
        let atom = self.net_active_window?;
        let reply = self
            .conn
            .get_property(false, self.root, atom, AtomEnum::WINDOW, 0, 1)
            .ok()?
            .reply()
            .ok()?;
        let window = reply.value32()?.next()?;
        (window != 0).then_some(window)
    }
}

impl Drop for InputShape {
    fn drop(&mut self) {
        let _ = self.release_focus();
    }
}

fn sanitize_revert(revert_to: InputFocus) -> InputFocus {
    match revert_to {
        InputFocus::NONE | InputFocus::POINTER_ROOT | InputFocus::PARENT => revert_to,
        _ => InputFocus::POINTER_ROOT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_revert_preserves_standard_modes_and_falls_back_for_extension_modes() {
        assert_eq!(sanitize_revert(InputFocus::NONE), InputFocus::NONE);
        assert_eq!(
            sanitize_revert(InputFocus::POINTER_ROOT),
            InputFocus::POINTER_ROOT
        );
        assert_eq!(sanitize_revert(InputFocus::PARENT), InputFocus::PARENT);
        assert_eq!(
            sanitize_revert(InputFocus::FOLLOW_KEYBOARD),
            InputFocus::POINTER_ROOT
        );
    }
}
