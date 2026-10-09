#!/usr/bin/env python3
"""Fetch the official Firefox test binary into ignored workspace scratch."""
import hashlib
import json
import re
from pathlib import Path
import tarfile
import urllib.request

root = Path(__file__).resolve().parents[1]
scratch = root / "tmp/linux-browser"
scratch.mkdir(parents=True, exist_ok=True)
with urllib.request.urlopen("https://product-details.mozilla.org/1.0/firefox_versions.json", timeout=30) as response:
    version = json.load(response)["LATEST_FIREFOX_VERSION"]
if not re.fullmatch(r"\d+(?:\.\d+){1,2}", version):
    raise SystemExit("Unexpected stable Firefox version")
directory = scratch / version
directory.mkdir(exist_ok=True)
name = f"linux-x86_64/en-US/firefox-{version}.tar.xz"
base = f"https://archive.mozilla.org/pub/firefox/releases/{version}/"
with urllib.request.urlopen(base + "SHA256SUMS", timeout=30) as response:
    lines = response.read().decode().splitlines()
expected = next(line.split()[0] for line in lines if line.split()[-1] == name)
archive = directory / "firefox.tar.xz"
if not archive.exists() or hashlib.sha256(archive.read_bytes()).hexdigest() != expected:
    with urllib.request.urlopen(base + name, timeout=60) as response, archive.open("wb") as target:
        while block := response.read(1024 * 1024):
            target.write(block)
if hashlib.sha256(archive.read_bytes()).hexdigest() != expected:
    raise SystemExit("Firefox archive SHA256 does not match Mozilla's manifest")
if not (directory / "firefox/firefox").exists():
    with tarfile.open(archive) as package:
        package.extractall(directory, filter="data")
(scratch / "current.txt").write_text(str(directory / "firefox/firefox") + "\n")
print(f"Firefox {version}: Mozilla SHA256 verified")
