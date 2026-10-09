#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ ${1:-} != --session ]]; then exec dbus-run-session -- bash "$0" --session; fi
export PATH="$HOME/.cargo/bin:$PATH"
mkdir -p tmp/linux-x11
export DISPLAY=:177 XDG_SESSION_TYPE=x11
export GDK_BACKEND=x11
unset WAYLAND_DISPLAY
export GTK_MODULES=atk-bridge
Xvfb "$DISPLAY" -noreset -screen 0 1024x768x24 -nolisten tcp >tmp/linux-x11/xvfb.log 2>&1 &
linux_test_pid=$!
finish() { kill "$linux_test_pid" 2>/dev/null || true; }
trap finish EXIT
sleep 1
if ! kill -0 "$linux_test_pid" 2>/dev/null; then cat tmp/linux-x11/xvfb.log; exit 1; fi
setxkbmap -layout us,ru
xprop -root _XKB_RULES_NAMES
cargo test -p okbs-platform-linux x11_geometry_uses_root_coordinates_for_reparented_client_windows --locked -- --ignored --nocapture
cargo test -p okbs-platform-linux x11_reads_switches_and_verifies_the_real_keyboard_group -- --ignored --nocapture
cargo test -p okbs-platform-linux x11_backend_change_preserves_clones_and_rejects_unavailable_methods --locked -- --ignored --nocapture
cargo test -p okbs-platform-linux x11_diagnosis_uses_the_same_backend_as_the_application --locked -- --ignored --nocapture
cargo test -p okbs-platform-linux passive_panels_preserve_focus_report_choices_and_autohide -- --ignored --nocapture
GDK_SCALE=2 cargo test -p okbs-platform-linux x11_panel_maps_physical_coordinates_to_gtk_scale_two --locked -- --ignored --nocapture
cargo test -p okbs-platform-linux real_accessibility_distinguishes_fields_password_and_caret_without_reading_text -- --ignored --nocapture
cargo test -p okbs-platform-linux logind_properties_are_read_and_missing_service_fails_closed -- --ignored --nocapture
