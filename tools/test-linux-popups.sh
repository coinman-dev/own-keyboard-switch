#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ ${1:-} != --session ]]; then exec dbus-run-session -- bash "$0" --session; fi
export PATH="$HOME/.cargo/bin:$PATH"
mkdir -p tmp/linux-popups
export DISPLAY=:179 XDG_SESSION_TYPE=x11 GDK_BACKEND=x11 LIBGL_ALWAYS_SOFTWARE=1
export WINIT_X11_SCALE_FACTOR=${OKBS_TEST_SCALE:-2}
unset WAYLAND_DISPLAY
Xvfb "$DISPLAY" -noreset -screen 0 2800x1400x24 -nolisten tcp >tmp/linux-popups/xvfb.log 2>&1 &
linux_x_pid=$!
linux_wm_pid=
finish() {
    [[ -z "$linux_wm_pid" ]] || kill "$linux_wm_pid" 2>/dev/null || true
    kill "$linux_x_pid" 2>/dev/null || true
}
trap finish EXIT
sleep 1
xrandr --setmonitor LEFT 1400/370x1400/370+0+0 screen
xrandr --setmonitor RIGHT 1400/370x1400/370+1400+0 none
xrandr --listmonitors >tmp/linux-popups/monitors.log
openbox --config-file /dev/null >tmp/linux-popups/openbox.log 2>&1 &
linux_wm_pid=$!
sleep 1
kill -0 "$linux_x_pid"
kill -0 "$linux_wm_pid"
cargo test -p okbs-ui x11_real_egui_popups_fit_workareas_and_preserve_passive_focus --locked -- --ignored --nocapture
