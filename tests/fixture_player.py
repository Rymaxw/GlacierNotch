#!/usr/bin/python3
"""MPRIS test player for Glacier integration checks.

Exposes the properties Glacier reads and records every transport call to
$GLACIER_PROBE_LOG so a test can assert which method was actually invoked.
"""
import os

import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

DBusGMainLoop(set_as_default=True)
bus = dbus.SessionBus()
name = dbus.service.BusName("org.mpris.MediaPlayer2.GlacierProbe", bus=bus, do_not_queue=True)

LENGTH_US = 180_000_000
TRACK_ID = "/org/glacier/probe/track/1"


class Player(dbus.service.Object):
    def __init__(self):
        super().__init__(bus, "/org/mpris/MediaPlayer2")
        self.status = "Playing"
        self.position_us = 60_000_000
        self.rate = 1.0
        self.cover = "artwork-fixture.png"
        self.title = "Real D-Bus Test Track"
        self.track_id = TRACK_ID

    @dbus.service.method("org.freedesktop.DBus.Properties", in_signature="ss", out_signature="v")
    def Get(self, iface, prop):
        return self.GetAll(iface)[prop]

    @dbus.service.method("org.freedesktop.DBus.Properties", in_signature="s", out_signature="a{sv}")
    def GetAll(self, iface):
        if iface == "org.mpris.MediaPlayer2":
            return dbus.Dictionary({"Identity": dbus.String("Glacier Probe")}, signature="sv")
        return dbus.Dictionary(
            {
                "PlaybackStatus": dbus.String(self.status),
                "Position": dbus.Int64(self.position_us),
                "Rate": dbus.Double(self.rate),
                "CanGoNext": dbus.Boolean(True),
                "CanGoPrevious": dbus.Boolean(True),
                "CanSeek": dbus.Boolean(True),
                "CanControl": dbus.Boolean(True),
                "MinimumRate": dbus.Double(0.25),
                "MaximumRate": dbus.Double(4.0),
                "Metadata": dbus.Dictionary(
                    {
                        "mpris:trackid": dbus.ObjectPath(self.track_id),
                        "mpris:length": dbus.Int64(LENGTH_US),
                        "mpris:artUrl": dbus.String(os.path.join(os.environ["XDG_DATA_HOME"], "glacier", self.cover)),
                        "xesam:title": dbus.String(self.title),
                        "xesam:artist": dbus.Array([dbus.String("Test Artist")], signature="s"),
                        "xesam:album": dbus.String("Test Album"),
                    },
                    signature="sv",
                ),
            },
            signature="sv",
        )

    @dbus.service.method("org.freedesktop.DBus.Properties", in_signature="ssv", out_signature="")
    def Set(self, iface, prop, value):
        if prop == "Rate":
            self.rate = float(value)
            self.hit(f"SetRate:{self.rate:.2f}")

    def hit(self, method):
        with open(os.environ["GLACIER_PROBE_LOG"], "a", encoding="utf-8") as output:
            output.write(method + "\n")
            output.flush()

    @dbus.service.method("org.mpris.MediaPlayer2.Player", in_signature="", out_signature="")
    def Previous(self):
        self.hit("Previous")

    @dbus.service.method("org.mpris.MediaPlayer2.Player", in_signature="", out_signature="")
    def PlayPause(self):
        self.hit("PlayPause")
        self.status = "Paused" if self.status == "Playing" else "Playing"

    @dbus.service.method("org.mpris.MediaPlayer2.Player", in_signature="", out_signature="")
    def Next(self):
        self.hit("Next")
        self.cover = "artwork-fixture-next.png"
        self.title = "Second D-Bus Test Track"
        self.track_id = "/org/glacier/probe/track/2"
        self.status = "Playing"

    @dbus.service.method("org.mpris.MediaPlayer2.Player", in_signature="x", out_signature="")
    def Seek(self, offset_us):
        self.hit(f"Seek:{int(offset_us)}")
        self.position_us = max(0, min(LENGTH_US, self.position_us + int(offset_us)))

    @dbus.service.method("org.mpris.MediaPlayer2.Player", in_signature="ox", out_signature="")
    def SetPosition(self, track_id, position_us):
        if str(track_id) != self.track_id:
            raise dbus.exceptions.DBusException("Wrong track id")
        self.hit(f"SetPosition:{int(position_us)}")
        self.position_us = max(0, min(LENGTH_US, int(position_us)))


Player()
GLib.MainLoop().run()
