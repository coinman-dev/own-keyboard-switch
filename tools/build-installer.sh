#!/usr/bin/env bash
# Builds the Windows installer: the release executable first, then NSIS.
#
#   sudo apt install nsis            # once, for makensis
#   tools/build-installer.sh         # target/okbswitch-install.exe
#
# Pass --skip-build to package an executable that is already built, for
# example the one produced by tools/cross-windows.sh.
set -euo pipefail

root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

skip_build=0
if [[ "${1:-}" == "--skip-build" ]]; then
    skip_build=1
fi

if ! command -v makensis >/dev/null; then
    echo 'makensis not found; install NSIS: sudo apt install nsis' >&2
    exit 1
fi

version="$(sed -n 's/^version[[:space:]]*=[[:space:]]*"\(.*\)"/\1/p' Cargo.toml | head -1)"
if [[ -z "$version" ]]; then
    echo 'cannot read the version from Cargo.toml' >&2
    exit 1
fi
# VIProductVersion only accepts four numbers: "0.1.0-beta" becomes 0.1.0.0.
numeric="$(printf '%s' "$version" | sed 's/[-+].*//')"
while [[ "$(tr -cd . <<<"$numeric" | wc -c)" -lt 3 ]]; do
    numeric="$numeric.0"
done

target_dir="${CARGO_TARGET_DIR:-$root/target}"
exe="${SOURCE_EXE:-$target_dir/x86_64-pc-windows-gnullvm/release/okbswitch.exe}"
if [[ "$skip_build" -eq 0 ]]; then
    # Through bash: a checkout copied from Windows loses the execute bit.
    bash "$root/tools/cross-windows.sh" build --release -p okbswitch
fi
if [[ ! -f "$exe" ]]; then
    echo "executable not found: $exe" >&2
    exit 1
fi

output="${OUTPUT:-$target_dir/okbswitch-install.exe}"
makensis -V2 \
    "-DVERSION=$version" \
    "-DVERSION_NUMERIC=$numeric" \
    "-DSOURCE_EXE=$exe" \
    "-DROOT=$root" \
    "-DOUTPUT=$output" \
    "$root/packaging/windows/okbswitch.nsi"

echo "installer: $output"
ls -l "$output"
