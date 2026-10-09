#!/usr/bin/env python3
"""Deliver real picker confirmation keys to one private Mutter session."""
from pathlib import Path
import signal
import sys
import time
import json
from gi.repository import Gio, GLib
command, ready = map(Path, sys.argv[1:3])
bus=Gio.bus_get_sync(Gio.BusType.SESSION,None)
manager=Gio.DBusProxy.new_sync(bus,0,None,"org.gnome.Mutter.RemoteDesktop","/org/gnome/Mutter/RemoteDesktop","org.gnome.Mutter.RemoteDesktop",None)
path=manager.call_sync("CreateSession",None,0,3000,None).unpack()[0]
remote=Gio.DBusProxy.new_sync(bus,0,None,"org.gnome.Mutter.RemoteDesktop",path,"org.gnome.Mutter.RemoteDesktop.Session",None)
def stop(*_): raise SystemExit
signal.signal(signal.SIGTERM,stop)
try:
    remote.call_sync("Start",None,0,3000,None)
    for pressed in (True,False): remote.call_sync("NotifyKeyboardKeycode",GLib.Variant("(ub)",(42,pressed)),0,3000,None)
    ready.write_text("ready\n")
    while True:
        if command.exists():
            action=json.loads(command.read_text())
            if isinstance(action, int):
                for pressed in (True,False): remote.call_sync("NotifyKeyboardKeycode",GLib.Variant("(ub)",(action,pressed)),0,3000,None)
            else:
                # A new virtual pointer may not share the previously active
                # device's position. Promote it with a real motion first.
                remote.call_sync("NotifyPointerMotionRelative",GLib.Variant("(dd)",(100.0,100.0)),0,3000,None)
                integration=Gio.DBusProxy.new_sync(bus,0,None,"org.own_keyboard_switch.Gnome","/org/own_keyboard_switch/Gnome","org.own_keyboard_switch.Gnome",None)
                x,y=action["click"]
                for _ in range(8):
                    time.sleep(0.04)
                    position=json.loads(integration.call_sync("GetState",None,0,3000,None).unpack()[0])["cursor"]
                    if abs(position[0]-x)<2 and abs(position[1]-y)<2: break
                    remote.call_sync("NotifyPointerMotionRelative",GLib.Variant("(dd)",(float(x-position[0]),float(y-position[1]))),0,3000,None)
                else: raise RuntimeError("Private Mutter pointer did not reach the requested row")
                # A corrective move can cross the private headless Shell's
                # hot corner before the new device's position is known.
                shell=Gio.DBusProxy.new_sync(bus,0,None,"org.gnome.Shell","/org/gnome/Shell","org.freedesktop.DBus.Properties",None)
                shell.call_sync("Set",GLib.Variant("(ssv)",("org.gnome.Shell","OverviewActive",GLib.Variant("b",False))),0,3000,None)
                time.sleep(0.3)
                for pressed in (True,False): remote.call_sync("NotifyPointerButton",GLib.Variant("(ib)",(272,pressed)),0,3000,None)
            command.unlink()
        time.sleep(0.01)
finally:
    try: remote.call_sync("Stop",None,0,3000,None)
    except GLib.Error: pass
