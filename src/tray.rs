//! Native XFCE system tray icon via `org.kde.StatusNotifierItem` and
//! `com.canonical.dbusmenu` over the existing `zbus` session connection.
//!
//! No GTK or AppIndicator C dependencies are needed; if the session bus or
//! `org.kde.StatusNotifierWatcher` is absent (for example in headless Xvfb
//! tests), the tray quietly stays inactive.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

use zbus::blocking::{Connection, Proxy, connection::Builder};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

static TRAY_SEQ: AtomicU32 = AtomicU32::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayEvent {
    OpenSettings,
    ToggleNotch,
    RestartService,
    Quit,
}

pub struct Tray {
    rx: Receiver<TrayEvent>,
    stop_tx: Option<Sender<()>>,
    notch_visible: Arc<AtomicBool>,
    enabled: bool,
}

impl Tray {
    pub fn new(enabled: bool) -> Self {
        let (_, rx) = channel();
        let mut tray = Self {
            rx,
            stop_tx: None,
            notch_visible: Arc::new(AtomicBool::new(true)),
            enabled: false,
        };
        tray.set_enabled(enabled);
        tray
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if enabled == self.enabled {
            return;
        }
        self.enabled = enabled;
        if enabled {
            let (event_tx, event_rx) = channel();
            let (stop_tx, stop_rx) = channel();
            let visible = Arc::clone(&self.notch_visible);
            self.rx = event_rx;
            self.stop_tx = Some(stop_tx);
            thread::Builder::new()
                .name("glacier-tray".into())
                .spawn(move || run_tray(event_tx, stop_rx, visible))
                .ok();
        } else if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(());
        }
    }

    pub fn set_notch_visible(&self, visible: bool) {
        self.notch_visible.store(visible, Ordering::Relaxed);
    }

    pub fn poll(&self) -> Option<TrayEvent> {
        self.rx.try_recv().ok()
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(());
        }
    }
}

type IconPixmap = (i32, i32, Vec<u8>);
type ToolTipData = (String, Vec<IconPixmap>, String, String);

struct StatusNotifierItem {
    tx: Sender<TrayEvent>,
    pixmap: Vec<IconPixmap>,
}

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl StatusNotifierItem {
    #[zbus(property)]
    fn category(&self) -> &'static str {
        "ApplicationStatus"
    }

    #[zbus(property)]
    fn id(&self) -> &'static str {
        "glaciernotch"
    }

    #[zbus(property)]
    fn title(&self) -> &'static str {
        "GlacierNotch"
    }

    #[zbus(property)]
    fn status(&self) -> &'static str {
        "Active"
    }

    #[zbus(property)]
    fn window_id(&self) -> u32 {
        0
    }

    #[zbus(property)]
    fn icon_name(&self) -> &'static str {
        "glaciernotch"
    }

    #[zbus(property)]
    fn icon_pixmap(&self) -> Vec<IconPixmap> {
        self.pixmap.clone()
    }

    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn menu(&self) -> OwnedObjectPath {
        OwnedObjectPath::try_from("/MenuBar").expect("valid static menu path")
    }

    #[zbus(property)]
    fn tool_tip(&self) -> ToolTipData {
        (
            "glaciernotch".to_owned(),
            Vec::new(),
            "GlacierNotch".to_owned(),
            "Click to open Settings · Right-click for quick actions".to_owned(),
        )
    }

    fn activate(&self, _x: i32, _y: i32) {
        let _ = self.tx.send(TrayEvent::OpenSettings);
    }

    fn secondary_activate(&self, _x: i32, _y: i32) {
        let _ = self.tx.send(TrayEvent::ToggleNotch);
    }

    fn context_menu(&self, _x: i32, _y: i32) {}

    fn scroll(&self, _delta: i32, _orientation: &str) {}
}

type MenuLayoutItem = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);

struct TrayMenu {
    tx: Sender<TrayEvent>,
    notch_visible: Arc<AtomicBool>,
    revision: AtomicU32,
}

