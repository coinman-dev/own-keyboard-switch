#!/usr/bin/env bash
# Run explicitly as root in an isolated development VM/WSL. Creates a test
# keyboard rather than sending keys to a real user's physical keyboard.
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ $EUID != 0 ]]; then echo 'This device integration test requires root.' >&2; exit 1; fi
mkdir -p tmp/linux-input
export DISPLAY=:178 XDG_SESSION_TYPE=x11
export GDK_BACKEND=x11 GTK_MODULES=atk-bridge
unset WAYLAND_DISPLAY
Xvfb "$DISPLAY" -noreset -screen 0 1024x768x24 -nolisten tcp >tmp/linux-input/xvfb.log 2>&1 &
linux_test_pid=$!
finish() { kill "$linux_test_pid" 2>/dev/null || true; }
trap finish EXIT
sleep 1
if ! kill -0 "$linux_test_pid" 2>/dev/null; then cat tmp/linux-input/xvfb.log; exit 1; fi
setxkbmap -layout us,ru
mapfile -t linux_test_binaries < <(find target/debug/deps -maxdepth 1 -name 'okbs_platform_linux-*' -type f -executable -printf '%T@ %p\n' | sort -nr | cut -d' ' -f2-)
if [[ ${#linux_test_binaries[@]} == 0 ]]; then echo 'Build the Linux backend tests first.' >&2; exit 1; fi
linux_test_filter=${1:-}
linux_test_matched=false
for linux_test_name in \
    real_evdev_source_withholds_boundaries_and_releases_the_device \
    real_terminal_receives_corrected_line_before_enter_and_enter_is_not_replayed \
    stale_spelling_popup_never_changes_another_real_field; do
    if [[ $linux_test_name != *"$linux_test_filter"* ]]; then continue; fi
    linux_test_matched=true
    if [[ $linux_test_name == stale_spelling_popup_never_changes_another_real_field ]]; then
        dbus-run-session -- "${linux_test_binaries[0]}" "$linux_test_name" --ignored --nocapture
    else
        "${linux_test_binaries[0]}" "$linux_test_name" --ignored --nocapture
    fi
done
if [[ $linux_test_matched == false ]]; then echo 'No matching input acceptance test.' >&2; exit 1; fi
