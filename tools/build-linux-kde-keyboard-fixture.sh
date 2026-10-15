#!/usr/bin/env bash
# Development-only: system protocol declarations and a tiny test driver.
set -euo pipefail
cd "$(dirname "$0")/.."
linux_protocol=/usr/share/plasma-wayland-protocols/fake-input.xml
test -f "$linux_protocol" || { echo 'Install plasma-wayland-protocols and libwayland-dev for KDE input acceptance.' >&2; exit 1; }
mkdir -p tmp/linux-kde-keyboard
wayland-scanner client-header "$linux_protocol" tmp/linux-kde-keyboard/fake-input-client.h
wayland-scanner private-code "$linux_protocol" tmp/linux-kde-keyboard/fake-input-code.c
cc -std=c11 -Wall -Wextra -Werror -I tmp/linux-kde-keyboard tools/linux-kde-keyboard-fixture.c tmp/linux-kde-keyboard/fake-input-code.c $(pkg-config --cflags --libs wayland-client) -o tmp/linux-kde-keyboard/keyboard-fixture