impl TrayMenu {
    fn item_props(&self, id: i32) -> HashMap<String, OwnedValue> {
        let mut map = HashMap::new();
        let insert_str = |m: &mut HashMap<String, OwnedValue>, key: &str, val: &str| {
            if let Ok(owned) = OwnedValue::try_from(Value::from(val)) {
                m.insert(key.to_owned(), owned);
            }
        };
        let insert_bool = |m: &mut HashMap<String, OwnedValue>, key: &str, val: bool| {
            m.insert(key.to_owned(), OwnedValue::from(val));
        };
        match id {
            0 => {
                insert_str(&mut map, "children-display", "submenu");
            }
            1 => {
                insert_str(&mut map, "label", "Open Settings...");
                insert_bool(&mut map, "enabled", true);
                insert_bool(&mut map, "visible", true);
            }
            2 => {
                let label = if self.notch_visible.load(Ordering::Relaxed) {
                    "Hide Notch"
                } else {
                    "Show Notch"
                };
                insert_str(&mut map, "label", label);
                insert_bool(&mut map, "enabled", true);
                insert_bool(&mut map, "visible", true);
            }
            3 => {
                insert_str(&mut map, "type", "separator");
                insert_bool(&mut map, "visible", true);
            }
            4 => {
                insert_str(&mut map, "label", "Restart GlacierNotch");
                insert_bool(&mut map, "enabled", true);
                insert_bool(&mut map, "visible", true);
            }
            5 => {
                insert_str(&mut map, "label", "Quit");
                insert_bool(&mut map, "enabled", true);
                insert_bool(&mut map, "visible", true);
            }
            _ => {}
        }
        map
    }

    fn build_layout(&self, parent_id: i32) -> MenuLayoutItem {
        if parent_id != 0 {
            return (parent_id, self.item_props(parent_id), Vec::new());
        }
        let mut children = Vec::new();
        for id in [1, 2, 3, 4, 5] {
            let child: MenuLayoutItem = (id, self.item_props(id), Vec::new());
            if let Ok(owned) = OwnedValue::try_from(Value::from(child)) {
                children.push(owned);
            }
        }
        (0, self.item_props(0), children)
    }
}

#[zbus::interface(name = "com.canonical.dbusmenu")]
impl TrayMenu {
    #[zbus(property)]
    fn version(&self) -> u32 {
        3
    }

    #[zbus(property)]
    fn text_direction(&self) -> &'static str {
        "ltr"
    }

    #[zbus(property)]
    fn status(&self) -> &'static str {
        "normal"
    }

    #[zbus(property)]
    fn icon_theme_path(&self) -> Vec<String> {
        Vec::new()
    }

    fn get_layout(
        &self,
        parent_id: i32,
        _recursion_depth: i32,
        _property_names: Vec<String>,
    ) -> (u32, MenuLayoutItem) {
        let rev = self.revision.fetch_add(1, Ordering::Relaxed);
        (rev, self.build_layout(parent_id))
    }

    fn get_group_properties(
        &self,
        ids: Vec<i32>,
        _property_names: Vec<String>,
    ) -> Vec<(i32, HashMap<String, OwnedValue>)> {
        ids.into_iter()
            .map(|id| (id, self.item_props(id)))
            .collect()
    }

    fn get_property(&self, id: i32, name: &str) -> OwnedValue {
        self.item_props(id)
            .remove(name)
            .unwrap_or_else(|| OwnedValue::try_from(Value::from("")).expect("string value"))
    }

    fn event(&self, id: i32, event_id: &str, _data: OwnedValue, _timestamp: u32) {
        if event_id != "clicked" {
            return;
        }
        let event = match id {
            1 => Some(TrayEvent::OpenSettings),
            2 => Some(TrayEvent::ToggleNotch),
            4 => Some(TrayEvent::RestartService),
            5 => Some(TrayEvent::Quit),
            _ => None,
        };
        if let Some(event) = event {
            let _ = self.tx.send(event);
        }
    }

    fn event_group(&self, events: Vec<(i32, String, OwnedValue, u32)>) -> Vec<i32> {
        for (id, event_id, data, timestamp) in events {
            self.event(id, &event_id, data, timestamp);
        }
        Vec::new()
    }

    fn about_to_show(&self, _id: i32) -> bool {
        true
    }

    fn about_to_show_group(&self, ids: Vec<i32>) -> (Vec<i32>, Vec<i32>) {
        (ids, Vec::new())
    }
}

