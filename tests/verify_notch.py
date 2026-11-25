#!/usr/bin/env python3
"""Isolated X11 regression for Glacier: geometry, click-through and panel actions.

Modes:
  collapsed    the pill is painted edge to edge and centred
  interaction  hover opens, the fixed window never moves, outside passes through
  tabs         every tab slot switches the panel
  media        lyric seek, scrubbing, +-5 s, speed, transport hit the real player

Run `media` under `dbus-run-session` so the fixture player and Glacier share a
private session bus.
"""
import ctypes
import json
from x11_input import XInput
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time

from PIL import Image
from typing import cast

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target/release/glacier"

W_EXPANDED, H_EXPANDED = 424, 284
W_COLLAPSED = 214
SCREEN_W = 1280
PANEL_X = (SCREEN_W - W_EXPANDED) // 2
PILL_X = PANEL_X + (W_EXPANDED - W_COLLAPSED) // 2
BG = (9, 9, 12)
TAB_SLOT = W_EXPANDED / 5.0


def until(check, timeout=12):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        result = check()
        if result:
            return result
        time.sleep(0.08)
    raise AssertionError("timed out waiting for condition")


def geometry(env):
    text = subprocess.check_output(["xwininfo", "-root", "-tree"], env=env, text=True)
    match = re.search(r'0x([\da-f]+) "GlacierNotch"[^\n]*? (\d+)x(\d+)\+(-?\d+)\+(-?\d+)', text)
    if match:
        return tuple(int(v, 16) if i == 0 else int(v) for i, v in enumerate(match.groups()))
    return None


def screenshot(env, path):
    subprocess.run(
        [
            "ffmpeg", "-y", "-loglevel", "error", "-f", "x11grab",
            "-draw_mouse", "0", "-video_size", f"{SCREEN_W}x720", "-i", env["DISPLAY"],
            "-frames:v", "1", str(path),
        ],
        env=env, check=True, timeout=15,
    )
    return Image.open(path).convert("RGB")


def assert_pill(img, state):
    """Painted pixels are exactly BG; anything clipped shows the backdrop."""
    for x in range(PILL_X + 8, PILL_X + W_COLLAPSED - 8, 6):
        pixel = img.getpixel((x, 5))
        assert pixel == BG, f"{state}: notch cut at x={x}, pixel={pixel}"
    for x in (PILL_X - 5, PILL_X + W_COLLAPSED + 4):
        assert img.getpixel((x, 5)) != BG, f"{state}: pill overdraw at x={x}"


def color_count(img, bounds, matches):
    pixels = img.crop(bounds).tobytes()
    return sum(matches(*rgb) for rgb in zip(pixels[::3], pixels[1::3], pixels[2::3]))


