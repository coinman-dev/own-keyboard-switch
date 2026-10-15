#!/usr/bin/env bash
# Explicit local build in Ubuntu 24.04. Keep host/newer glibc out of artifacts.
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ $EUID != 0 ]]; then echo 'Run this isolated build helper as root.' >&2; exit 1; fi
linux_workspace=$PWD
linux_owner_home=$(getent passwd "$(stat -c %u "$linux_workspace/Cargo.toml")" | cut -d: -f6)
linux_cargo_cache=${OKBS_BUILD_CARGO_HOME:-$linux_owner_home/.cargo}
linux_rustup_cache=${OKBS_BUILD_RUSTUP_HOME:-$linux_owner_home/.rustup}
linux_root="$linux_workspace/tmp/linux-build-noble"
if [[ ! -f "$linux_root/etc/os-release" ]]; then
    debootstrap --variant=minbase noble "$linux_root" http://archive.ubuntu.com/ubuntu
fi
mkdir -p "$linux_root/build" "$linux_root/opt/cargo" "$linux_root/opt/rustup" "$linux_root/proc"
mount --bind "$linux_workspace" "$linux_root/build"
mount --bind "$linux_cargo_cache" "$linux_root/opt/cargo"
mount --bind "$linux_rustup_cache" "$linux_root/opt/rustup"
mount -t proc proc "$linux_root/proc"
finish() {
    umount "$linux_root/proc" || true
    umount "$linux_root/opt/rustup" || true
    umount "$linux_root/opt/cargo" || true
    umount "$linux_root/build" || true
}
trap finish EXIT
cat >"$linux_root/etc/apt/sources.list" <<'SOURCES'
deb http://archive.ubuntu.com/ubuntu noble main universe
deb http://archive.ubuntu.com/ubuntu noble-updates main universe
deb http://security.ubuntu.com/ubuntu noble-security main universe
SOURCES
cp -L /etc/resolv.conf "$linux_root/etc/resolv.conf"
chroot "$linux_root" /bin/bash -c 'apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq build-essential pkg-config ca-certificates libgtk-3-dev libgtk-layer-shell-dev libxkbcommon-dev libx11-dev libxcb1-dev libssl-dev wl-clipboard zenity'
chroot "$linux_root" /usr/bin/env PATH=/opt/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin CARGO_HOME=/opt/cargo RUSTUP_HOME=/opt/rustup CARGO_TARGET_DIR=/build/target/linux-baseline /bin/bash -c 'cd /build; cargo build --release --locked -p okbswitch'