fn run_tray(tx: Sender<TrayEvent>, stop_rx: Receiver<()>, notch_visible: Arc<AtomicBool>) {
    let seq = TRAY_SEQ.fetch_add(1, Ordering::Relaxed);
    let service_name = format!("org.kde.StatusNotifierItem-{}-{seq}", std::process::id());
    let Ok(builder) = Builder::session() else {
        return;
    };
    let Ok(builder) = builder.name(service_name.as_str()) else {
        return;
    };
    let sni = StatusNotifierItem {
        tx: tx.clone(),
        pixmap: vec![build_tray_icon_argb32(24)],
    };
    let menu = TrayMenu {
        tx,
        notch_visible,
        revision: AtomicU32::new(1),
    };
    let Ok(builder) = builder.serve_at("/StatusNotifierItem", sni) else {
        return;
    };
    let Ok(builder) = builder.serve_at("/MenuBar", menu) else {
        return;
    };
    let Ok(conn): Result<Connection, _> = builder.build() else {
        return;
    };

    if let Ok(watcher) = Proxy::new(
        &conn,
        "org.kde.StatusNotifierWatcher",
        "/StatusNotifierWatcher",
        "org.kde.StatusNotifierWatcher",
    ) {
        let _ = watcher.call::<_, _, ()>("RegisterStatusNotifierItem", &(service_name.as_str(),));
    }

    // Hold the D-Bus connection alive until the tray is disabled or app exits.
    let _ = stop_rx.recv();
    drop(conn);
}

/// Generates a crisp 24×24 ARGB32 fallback icon (dark rounded pill with gold
/// crown and ice-blue eyes) so the tray looks sharp even if the icon theme
/// cache has not indexed `glaciernotch.svg`.
pub fn build_tray_icon_argb32(size: usize) -> (i32, i32, Vec<u8>) {
    let mut argb = vec![0u8; size * size * 4];
    let s = size as f32;
    let radius = s * 0.22;

    for y in 0..size {
        for x in 0..size {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            // Rounded square background (#09090c)
            let dx = (fx - s * 0.5).abs() - (s * 0.5 - radius);
            let dy = (fy - s * 0.5).abs() - (s * 0.5 - radius);
            let outside = dx.max(0.0).hypot(dy.max(0.0)) > radius;
            let pixel = if outside {
                [0, 0, 0, 0]
            } else {
                // Default dark fill with subtle rim
                let mut col = [255, 11, 11, 16];
                let nx = fx / s;
                let ny = fy / s;
                // Gold spiky hair band across top third
                let spike = 0.20 + ((nx * 14.0).sin().abs()) * 0.14;
                if ny >= spike && ny <= 0.52 && (0.14..=0.86).contains(&nx) {
                    col = [255, 248, 195, 75];
                }
                // Two glowing ice-blue eyes
                let left_eye = ((nx - 0.38) / 0.09).powi(2) + ((ny - 0.68) / 0.12).powi(2) <= 1.0;
                let right_eye = ((nx - 0.62) / 0.09).powi(2) + ((ny - 0.68) / 0.12).powi(2) <= 1.0;
                if left_eye || right_eye {
                    col = [255, 108, 207, 255];
                }
                col
            };
            let idx = (y * size + x) * 4;
            argb[idx..idx + 4].copy_from_slice(&pixel);
        }
    }
    (size as i32, size as i32, argb)
}

/// RGBA version of the icon for `egui::IconData` on the Settings window.
pub fn build_window_icon_rgba(size: usize) -> Vec<u8> {
    let (_, _, argb) = build_tray_icon_argb32(size);
    let mut rgba = Vec::with_capacity(argb.len());
    for &[a, r, g, b] in argb.as_chunks::<4>().0 {
        rgba.extend_from_slice(&[r, g, b, a]);
    }
    rgba
}
