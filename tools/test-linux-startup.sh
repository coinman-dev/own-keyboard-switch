#!/usr/bin/env bash
# Explicit GUI acceptance with an inaccessible selected device. No setup
# helper is authorized and no physical keyboard is captured by this test.
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ $EUID == 0 ]]; then echo 'Run the startup GUI acceptance as a regular user.' >&2; exit 1; fi
if [[ ${1:-} != --session ]]; then exec dbus-run-session -- bash "$0" --session; fi
export PATH="$HOME/.cargo/bin:$PATH"
mkdir -p tmp/linux-startup
export DISPLAY=:179 XDG_SESSION_TYPE=x11 GDK_BACKEND=x11 LIBGL_ALWAYS_SOFTWARE=1
unset WAYLAND_DISPLAY
Xvfb "$DISPLAY" -noreset -screen 0 1024x768x24 -nolisten tcp >tmp/linux-startup/xvfb.log 2>&1 &
linux_test_pid=$!
finish() { kill "$linux_test_pid" 2>/dev/null || true; }
trap finish EXIT
sleep 1
if ! kill -0 "$linux_test_pid" 2>/dev/null; then cat tmp/linux-startup/xvfb.log; exit 1; fi
setxkbmap -layout us,ru
cargo build -p okbswitch --locked
cargo test -p okbs-platform-linux real_setup_window_stays_open_without_input_access_and_closes_cleanly --locked -- --ignored --nocapture
