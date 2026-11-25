# GlacierNotch

A small Linux/X11 notch-like music overlay. Built in Rust with eframe/egui and MPRIS over D-Bus. Inspired by Max's macOS [Gliese](https://github.com/Rymaxw/Gliese), but this is a separate native Linux application, not an automated Swift port.

## v0.2

- Top-center notch above the XFCE panel. The X11 window stays at a fixed size; only the painted panel animates on hover. The input shape passes clicks through the transparent area when collapsed.
- Expanded media panel: MPRIS album artwork when the player supplies a readable local `mpris:artUrl`, track, artist, album, progress and centered previous / play-pause / next controls. Remote artwork URLs are not fetched; unavailable art uses a title initial on a track-specific color tile.
- Collapsed state: the idle mascot is just animated hair, blinking eyes, and a gold aura; the notch itself is the face. A playing or paused track replaces it with the cover (or title initial) and title.
- Music colors: the dominant visible hue of a local cover tints the track title, timestamps, progress bar, and Play/Pause icon. Missing or monochrome artwork falls back to a stable color from the track title and artist. Artwork is never downloaded.
- Selects a playing source through `org.mpris.MediaPlayer2.*` on the session bus. A monotonic clock advances progress between MPRIS updates, including players that report stale Position values.
- Lyrics: first look for `~/.local/share/glacier/lyrics/Artist - Title.lrc` or `Title.lrc`; if absent, automatically search LRCLIB by track and artist (as Max explicitly chose). This sends the current track title and artist to LRCLIB. Synced LRC shows the current and next lines; untimed lyrics show an excerpt. Responses are cached in memory for the session.
- No volume mixer or CPU monitor. Shelf and clipboard are provided in their own panels; album art is read from local MPRIS artwork URLs only.

### Desktop app (Linux Mint XFCE)

GlacierNotch is installed at `~/Applications/GlacierNotch`. Open it from the XFCE application menu; its desktop launcher starts `glaciernotch.service` in the user manager, so no terminal needs to remain open. The same launcher in `~/.config/autostart/glaciernotch.desktop` starts it once per desktop login. The old `glacier.desktop` autostart entry is removed to prevent duplicate windows. The launcher and service are kept in `packaging/` for future installation; after rebuilding, reinstall the binary to `~/Applications/GlacierNotch` and restart the user service to deploy a new version. User data remains under `~/.local/share/glacier/`.

```sh
systemctl --user status glaciernotch.service
systemctl --user restart glaciernotch.service  # after replacing the installed binary
systemctl --user stop glaciernotch.service     # close it for this session
```

For development only, you can run a separate process from the checkout, but stop the installed service first to avoid stacked notches:

```sh
cd ~/Projects/glacier
cargo run --release
# Or launch the built binary directly:
./target/release/glacier
```

To enable/disable login autostart: `~/Applications/GlacierNotch --autostart enable|disable|status`. This controls the XFCE login entry, not the running service. The service runs without demo mode and is managed independently of the terminal. Escape or Ctrl+Q closes the window when focused; open it again from the XFCE menu.

For UI-only testing without a media player: `./target/release/glacier --demo` shows an explicitly labelled sample track. Demo controls do not affect real playback.

### Build / check

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

The ignored `live_mpris_probe` test requires an MPRIS service named `org.mpris.MediaPlayer2.GlacierProbe` and is not part of routine tests. Real media integration was also tested with a temporary D-Bus player on this machine; the probe is not shipped or installed.

### Limitations

- X11-specific always-on-top positioning uses an override-redirect dropdown window. Wayland is not supported yet.
- Playback position is estimated between player updates; a seek or a newly selected track resets the estimate.
- Remote artwork URLs are not fetched. Missing local album art uses a track-derived color and monogram.
- The hover window can take focus when clicked; this version has no global hotkey or tray icon.
