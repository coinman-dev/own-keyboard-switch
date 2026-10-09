#!/usr/bin/env python3
"""Visible Writer documents in a private profile; UNO observes only synthetic text."""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
import uno
from com.sun.star.beans import PropertyValue
from com.sun.star.connection import NoConnectException

state, command = map(Path, sys.argv[1:3])
scratch = state.parent
profile = scratch / "writer-profile"
profile.mkdir()
pipe = f"okbs_writer_fixture_{os.getpid()}"
environment = dict(os.environ, SAL_USE_VCLPLUGIN="gtk3", GDK_BACKEND="wayland", SAL_ACCESSIBILITY_ENABLED="1")
# The private KWin service cache is for desktop-file permission lookup. Glycin
# in this WSL distribution cannot use that nested cache mount; Writer's image
# loaders use the ordinary user cache while its profile/documents stay here.
environment.pop("XDG_CACHE_HOME", None)
process = subprocess.Popen([
    "libreoffice", "--nologo", "--norestore",
    "--nodefault", "--nofirststartwizard", f"-env:UserInstallation={profile.as_uri()}",
    f"--accept=pipe,name={pipe};urp;StarOffice.ServiceManager",
], env=environment)
documents = {}

def stop(*_):
    raise SystemExit
signal.signal(signal.SIGTERM, stop)
signal.signal(signal.SIGINT, stop)

try:
    context = uno.getComponentContext()
    resolver = context.ServiceManager.createInstanceWithContext(
        "com.sun.star.bridge.UnoUrlResolver", context)
    deadline = time.monotonic() + 20
    while True:
        try:
            remote = resolver.resolve(f"uno:pipe,name={pipe};urp;StarOffice.ComponentContext")
            break
        except NoConnectException:
            if time.monotonic() >= deadline or process.poll() is not None:
                raise RuntimeError("Private Writer UNO service unavailable")
            time.sleep(0.1)
    desktop = remote.ServiceManager.createInstanceWithContext("com.sun.star.frame.Desktop", remote)
    for name in ("first", "second"):
        document = desktop.loadComponentFromURL("private:factory/swriter", "_blank", 0, ())
        overwrite = PropertyValue()
        overwrite.Name, overwrite.Value = "Overwrite", True
        document.storeAsURL((scratch / f"okbs-{name}.odt").as_uri(), (overwrite,))
        documents[name] = document
    # The distribution launcher handles first-profile bootstrap restarts.
    # Its child owns the actual window; identify it only within our process tree.
    candidates = [process.pid]
    native_pid = None
    while candidates:
        candidate = candidates.pop()
        proc = Path(f"/proc/{candidate}")
        try:
            if (proc / "exe").resolve().name == "soffice.bin":
                native_pid = candidate
                break
            candidates.extend(map(int, (proc / f"task/{candidate}/children").read_text().split()))
        except FileNotFoundError:
            pass
    if native_pid is None:
        # oosplash can reap/reparent a bootstrap restart. Match only the exact
        # private UserInstallation argument, never an unrelated office profile.
        expected = f"-env:UserInstallation={profile.as_uri()}".encode()
        for proc in Path("/proc").glob("[0-9]*"):
            try:
                if (proc / "exe").resolve().name == "soffice.bin" and expected in (proc / "cmdline").read_bytes().split(b"\0"):
                    native_pid = int(proc.name)
                    break
            except (FileNotFoundError, PermissionError):
                pass
        if native_pid is None:
            raise RuntimeError("Private Writer window process unavailable")
    active, acknowledgement = "first", None
    documents[active].CurrentController.Frame.activate()
    while True:
        if command.exists():
            action = json.loads(command.read_text())
            command.unlink()
            active = action["document"]
            document = documents[active]
            if "text" in action:
                document.Text.String = action["text"]
            cursor = document.CurrentController.ViewCursor
            if action.get("select_all"):
                cursor.gotoStart(False)
                cursor.gotoEnd(True)
            else:
                cursor.gotoEnd(False)
            document.CurrentController.Frame.activate()
            document.CurrentController.Frame.ContainerWindow.setFocus()
            acknowledgement = action["token"]
        value = {"pid": native_pid, "active": active, "acknowledgement": acknowledgement,
                 "titles": {name: document.CurrentController.Frame.Title for name, document in documents.items()},
                 "values": {name: document.Text.String for name, document in documents.items()}}
        temporary = state.with_suffix(".new")
        temporary.write_text(json.dumps(value))
        temporary.replace(state)
        time.sleep(0.05)
finally:
    for document in documents.values():
        try:
            document.setModified(False)
            document.close(True)
        except Exception:
            pass
    if "desktop" in locals():
        try:
            desktop.terminate()
        except Exception:
            pass
    process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()
