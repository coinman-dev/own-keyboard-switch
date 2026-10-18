#!/usr/bin/env bash
# Builds the Linux release files locally (GitHub minutes are not used):
# the executable in an isolated Ubuntu 24.04 root, then .deb and AppImage.
#
#   sudo apt install debootstrap squashfs-tools   # once
#   sudo tools/build-linux-release.sh             # target/linux-dist
#
# The AppImage runtime must be tmp/appimage-runtime/runtime-continuous
# (pinned by SHA-256 in package-linux.py).
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ $EUID != 0 ]]; then echo 'Run as root: sudo tools/build-linux-release.sh' >&2; exit 1; fi
owner=$(stat -c %u:%g Cargo.toml)
bash tools/build-linux-baseline.sh
rm -rf target/linux-dist
python3 tools/package-linux.py \
    --exe target/linux-baseline/release/okbswitch \
    --sysroot tmp/linux-build-noble \
    --runtime tmp/appimage-runtime/runtime-continuous
chown -R "$owner" target/linux-baseline target/linux-dist
ls -l target/linux-dist
