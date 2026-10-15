#!/usr/bin/env bash
# Read-only package smoke checks. Installation tests run in a separate sysroot.
set -euo pipefail
cd "$(dirname "$0")/.."
linux_dist="$PWD/target/linux-dist"
linux_version=$(sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)"/\1/p' Cargo.toml | head -1)
linux_image="$linux_dist/okbswitch-$linux_version-x86_64.AppImage"
linux_deb_version=${linux_version/-/\~}
linux_deb="$linux_dist/okbswitch_${linux_deb_version}_amd64.deb"
(cd "$linux_dist" && sha256sum -c SHA256SUMS.txt)
APPIMAGE_EXTRACT_AND_RUN=1 "$linux_image" --version
linux_paths=$(APPIMAGE_EXTRACT_AND_RUN=1 "$linux_image" --paths)
[[ "$linux_paths" == *"$linux_dist/data/config.toml"* ]]
[[ "$linux_paths" != *".mount"* ]]
APPIMAGE_EXTRACT_AND_RUN=1 "$linux_image" --licenses >tmp/linux-package-licenses.log
grep -q 'gtk 0.19.0' tmp/linux-package-licenses.log
grep -q 'wl-clipboard-rs 0.9.4' tmp/linux-package-licenses.log
grep -q 'PolyForm' tmp/linux-package-licenses.log
[[ "$(dpkg-deb --field "$linux_deb" Package)" == okbswitch ]]
[[ "$(dpkg-deb --field "$linux_deb" Architecture)" == amd64 ]]
dpkg-deb --contents "$linux_deb" >tmp/linux-package-contents.log
grep -q 'usr/lib/udev/rules.d/70-okbswitch.rules' tmp/linux-package-contents.log
grep -q 'usr/share/okbswitch/integrations/gnome/extension.js' tmp/linux-package-contents.log
echo 'Linux package smoke checks passed.'
