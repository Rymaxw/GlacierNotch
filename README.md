<div align="center">
  <img src="packaging/glaciernotch.svg" width="96" height="96" alt="GlacierNotch app icon" />
  <h1>GlacierNotch</h1>
  <p>A Linux/X11 top-center notch overlay built in Rust with eframe/egui and MPRIS over D-Bus.</p>
</div>

GlacierNotch sits at the top center of an X11 desktop above the XFCE panel. The window stays at a fixed size while only the painted pill animates on hover, and its X11 input shape passes clicks through the transparent area when collapsed.

## Features

| Module | Details |
| --- | --- |
| **Idle companion** | Collapsed notch shows an animated Minato Namikaze head emblem in Kurama Chakra Mode by default, with Normal and Toad Sage forms selectable live in Settings. Any playing or paused track replaces the mascot with album art (or a title monogram) and track title. |
| **Music & lyrics** | Connects to `org.mpris.MediaPlayer2.*` over the session bus with monotonic progress tracking between updates. Local album artwork tints the track title, timestamps, progress bar, and Play/Pause icon; remote artwork URLs are never fetched. Lyrics load from `~/.local/share/glacier/lyrics/` first and fall back to LRCLIB search by track and artist. |
| **System controls** | PipeWire/PulseAudio volume and mute via `wpctl` or `pactl`, plus software display dimming through X11 CRTC gamma ramps. |
| **Focus timer** | Built-in Pomodoro timer (25-minute focus and 5-minute break defaults, configurable in Settings). |
| **File shelf** | Drop files onto the notch to open them, copy their paths, or keep them pinned across restarts. |
| **Clipboard & notes** | Native X11 clipboard history with pinning (no `xclip` or `xsel` needed) alongside Quick Notes stored locally. The override-redirect window acquires X11 keyboard focus only while editing a note and restores focus to the previous window on save or cancel. |
| **Settings & tray** | Standalone Settings window (`--settings`) to toggle modules, customize the idle companion with a live preview, configure per-module behavior, manage login autostart, and control `glaciernotch.service`. Changes write atomically to `~/.config/glacier/config.json` and hot-reload in the running notch. Optional native XFCE tray icon via `org.kde.StatusNotifierItem`. |

## Desktop setup (Linux Mint XFCE)

Packaging files live in [`packaging/`](packaging/):

- `packaging/glaciernotch.svg`: application icon
- `packaging/glaciernotch.desktop`: XFCE menu launcher (opens Settings via `"$HOME/Applications/GlacierNotch" --settings`)
- `packaging/glaciernotch-autostart.desktop`: login autostart entry (`~/.config/autostart/glaciernotch.desktop`) that starts `glaciernotch.service` silently without opening Settings
- `packaging/glaciernotch.service`: systemd user unit launching `%h/Applications/GlacierNotch`

After building a release binary, install it to `~/Applications/GlacierNotch` and restart the user service:

```sh
~/Applications/GlacierNotch --settings                     # open Settings & Feature Manager
~/Applications/GlacierNotch --autostart enable|disable|status
systemctl --user status glaciernotch.service
systemctl --user restart glaciernotch.service
systemctl --user stop glaciernotch.service
```

Configuration is stored in `~/.config/glacier/config.json`, and local user data (clipboard history, quick notes, shelf items, and local `.lrc` files) lives under `~/.local/share/glacier/`.

## Build from source

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

To run a development build directly from a checkout, stop the installed service first to avoid stacked notches:

```sh
systemctl --user stop glaciernotch.service
./target/release/glacier
```

For UI testing without a media player, pass `--demo` to display a labelled sample track whose controls do not affect real playback:

```sh
./target/release/glacier --demo
```

Press Escape or Ctrl+Q when focused to close the window.

## Limitations

- Uses an X11 override-redirect dropdown window; Wayland is not supported yet.
- Playback position is estimated between player updates, and seeking or switching tracks resets the estimate.
- Remote `mpris:artUrl` values are not downloaded; missing local artwork uses a track-derived color tile and title initial.
- No global hotkey yet.
