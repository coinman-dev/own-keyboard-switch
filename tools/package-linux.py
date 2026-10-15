#!/usr/bin/env python3
"""Package a reviewed Linux executable into .deb and a type-2 AppImage.

Use --sysroot for Ubuntu 24.04 libraries; --runtime must match the pinned digest.
No installation, release tag or publication is performed by this command.
"""
import argparse
import hashlib
import os
from pathlib import Path
import re
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]
RUNTIME_SHA256 = "156f4bdbde9c52d01814600013e0a273f0118dc2de98975f3c8c63427ec79074"

def run(*args):
    subprocess.run([str(arg) for arg in args], check=True)

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--exe", type=Path, required=True)
    parser.add_argument("--sysroot", type=Path)
    parser.add_argument("--runtime", type=Path, required=True)
    args = parser.parse_args()
    if hashlib.sha256(args.runtime.read_bytes()).hexdigest() != RUNTIME_SHA256:
        raise SystemExit("AppImage runtime checksum mismatch")
    version = re.search(r'^version\s*=\s*"([^"]+)"', (ROOT / "Cargo.toml").read_text(), re.M)[1]
    actual = subprocess.check_output([str(args.exe.resolve()), "--version"], text=True).strip()
    if actual != f"okbswitch {version}":
        raise SystemExit("Executable and manifest versions differ")
    output = ROOT / "target/linux-dist"
    output.mkdir(parents=True, exist_ok=True)
    # Unique staging directories avoid deleting an existing build or user data.
    import tempfile
    with tempfile.TemporaryDirectory(prefix="linux-package-", dir=ROOT / "target") as temporary:
        stage = Path(temporary)
        deb = stage / "deb"
        app = stage / "AppDir"
        for directory in [deb / "DEBIAN", deb / "usr/bin", app / "usr/bin", app / "usr/lib"]:
            directory.mkdir(parents=True)
        for dest in [deb / "usr/bin/okbswitch", app / "usr/bin/okbswitch"]:
            shutil.copy2(args.exe, dest)
            dest.chmod(0o755)
        desktop = "[Desktop Entry]\nType=Application\nName=Own Keyboard Switch\nExec=okbswitch\nIcon=okbswitch\nTerminal=false\nCategories=Utility;Accessibility;\n"
        for tree in [deb, app]:
            icons = tree / "usr/share/icons/hicolor/256x256/apps"
            icons.mkdir(parents=True)
            shutil.copy2(ROOT / "images/logo-settings.png", icons / "okbswitch.png")
            applications = tree / "usr/share/applications"
            applications.mkdir(parents=True)
            (applications / "okbswitch.desktop").write_text(desktop)
            docs = tree / "usr/share/doc/okbswitch"
            docs.mkdir(parents=True)
            for name in ["LICENSE", "NOTICE", "THIRD-PARTY-NOTICES.txt"]:
                shutil.copy2(ROOT / name, docs / name)
            integrations = tree / "usr/share/okbswitch/integrations"
            integrations.mkdir(parents=True)
            for name in ["gnome", "kde"]:
                shutil.copytree(ROOT / "packaging/linux" / name, integrations / name)
        rules = deb / "usr/lib/udev/rules.d"
        rules.mkdir(parents=True)
        shutil.copy2(ROOT / "packaging/linux/70-okbswitch.rules", rules / "70-okbswitch.rules")
        modules = deb / "usr/lib/modules-load.d"
        modules.mkdir(parents=True)
        (modules / "okbswitch.conf").write_text("evdev\nuinput\n")
        (deb / "DEBIAN/control").write_text(f"Package: okbswitch\nVersion: {version.replace('-', '~', 1)}\nArchitecture: amd64\nMaintainer: coinman.dev\nSection: utils\nPriority: optional\nDepends: libc6 (>= 2.39), libgtk-3-0t64, libgtk-layer-shell0, libxkbcommon0, libx11-6, libxcb1, libgl1, libegl1, pkexec, udev, wl-clipboard, zenity\nRecommends: pulseaudio-utils | pipewire-bin\nDescription: Own Keyboard Switch\n Automatic Russian and English keyboard layout switching.\n")
        post = "#!/bin/sh\nset -e\n# Offline image construction has no running udev daemon.\nif [ -S /run/udev/control ]; then\n /usr/sbin/modprobe -a evdev uinput\n /usr/bin/udevadm control --reload-rules\n /usr/bin/udevadm trigger --subsystem-match=input\n /usr/bin/udevadm trigger --subsystem-match=misc --sysname-match=uinput\n /usr/bin/udevadm settle --timeout=5\nfi\n"
        (deb / "DEBIAN/postinst").write_text(post)
        (deb / "DEBIAN/postinst").chmod(0o755)
        (deb / "DEBIAN/postrm").write_text("#!/bin/sh\nset -e\nif [ -S /run/udev/control ]; then /usr/bin/udevadm control --reload-rules; fi\n")
        (deb / "DEBIAN/postrm").chmod(0o755)
        run("dpkg-deb", "--root-owner-group", "--build", deb, output / f"okbswitch_{version.replace('-', '~', 1)}_amd64.deb")
        shutil.copy2(app / "usr/share/icons/hicolor/256x256/apps/okbswitch.png", app / "okbswitch.png")
        (app / "okbswitch.desktop").write_text(desktop)
        (app / "AppRun").write_text("#!/bin/sh\nset -e\nAPPDIR=$(CDPATH= cd -- \"$(dirname -- \"$0\")\" && pwd)\nexport LD_LIBRARY_PATH=\"$APPDIR/usr/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}\"\nexport PATH=\"$APPDIR/usr/bin:$PATH\"\nexport XDG_DATA_DIRS=\"$APPDIR/usr/share:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}\"\nexec \"$APPDIR/usr/bin/okbswitch\" \"$@\"\n")
        (app / "AppRun").chmod(0o755)
        sysroot = args.sysroot.resolve() if args.sysroot else Path("/")
        def libraries(binary):
            if args.sysroot:
                with tempfile.TemporaryDirectory(prefix="okbs-library-probe-",dir=sysroot / "tmp") as probe:
                    dest = Path(probe) / "okbswitch"
                    shutil.copy2(binary,dest)
                    listing = subprocess.check_output(["chroot",str(sysroot),"ldd","/" + str(dest.relative_to(sysroot))],text=True)
            else:
                listing = subprocess.check_output(["ldd", str(binary)], text=True)
            return re.findall(r'=>\s+(/\S+)', listing)
        bundled = set()
        for path in libraries(args.exe):
            name = Path(path).name
            if name.startswith(("libc.so", "libm.so", "libpthread.so", "libdl.so", "librt.so", "libresolv.so", "libGL", "libEGL", "libOpenGL")):
                continue
            shutil.copy2(sysroot / path.lstrip("/"), app / "usr/lib" / name)
            bundled.add(path)
        # Keep command helpers in the portable file. Resolve their libraries in
        # the same build sysroot, rather than silently copying newer host libraries.
        for helper in ["wl-copy", "wl-paste", "zenity"]:
            binary = sysroot / "usr/bin" / helper
            if not binary.exists():
                raise SystemExit(f"Build sysroot is missing {helper}")
            shutil.copy2(binary, app / "usr/bin" / helper)
            listing = subprocess.check_output(["chroot",str(sysroot),"ldd",f"/usr/bin/{helper}"],text=True) if args.sysroot else subprocess.check_output(["ldd",str(binary)],text=True)
            for path in re.findall(r'=>\s+(/\S+)',listing):
                name = Path(path).name
                if name.startswith(("libc.so","libm.so","libpthread.so","libdl.so","librt.so","libresolv.so","libGL","libEGL","libOpenGL")): continue
                shutil.copy2(sysroot / path.lstrip("/"), app / "usr/lib" / name)
                bundled.add(path)
        # Distribute the distro's copyright notices and source-package versions
        # for every bundled system library as well as the Rust dependency notices.
        system_docs = app / "usr/share/doc/okbswitch/system-libraries"
        system_docs.mkdir(parents=True)
        packages = set()
        for path in bundled:
            absolute = sysroot / path.lstrip("/")
            resolved = "/" + str(absolute.resolve().relative_to(sysroot)) if args.sysroot else str(absolute.resolve())
            # usrmerge may retain /lib paths in dpkg's database although real
            # files resolve below /usr/lib. Match the exact library basename.
            pattern = "*/" + Path(resolved).name
            command = ["chroot",str(sysroot),"dpkg-query","-S",pattern] if args.sysroot else ["dpkg-query","-S",pattern]
            found = subprocess.check_output(command,text=True).strip().splitlines()
            for record in found: packages.add(record.split(": /",1)[0].split(":",1)[0])
        records = []
        for package in sorted(packages):
            source = sysroot / "usr/share/doc" / package / "copyright"
            if not source.exists(): raise SystemExit(f"Missing copyright notice for {package}")
            target = system_docs / package
            target.mkdir()
            shutil.copy2(source,target / "copyright")
            command = ["chroot",str(sysroot),"dpkg-query","-W","-f=${Package}\t${Version}\t${Source}\n",package] if args.sysroot else ["dpkg-query","-W","-f=${Package}\t${Version}\t${Source}\n",package]
            records.append(subprocess.check_output(command,text=True))
        (system_docs / "SOURCE-PACKAGES.tsv").write_text("".join(records))
        shutil.copytree(sysroot / "usr/share/common-licenses",system_docs / "common-licenses")
        schemas = sysroot / "usr/share/glib-2.0/schemas"
        if schemas.exists(): shutil.copytree(schemas, app / "usr/share/glib-2.0/schemas")
        xkb = sysroot / "usr/share/X11/xkb"
        if xkb.exists(): shutil.copytree(xkb, app / "usr/share/X11/xkb")
        (app / "AppRun").write_text((app / "AppRun").read_text().replace('export PATH=', 'export XKB_CONFIG_ROOT="$APPDIR/usr/share/X11/xkb"\nexport PATH='))
        squash = stage / "image.squashfs"
        run("mksquashfs", app, squash, "-noappend", "-all-root", "-comp", "zstd", "-quiet")
        image = output / f"okbswitch-{version}-x86_64.AppImage"
        with image.open("wb") as file:
            file.write(args.runtime.read_bytes())
            file.write(squash.read_bytes())
        image.chmod(0o755)
        assets = sorted(output.glob("*.deb")) + sorted(output.glob("*.AppImage"))
        (output / "SHA256SUMS.txt").write_text("".join(f"{hashlib.sha256(file.read_bytes()).hexdigest()}  {file.name}\n" for file in assets))
        print(output)

if __name__ == "__main__": main()