def record_mascot(env, target):
    size = (110, 32)
    left = SCREEN_W // 2 - size[0] // 2
    fps = 20
    raw = subprocess.check_output(
        ["ffmpeg", "-loglevel", "error", "-f", "x11grab", "-draw_mouse", "0",
         "-framerate", str(fps), "-video_size", f"{size[0]}x{size[1]}",
         "-i", f"{env['DISPLAY']}+{left},1", "-t", "4.6",
         "-f", "rawvideo", "-threads:v", "1", "-pix_fmt", "rgb24", "pipe:1"],
        env=env, timeout=15,
    )
    stride = size[0] * size[1] * 3
    assert len(raw) % stride == 0 and len(raw) // stride >= 85, "idle capture incomplete"
    frames = [Image.frombytes("RGB", size, raw[offset:offset + stride]) for offset in range(0, len(raw), stride)]
    scaled = [img.resize((size[0] * 3, size[1] * 3), Image.Resampling.NEAREST) for img in frames]
    scaled[0].save(target, save_all=True, append_images=scaled[1:], duration=1000 // fps, loop=0)
    return frames


def gold(r, g, b):
    return r > 85 and r > g * 1.05 and g > b * 1.35


def warm(r, g, b):
    return r > 150 and r > g * 1.30 and g >= b * 0.90


def cool(r, g, b):
    return b > 155 and b > g * 1.30 and g > r * 1.20


def signature(img, y0, y1):
    return bytes(
        sum(img.getpixel((x, y))[i] for x in range(PANEL_X + 8, PANEL_X + W_EXPANDED - 8, 3) for y in range(y0, y1, 3))
        % 251
        for i in range(3)
    )


def xpointer(display):
    x = ctypes.CDLL("libX11.so.6")
    t = ctypes.CDLL("libXtst.so.6")
    x.XOpenDisplay.restype = ctypes.c_void_p
    x.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
    x.XDefaultRootWindow.restype = ctypes.c_ulong
    x.XFlush.argtypes = [ctypes.c_void_p]
    x.XQueryPointer.argtypes = [ctypes.c_void_p, ctypes.c_ulong] + [ctypes.c_void_p] * 7
    t.XTestFakeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
    t.XTestFakeButtonEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
    handle = x.XOpenDisplay(display.encode())
    assert handle, "Xvfb connection failed"
    root = x.XDefaultRootWindow(handle)

    def move(px, py, settle=0.42):
        assert t.XTestFakeMotionEvent(handle, 0, px, py, 0)
        x.XFlush(handle)
        time.sleep(settle)
        data = [ctypes.c_ulong(), ctypes.c_ulong(), ctypes.c_int(), ctypes.c_int(), ctypes.c_int(), ctypes.c_int(), ctypes.c_uint()]
        assert x.XQueryPointer(handle, root, *(ctypes.byref(item) for item in data))
        return data[1].value

    def click(px, py, settle=0.5):
        move(px, py, settle=0.06)
        assert t.XTestFakeButtonEvent(handle, 1, 1, 0)
        x.XFlush(handle)
        time.sleep(0.05)
        assert t.XTestFakeButtonEvent(handle, 1, 0, 0)
        x.XFlush(handle)
        time.sleep(settle)

    return move, click


def read_log(path):
    if not path.exists():
        return []
    entries = path.read_text().split()
    collapsed = []
    for entry in entries:
        if not collapsed or collapsed[-1] != entry:
            collapsed.append(entry)
    return collapsed


def main():
    mode = sys.argv[1] if len(sys.argv) > 1 else "collapsed"
    assert mode in {"collapsed", "interaction", "tabs", "media", "controls", "focus", "clips", "notes", "shelf", "keyboard"}, mode
    assert BIN.exists(), "build the release binary first"
    if mode == "media":
        assert "DBUS_SESSION_BUS_ADDRESS" in os.environ, "run media mode under dbus-run-session"
    with tempfile.TemporaryDirectory(prefix="glacier-qa-") as temp:
        base = Path(temp)
        shots = Path(os.environ.get("GLACIER_QA_SHOTS", temp))
        shots.mkdir(parents=True, exist_ok=True)
        # :97 is reserved for tests, never Max's real DISPLAY=:0.
        env = {**os.environ, "DISPLAY": ":97", "XDG_DATA_HOME": str(base / "data")}
        assert not Path("/tmp/.X97-lock").exists(), "test display :97 already in use"
        stub_log = base / "stub.log"
        if mode in {"collapsed", "interaction", "tabs", "media", "controls", "focus", "clips", "notes", "shelf", "keyboard"}:
            stubs = base / "stubs"
            stubs.mkdir()
            (stubs / "wpctl").write_text(
                "#!/bin/sh\n"
                f'echo "wpctl $*" >> {stub_log}\n'
                'if [ "$1" = "get-volume" ]; then echo "Volume: 1.00"; fi\n'
            )
            (stubs / "xrandr").write_text(
                "#!/bin/sh\n"
                f'echo "xrandr $*" >> {stub_log}\n'
                'if [ "$1" = "--query" ]; then printf "Screen 0: minimum 8 x 8\\nHDMI-0 connected 1920x1080+0+0\\n"; fi\n'
            )
            for program in ("notify-send", "xdg-open"):
                (stubs / program).write_text("#!/bin/sh\nexit 0\n")
            for script in ("wpctl", "xrandr", "notify-send", "xdg-open"):
                (stubs / script).chmod(0o755)
            env["PATH"] = f"{stubs}:{env['PATH']}"
        xvfb = subprocess.Popen(
            ["Xvfb", ":97", "-screen", "0", f"{SCREEN_W}x720x24", "-ac", "+extension", "GLX"],
            stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
        )
        app = probe = backdrop = None
        input_driver = None
        try:
            def display_ready():
                assert xvfb.poll() is None, "owned Xvfb did not start"
                return subprocess.run(["xdpyinfo"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode == 0

            until(display_ready)
            backdrop = subprocess.Popen(
                ["/usr/bin/python3", str(ROOT / "tests/backdrop.py")],
                env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            )
            assert backdrop.stdout is not None
            until(lambda: backdrop.stdout.readline().strip() == b"BACKDROP_READY")

            log = base / "calls.log"
            demo_lyrics = base / "data/glacier/lyrics"
            demo_lyrics.mkdir(parents=True, exist_ok=True)
            (demo_lyrics / "Glacier - Sample track demo.lrc").write_text(
                "[00:00.00]Demo lyric fixture\n[00:10.00]Next demo line\n"
            )
            if mode == "media":
                lyrics = base / "data/glacier/lyrics"
                lyrics.mkdir(parents=True, exist_ok=True)
                subprocess.run(["/usr/bin/python3", str(ROOT / "tests/artwork_fixture.py")], env=env, check=True)
                (lyrics / "Test Artist - Real D-Bus Test Track.lrc").write_text(
                    "[00:00.00]Opening line\n[00:59.00]Active lyric at one minute\n[01:30.00]Next lyric line\n"
                )
                (lyrics / "Test Artist - Second D-Bus Test Track.lrc").write_text("[00:00.00]Second track\n")
                env["GLACIER_PROBE_LOG"] = str(log)
                probe = subprocess.Popen(
                    ["/usr/bin/python3", str(ROOT / "tests/fixture_player.py")],
                    env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
                )
                until(lambda: "GlacierProbe" in subprocess.run(
                    ["dbus-send", "--session", "--dest=org.freedesktop.DBus", "--type=method_call",
                     "--print-reply", "/org/freedesktop/DBus", "org.freedesktop.DBus.ListNames"],
                    env=env, capture_output=True, text=True, check=False).stdout)

            app = subprocess.Popen(
                [str(BIN)] + ([] if mode in {"media", "collapsed"} else ["--demo"]),
                env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
            )
            expected = (W_EXPANDED, H_EXPANDED, PANEL_X, 0)
            wid, width, height, x, y = until(
                lambda: geometry(env) if geometry(env) and geometry(env)[1:] == expected else None
            )
            assert (width, height, x, y) == expected, (width, height, x, y)

            input_driver = XInput(env["DISPLAY"])
            move, click = input_driver.move, input_driver.click
            move(120, 320)
            collapsed_image = screenshot(env, shots / "collapsed.png")
            assert_pill(collapsed_image, "collapsed")
            if mode == "collapsed":
                frames = record_mascot(env, shots / "mascot-idle.gif")
                aura = (80, 6, 103, 28)
                assert max(color_count(frame, aura, gold) for frame in frames) >= 6, "aura does not extend past the hair"
                assert len({frame.crop(aura).tobytes() for frame in frames}) >= 3, "idle aura stayed static"
                for side in (-10, 10):
                    eye = (55 + side - 5, 14, 55 + side + 5, 28)
                    counts = [color_count(frame, eye, lambda r, g, b: b > 170 and b > r * 1.35) for frame in frames]
                    assert max(counts) >= 25, "blue eye missing"
                    assert min(counts) < max(counts) // 2, "eye did not blink"
                print("G1_NOTCH_VISIBLE")
                return
            if mode == "media":
                collapsed_title = (PILL_X + 46, 8, PILL_X + W_COLLAPSED - 8, 27)
                def collapsed_palette():
                    frame = screenshot(env, shots / "media-collapsed.png")
                    return color_count(frame, collapsed_title, warm) > 20

                until(collapsed_palette)

            assert move(480, 185) != wid, "transparent area outside the notch captured the pointer"
            assert move(PILL_X + W_COLLAPSED // 2, 15) == wid, "collapsed pill did not capture the pointer"
            time.sleep(0.5)
            assert geometry(env) == (wid, *expected), "native window moved or resized while opening"
            # Expanded: the panel fills the whole window, so the edges must be
            # painted while just outside the window stays untouched.
            wide = screenshot(env, shots / "expanded.png")
            for x in (PANEL_X + 2, PANEL_X + W_EXPANDED // 2, PANEL_X + W_EXPANDED - 3):
                assert wide.getpixel((x, 40)) == BG, f"expanded panel not painted at x={x}"
            for x in (PANEL_X - 4, PANEL_X + W_EXPANDED + 3):
                assert wide.getpixel((x, 40)) != BG, f"panel bled outside the window at x={x}"

            if mode == "notes":
                store = base / "data/glacier/notes.json"
                click(PANEL_X + int(TAB_SLOT * 4.5), 13)
                click(PANEL_X + 126, 46)                       # Notes view
                click(PANEL_X + W_EXPANDED - 55, 46)         # New note
                input_driver.focus(wid)
                click(PANEL_X + W_EXPANDED - 137, 214)       # Empty save keeps the editor
                assert not store.exists(), "An empty note was persisted"
                screenshot(env, shots / "notes-validation.png")
                click(PANEL_X + 110, 126)
                input_driver.type_text("first quick note")
                click(PANEL_X + W_EXPANDED - 137, 214)       # Save
                until(lambda: store.exists() and json.loads(store.read_text())[0]["text"] == "first quick note")

                input_driver.focus(wid)
                input_driver.key("Escape")
                app.wait(timeout=5)
                app = subprocess.Popen([str(BIN), "--demo"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
                wid = until(lambda: geometry(env))[0]
                move(PILL_X + W_COLLAPSED // 2, 15)
                time.sleep(0.6)
                click(PANEL_X + int(TAB_SLOT * 4.5), 13)
                click(PANEL_X + 126, 46)
                screenshot(env, shots / "notes-reloaded.png")
                click(PANEL_X + W_EXPANDED - 109, 83)        # Copy row 0
                time.sleep(0.6)
                got = subprocess.run([str(ROOT / "target/probe/release/clip-probe"), "get"], env=env, capture_output=True, text=True)
                assert got.returncode == 0, got.stderr
                assert got.stdout.strip() == "first quick note", got.stdout
                click(PANEL_X + W_EXPANDED - 59, 83)         # Edit row 0
                input_driver.focus(wid)
                click(PANEL_X + 110, 126)
                input_driver.key("a", ("Control_L",))
                input_driver.type_text("edited quick note")
                click(PANEL_X + W_EXPANDED - 137, 214)
                until(lambda: json.loads(store.read_text())[0]["text"] == "edited quick note")
                click(PANEL_X + W_EXPANDED - 22, 83)         # Remove
                until(lambda: json.loads(store.read_text()) == [])

                # Fixture data is confined to this test's private store.
                input_driver.focus(wid)
                input_driver.key("Escape")
                app.wait(timeout=5)
                fixtures = [{"id": index + 1, "text": f"note fixture {index}"} for index in range(6)]
                store.write_text(json.dumps(fixtures))
                app = subprocess.Popen([str(BIN), "--demo"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
                wid = until(lambda: geometry(env))[0]
                move(PILL_X + W_COLLAPSED // 2, 15)
                time.sleep(0.6)
                click(PANEL_X + int(TAB_SLOT * 4.5), 13)
                click(PANEL_X + 126, 46)
                click(PANEL_X + 177, 260)                    # Page two
                click(PANEL_X + W_EXPANDED - 109, 83)
                got = subprocess.run([str(ROOT / "target/probe/release/clip-probe"), "get"], env=env, capture_output=True, text=True)
                assert got.returncode == 0 and got.stdout.strip() == fixtures[-1]["text"], got.stderr
                click(PANEL_X + W_EXPANDED - 22, 83)         # Delete the last page's only note
                until(lambda: len(json.loads(store.read_text())) == 5)
                screenshot(env, shots / "notes-page-clamped.png")
                click(PANEL_X + W_EXPANDED - 109, 83)        # Must now copy page one's first note
                got = subprocess.run([str(ROOT / "target/probe/release/clip-probe"), "get"], env=env, capture_output=True, text=True)
                assert got.returncode == 0 and got.stdout.strip() == fixtures[0]["text"], "Removing the last item left Notes on an empty page"
                click(PANEL_X + W_EXPANDED - 55, 46)
                input_driver.focus(wid)
                click(PANEL_X + 110, 126)
                input_driver.type_text("discarded note 12345")
                input_driver.key("Escape")
                assert app.poll() is None, "Escape in the editor quit the app"
                assert len(json.loads(store.read_text())) == 5, "Cancel saved an unwanted note"
                click(PANEL_X + W_EXPANDED - 109, 83)
                got = subprocess.run([str(ROOT / "target/probe/release/clip-probe"), "get"], env=env, capture_output=True, text=True)
                assert got.returncode == 0 and got.stdout.strip() == fixtures[0]["text"], "Typing digits switched tabs out of Notes"
                print("G10_NOTES_PERSIST")
                return

            if mode == "keyboard":
                click(PANEL_X + int(TAB_SLOT * 1.5), 13)
                reference = signature(screenshot(env, shots / "keyboard-control-reference.png"), 70, 250)
                click(PANEL_X + int(TAB_SLOT * 0.5), 13)
                input_driver.focus(wid)
                input_driver.key("Tab")
                input_driver.key("Tab")
                input_driver.key("Return")
                after = screenshot(env, shots / "keyboard-control.png")
                # Animated fractional geometry changes a few antialias pixels.
                # The active tab underline is the actual selection oracle.
                accent = (122, 211, 255)
                underline = [sum(after.getpixel((x, y)) == accent
                    for x in range(PANEL_X + int(index * TAB_SLOT), PANEL_X + int((index + 1) * TAB_SLOT))
                    for y in range(22, 25)) for index in range(5)]
                assert underline[1] >= 40 and sum(underline) == underline[1], f"Control not selected: {underline}"
                focused = sum(after.getpixel((x, y)) == accent
                    for x in range(PANEL_X + int(TAB_SLOT), PANEL_X + int(TAB_SLOT * 2))
                    for y in range(1, 21))
                assert focused >= 50, f"Control tab has no visible focus ring: {focused}"
                print("G11_KEYBOARD_PASS")
                return

            if mode == "clips":
                probe_bin = ROOT / "target/probe/release/clip-probe"
                assert probe_bin.exists(), "build tests/clip_probe first"
                text = "glacier clipboard round trip 4711"
                owner = subprocess.Popen(
                    [str(probe_bin), "set", text, "25"],
                    env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                )
                try:
                    assert owner.stdout is not None
                    assert owner.stdout.readline().strip() == "CLAIMED", "probe could not own CLIPBOARD"
                    store = base / "data/glacier/clips.json"
                    until(lambda: store.exists() and text in store.read_text(), timeout=8)

                    click(PANEL_X + int(TAB_SLOT * 4.5), 13)   # Clips tab
                    clip_view = screenshot(env, shots / "clips.png")
                    ink = sum(
                        1
                        for x in range(PANEL_X + 18, PANEL_X + 250, 2)
                        for y in range(76, 96, 2)
                        if clip_view.getpixel((x, y)) != BG
                    )
                    assert ink > 40, f"clip row not painted (ink={ink})"

                    # Remove must suppress the unchanged external selection.
                    click(PANEL_X + W_EXPANDED - 22, 83)       # Remove row 0
                    until(lambda: store.exists() and json.loads(store.read_text()) == [], timeout=5)
                    time.sleep(1.5)  # More than two history polling intervals.
                    assert json.loads(store.read_text()) == [], "Remove re-imported the unchanged clipboard selection"

                    # Clear also suppresses an older selection, while a new
                    # external value resumes capture normally.
                    click(PANEL_X + W_EXPANDED - 49, 260)       # Clear all
                    time.sleep(0.3)
                finally:
                    owner.kill()
                    owner.wait(timeout=5)

                owner = subprocess.Popen(
                    [str(probe_bin), "set", text + " changed", "5"],
                    env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                )
                try:
                    assert owner.stdout is not None
                    assert owner.stdout.readline().strip() == "CLAIMED", "new selection owner failed"
                    until(lambda: store.exists() and any(
                        clip["text"] == text + " changed" for clip in json.loads(store.read_text())
                    ), timeout=8)
                finally:
                    owner.kill()
                    owner.wait(timeout=5)

                print("G8_CLIPS_DELETE_AND_RECAPTURE")
                return

            if mode == "controls":
                click(PANEL_X + int(TAB_SLOT * 1.5), 13)        # Control tab
                click(PANEL_X + 16 + (W_EXPANDED - 32) // 2, 109)   # midpoint is 50% volume
                click(PANEL_X + 16 + int((W_EXPANDED - 32) * 0.6), 181)  # brightness to 60%
                time.sleep(0.6)
                lines = stub_log.read_text().splitlines()
                volume = [line for line in lines if line.startswith("wpctl set-volume")]
                assert len(volume) == 1, volume
                assert abs(float(volume[0].split()[-1]) - 0.50) < 0.02, volume
                gamma = [line for line in lines if "--gamma" in line]
                assert len(gamma) == 1, gamma
                assert "HDMI-0" in gamma[0], gamma
                applied = float(gamma[0].split()[-1].split(":")[0])
                assert abs(applied - 0.60) < 0.01, gamma

                def volume_commands():
                    return [float(line.split()[-1]) for line in stub_log.read_text().splitlines()
                            if line.startswith("wpctl set-volume")]

                click(PANEL_X + W_EXPANDED - 17, 109)         # Inside the right endpoint: 100%, not 150%
                assert volume_commands()[-1] == 1.0, volume_commands()
                count = len(volume_commands())
                input_driver.wheel(PANEL_X + W_EXPANDED - 20, 109, up=True)
                assert len(volume_commands()) > count and volume_commands()[-1] == 1.0, volume_commands()

                input_driver.focus(wid)
                for attempt in range(9):
                    input_driver.key("Tab")
                    frame = screenshot(env, shots / "volume-keyboard-focus.png")
                    ring = sum(frame.getpixel((x, y)) == (122, 211, 255)
                        for x in range(PANEL_X + 30, PANEL_X + W_EXPANDED - 30)
                        for y in range(96, 99))
                    if ring > 100:
                        break
                else:
                    raise AssertionError("Could not focus the volume slider")
                count = len(volume_commands())
                input_driver.key("Right")
                assert len(volume_commands()) > count and volume_commands()[-1] == 1.0, volume_commands()
                input_driver.key("Left")
                assert abs(volume_commands()[-1] - 0.97) < 0.01, volume_commands()
                input_driver.key("Right")
                input_driver.key("Right")
                assert volume_commands()[-1] == 1.0 and all(0.0 <= value <= 1.0 for value in volume_commands()), volume_commands()
                print("G6_CONTROLS_APPLIED")
                return

            if mode == "focus":
                click(PANEL_X + int(TAB_SLOT * 2.5), 13)        # Focus tab
                before = signature(screenshot(env, shots / "focus-before.png"), 70, 135)
                click(PANEL_X + W_EXPANDED // 2 - 62, 212)      # Start chip
                time.sleep(1.4)
                after = signature(screenshot(env, shots / "focus-after.png"), 70, 135)
                assert before != after, "pomodoro clock did not tick after Start"
                print("G7_FOCUS_TICKS")
                return

            if mode == "interaction":
                assert move(480, 185) == wid, "expanded panel did not capture the pointer"
                assert move(120, 320) != wid, "mouse-away failed"
                time.sleep(0.6)
                assert move(480, 185) != wid, "transparent area blocked clicks after collapsing"
                assert_pill(screenshot(env, base / "recollapsed.png"), "recollapsed")
                print("G2_HOVER_SHAPE_PASS")
                return

            # Pointer back inside so every subsequent click lands on the open panel.
            move(PANEL_X + W_EXPANDED // 2, 60)
            if mode == "media":
                time.sleep(0.3)
                art_capture = screenshot(env, shots / "media-artwork.png")
                warm_pixels = sum(
                    1
                    for x in range(PANEL_X + 18, PANEL_X + 70)
                    for y in range(68, 120)
                    for pixel in [cast(tuple[int, int, int], art_capture.getpixel((x, y)))]
                    if pixel[0] > 150 and pixel[1] < 120 and pixel[2] < 120
                )
                assert warm_pixels > 300, f"MPRIS cover artwork not rendered (fixture red pixels={warm_pixels})"
                assert color_count(art_capture, (PANEL_X + 25, 139, PANEL_X + 95, 142), warm) > 90, "progress did not inherit warm cover color"
                assert color_count(art_capture, (PANEL_X + 15, 148, PANEL_X + 53, 162), warm) > 6, "elapsed time did not inherit warm cover color"
            views = []
            for index, name in enumerate(["Music", "Control", "Focus", "Shelf", "Clips"]):
                click(PANEL_X + int(TAB_SLOT * (index + 0.5)), 13)
                views.append((name, signature(screenshot(env, shots / f"tab-{name}.png"), 36, H_EXPANDED)))

            if mode == "tabs":
                assert len({sig for _, sig in views[1:]}) == 4, f"tabs render identically: {views}"
                print("G5_TABS_SWITCH_PASS")
                return

            # ── media actions ───────────────────────────────────────────────
            click(PANEL_X + int(TAB_SLOT * 0.5), 13)  # back to Music
            transport = PANEL_X + W_EXPANDED // 2
            click(PANEL_X + W_EXPANDED // 2, 179)              # active lyric line
            click(PANEL_X + 16 + (W_EXPANDED - 32) // 2, 140)  # scrub to the middle
            click(PANEL_X + 30, 155)                           # elapsed -> -5 s
            click(PANEL_X + W_EXPANDED - 30, 155)              # remaining -> +5 s
            click(PANEL_X + W_EXPANDED - 46, 246)              # speed badge
            click(transport - 44, 246)                         # previous
            click(transport, 246)                              # play/pause
            click(transport + 44, 246)                         # next

            calls = read_log(log)
            expected_calls = [
                "SetPosition:59000000",
                "SetPosition:90000000",
                "Seek:-5000000",
                "Seek:5000000",
                "SetRate:1.25",
                "Previous",
                "PlayPause",
                "Next",
            ]
            assert calls == expected_calls, calls
            def next_palette():
                frame = screenshot(env, shots / "media-artwork-next.png")
                return color_count(frame, (PANEL_X + 25, 139, PANEL_X + 95, 142), cool) > 90

            until(next_palette)
            print("G4_MEDIA_LYRICS_OK")
        finally:
            if input_driver is not None:
                input_driver.close()
            for proc in (app, probe, backdrop, xvfb):
                if proc is not None and proc.poll() is None:
                    proc.terminate()
                    try:
                        proc.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        proc.kill()
                        proc.wait(timeout=5)


if __name__ == "__main__":
    main()
