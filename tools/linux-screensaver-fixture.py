#!/usr/bin/env python3
"""An unlocked org.freedesktop.ScreenSaver for an isolated test session bus.

A headless KWin has no screen locker. Without this service the program
treats the session as locked and does nothing, as it must on a real desktop
whose lock state is unknown. Only for private test buses.
"""
from gi.repository import Gio, GLib

XML = """
<node>
  <interface name="org.freedesktop.ScreenSaver">
    <method name="GetActive"><arg type="b" direction="out"/></method>
    <signal name="ActiveChanged"><arg type="b"/></signal>
  </interface>
</node>
"""


def call(connection, sender, path, interface, method, parameters, invocation):
    if method == "GetActive":
        invocation.return_value(GLib.Variant("(b)", (False,)))
    else:
        invocation.return_dbus_error("org.freedesktop.DBus.Error.UnknownMethod", method)


def acquired(connection, name):
    info = Gio.DBusNodeInfo.new_for_xml(XML).interfaces[0]
    for path in ["/ScreenSaver", "/org/freedesktop/ScreenSaver"]:
        connection.register_object(path, info, call, None, None)


Gio.bus_own_name(
    Gio.BusType.SESSION,
    "org.freedesktop.ScreenSaver",
    Gio.BusNameOwnerFlags.NONE,
    acquired,
    None,
    lambda *_: loop.quit(),
)
loop = GLib.MainLoop()
loop.run()
