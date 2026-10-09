#!/usr/bin/env python3
"""Keep keyboard capability alive throughout one private headless Mutter session."""
import signal
import sys
from pathlib import Path
from gi.repository import Gio, GLib

ready=Path(sys.argv[1])
bus=Gio.bus_get_sync(Gio.BusType.SESSION,None)
manager=Gio.DBusProxy.new_sync(bus,0,None,"org.gnome.Mutter.RemoteDesktop",
    "/org/gnome/Mutter/RemoteDesktop","org.gnome.Mutter.RemoteDesktop",None)
path=manager.call_sync("CreateSession",None,0,3000,None).unpack()[0]
session=Gio.DBusProxy.new_sync(bus,0,None,"org.gnome.Mutter.RemoteDesktop",path,
    "org.gnome.Mutter.RemoteDesktop.Session",None)
def stop(*_): raise SystemExit
signal.signal(signal.SIGTERM,stop)
signal.signal(signal.SIGINT,stop)
try:
    session.call_sync("Start",None,0,3000,None)
    for pressed in (True,False):
        session.call_sync("NotifyKeyboardKeycode",GLib.Variant("(ub)",(42,pressed)),0,3000,None)
    ready.write_text("ready\n")
    while True: signal.pause()
finally:
    try: session.call_sync("Stop",None,0,3000,None)
    except GLib.Error: pass
