#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ ${1:-} != --session ]]; then exec dbus-run-session -- bash "$0" --session; fi
export PATH="$HOME/.cargo/bin:$PATH"
mkdir -p tmp
# GTK/portal helpers in one WSL desktop are shared across compositor sessions.
# Keep complete Wayland acceptance sessions serial, including their teardown.
exec 9>tmp/linux-wayland-session.lock
flock 9
mkdir -p tmp/linux-kde/{run,config,data}
export XDG_RUNTIME_DIR="$PWD/tmp/linux-kde/run"
export XDG_CONFIG_HOME="$PWD/tmp/linux-kde/config"
export XDG_DATA_HOME="$PWD/tmp/linux-kde/data"
export XDG_CURRENT_DESKTOP=KDE XDG_SESSION_TYPE=wayland LIBGL_ALWAYS_SOFTWARE=1
export GDK_BACKEND=wayland WAYLAND_DISPLAY=okbs-kde-test
unset DISPLAY AT_SPI_BUS_ADDRESS
export GTK_MODULES=atk-bridge GSETTINGS_BACKEND=keyfile
gsettings set org.gnome.desktop.interface toolkit-accessibility true
export AT_SPI_BUS_ADDRESS
AT_SPI_BUS_ADDRESS=$(gdbus call --session --dest org.a11y.Bus --object-path /org/a11y/bus --method org.a11y.Bus.GetAddress | python3 -c 'import ast,sys; print(ast.literal_eval(sys.stdin.read())[0])')
gdbus call --address "$AT_SPI_BUS_ADDRESS" --dest org.a11y.atspi.Registry --object-path /org/a11y/atspi/accessible/root --method org.a11y.atspi.Accessible.GetChildren >tmp/linux-kde/accessibility.log
chmod 700 "$XDG_RUNTIME_DIR"
mkdir -p "$XDG_DATA_HOME/kwin/scripts/okbswitch/contents/code"
sed 's/protocol:4, //' packaging/linux/kde/contents/code/main.js >"$XDG_DATA_HOME/kwin/scripts/okbswitch/contents/code/main.js"
sed 's/"Version": "4.0"/"Version": "1.0"/' packaging/linux/kde/metadata.json >"$XDG_DATA_HOME/kwin/scripts/okbswitch/metadata.json"
kwriteconfig6 --file kwinrc --group Plugins --key okbswitchEnabled true
cat >"$XDG_CONFIG_HOME/kxkbrc" <<'CONFIG'
[Layout]
Use=true
LayoutList=us,ru
Model=pc105
CONFIG
kwin_wayland --virtual --output-count 2 --socket okbs-kde-test --width 1024 --height 768 --no-lockscreen --no-kactivities >tmp/linux-kde/kwin.log 2>&1 &
linux_test_pid=$!
finish() { kill "$linux_test_pid" 2>/dev/null || true; }
trap finish EXIT
for linux_attempt in {1..60}; do
    if gdbus call --session --dest org.kde.keyboard --object-path /Layouts --method org.kde.KeyboardLayouts.getLayout >tmp/linux-kde/layout.log 2>/dev/null; then break; fi
    if ! kill -0 "$linux_test_pid" 2>/dev/null; then cat tmp/linux-kde/kwin.log; exit 1; fi
    sleep 0.5
done
if [[ ${OKBS_TEST_APP:-} == browser ]]; then
    timeout 90s cargo test -p okbs-platform-linux real_firefox_fields_password_and_editable_document_have_distinct_targets --locked -- --ignored --nocapture
    exit
fi
cargo test -p okbs-platform-linux kde_reads_switches_and_verifies_the_real_keyboard_group -- --ignored --nocapture
cargo test -p okbs-platform-linux kde_setup_loads_the_script_and_enables_it_at_login --locked -- --ignored --nocapture
cargo test -p okbs-platform-linux kde_script_tracks_real_windows_and_applies_owned_placement -- --ignored --nocapture
cargo test -p okbs-platform-linux layer_shell_popup_uses_the_secondary_monitor_and_local_margins --locked -- --ignored --nocapture
timeout 60s cargo test -p okbs-ui real_wayland_egui_popups_and_caret_use_the_requested_monitor --locked -- --ignored --nocapture

timeout 60s cargo test -p okbs-platform-linux real_qt_fields_and_kwrite_preserve_caret_password_and_window_identity --locked -- --ignored --nocapture

OKBS_TEST_QT_FRAMELESS=1 timeout 60s cargo test -p okbs-platform-linux real_qt_fields_and_kwrite_preserve_caret_password_and_window_identity --locked -- --ignored --nocapture

timeout 90s cargo test -p okbs-platform-linux real_firefox_fields_password_and_editable_document_have_distinct_targets --locked -- --ignored --nocapture
