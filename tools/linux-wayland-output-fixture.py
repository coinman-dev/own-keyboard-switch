#!/usr/bin/env python3
"""Test-only bridge: owned evdev output to a private Wayland compositor."""
import os
from pathlib import Path
import select
import signal
import struct
import subprocess
import sys
import time
from gi.repository import Gio, GLib

device, ready = map(Path, sys.argv[1:3])
native = None
remote = None
def stop(*_):
    raise SystemExit
signal.signal(signal.SIGTERM, stop)
try:
    if os.environ["XDG_CURRENT_DESKTOP"] == "KDE":
        native = subprocess.Popen([str(Path(__file__).resolve().parent.parent / "tmp/linux-kde-keyboard/keyboard-fixture")], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        if native.stdout.readline().strip() != "ready":
            raise RuntimeError("Private KDE keyboard unavailable")
    else:
        bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
        manager = Gio.DBusProxy.new_sync(bus, 0, None, "org.gnome.Mutter.RemoteDesktop", "/org/gnome/Mutter/RemoteDesktop", "org.gnome.Mutter.RemoteDesktop", None)
        path = manager.call_sync("CreateSession", None, 0, 3000, None).unpack()[0]
        remote = Gio.DBusProxy.new_sync(bus, 0, None, "org.gnome.Mutter.RemoteDesktop", path, "org.gnome.Mutter.RemoteDesktop.Session", None)
        remote.call_sync("Start", None, 0, 3000, None)
        for pressed in (True, False):
            remote.call_sync("NotifyKeyboardKeycode", GLib.Variant("(ub)", (42, pressed)), 0, 3000, None)
    event = struct.Struct("llHHi")
    with device.open("rb", buffering=0) as stream:
        ready.write_text("ready\n")
        while True:
            if not select.select([stream], [], [], 0.05)[0]:
                ready.with_suffix(".idle").write_text(str(time.monotonic()))
                continue
            block = stream.read(event.size)
            if len(block) != event.size:
                raise RuntimeError("Owned evdev stream closed")
            _, _, kind, code, value = event.unpack(block)
            if kind != 1:
                continue
            if remote:
                remote.call_sync("NotifyKeyboardKeycode", GLib.Variant("(ub)", (code, value != 0)), 0, 3000, None)
            else:
                request = f"{code} {int(value != 0)}"
                native.stdin.write(request + "\n")
                native.stdin.flush()
                if native.stdout.readline().strip() != request:
                    raise RuntimeError("KDE output delivery failed")
finally:
    if remote:
        try: remote.call_sync("Stop", None, 0, 3000, None)
        except GLib.Error: pass
    if native:
        native.terminate()
        native.wait(timeout=5)
