#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ ${1:-} != --session ]]; then exec dbus-run-session -- bash "$0" --session; fi
export PATH="$HOME/.cargo/bin:$PATH"
mkdir -p tmp
exec 9>tmp/linux-wayland-session.lock
flock 9
linux_gnome_data=tmp/linux-gnome
if [[ ${OKBS_TEST_BROWSER_INPUT:-0} == 1 ]]; then linux_gnome_data=tmp/linux-gnome-input; fi
mkdir -p "$linux_gnome_data"/{run,config,data/gnome-shell/extensions/okbswitch@own-keyboard-switch}
export XDG_RUNTIME_DIR="$PWD/$linux_gnome_data/run"
export XDG_CONFIG_HOME="$PWD/$linux_gnome_data/config"
export XDG_DATA_HOME="$PWD/$linux_gnome_data/data"
export XDG_CURRENT_DESKTOP=GNOME XDG_SESSION_TYPE=wayland LIBGL_ALWAYS_SOFTWARE=1
unset DISPLAY AT_SPI_BUS_ADDRESS
export GSETTINGS_BACKEND=keyfile
export GTK_MODULES=atk-bridge
gsettings set org.gnome.desktop.interface toolkit-accessibility true
export AT_SPI_BUS_ADDRESS
AT_SPI_BUS_ADDRESS=$(gdbus call --session --dest org.a11y.Bus --object-path /org/a11y/bus --method org.a11y.Bus.GetAddress | python3 -c 'import ast,sys; print(ast.literal_eval(sys.stdin.read())[0])')
gdbus call --address "$AT_SPI_BUS_ADDRESS" --dest org.a11y.atspi.Registry --object-path /org/a11y/atspi/accessible/root --method org.a11y.atspi.Accessible.GetChildren >tmp/linux-gnome/accessibility.log
linux_test_scale=${OKBS_TEST_SCALE:-2}
chmod 700 "$XDG_RUNTIME_DIR"
cp packaging/linux/gnome/{metadata.json,extension.js} "$XDG_DATA_HOME/gnome-shell/extensions/okbswitch@own-keyboard-switch/"
if [[ ${OKBS_TEST_OLD_EXTENSION:-0} == 1 ]]; then
    linux_extension="$XDG_DATA_HOME/gnome-shell/extensions/okbswitch@own-keyboard-switch"
    sed -i 's/GetVersion() { return 5; }/GetVersion() { return 4; }/' "$linux_extension/extension.js"
    sed -i 's/"version": 5/"version": 4/' "$linux_extension/metadata.json"
fi
gsettings set org.gnome.desktop.input-sources sources "[('xkb', 'us'), ('xkb', 'ru')]"
gsettings set org.gnome.shell enabled-extensions "['okbswitch@own-keyboard-switch']"
gsettings set org.gnome.shell disable-user-extensions false
gsettings set org.gnome.mutter experimental-features "['scale-monitor-framebuffer']"
gnome-shell --headless --wayland --no-x11 --wayland-display okbs-gnome-test --virtual-monitor 1920x1080 --virtual-monitor 1200x900 >tmp/linux-gnome/shell.log 2>&1 &
linux_test_pid=$!
linux_keyboard_pid=
finish() {
    [[ -z "$linux_keyboard_pid" ]] || kill "$linux_keyboard_pid" 2>/dev/null || true
    kill "$linux_test_pid" 2>/dev/null || true
}
trap finish EXIT
for linux_attempt in {1..60}; do
    if [[ $linux_attempt == 5 ]]; then
        gnome-extensions list >tmp/linux-gnome/extensions.log 2>&1 || true
        gnome-extensions enable okbswitch@own-keyboard-switch >>tmp/linux-gnome/extensions.log 2>&1 || true
    fi
    if gdbus call --session --dest org.own_keyboard_switch.Gnome --object-path /org/own_keyboard_switch/Gnome --method org.own_keyboard_switch.Gnome.GetState >tmp/linux-gnome/state.log 2>/dev/null; then break; fi
    if ! kill -0 "$linux_test_pid" 2>/dev/null; then cat tmp/linux-gnome/shell.log; exit 1; fi
    sleep 0.5
done
export GDK_BACKEND=wayland WAYLAND_DISPLAY=okbs-gnome-test
test -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY"
linux_keyboard_ready="$PWD/tmp/linux-gnome/keyboard-ready"
rm -f "$linux_keyboard_ready"
python3 tools/linux-gnome-keyboard-fixture.py "$linux_keyboard_ready" >tmp/linux-gnome/keyboard.log 2>&1 &
linux_keyboard_pid=$!
for linux_attempt in {1..30}; do
    [[ ! -f "$linux_keyboard_ready" ]] || break
    kill -0 "$linux_keyboard_pid" || { cat tmp/linux-gnome/keyboard.log; exit 1; }
    sleep 0.1
done
test -f "$linux_keyboard_ready"
# The private headless compositor can start in Overview. Native window
# acceptance needs an actual application input target, not the background WM
# focus_window retained while Shell owns the keyboard.
gdbus call --session --dest org.gnome.Shell --object-path /org/gnome/Shell --method org.freedesktop.DBus.Properties.Set org.gnome.Shell OverviewActive '<false>'
sleep 0.5
gdctl show >tmp/linux-gnome/monitors.log
gdctl set -L -M Meta-1 --primary -L -M Meta-0 --scale "$linux_test_scale" --right-of Meta-1
gdctl show >tmp/linux-gnome/scaled-monitors.log
gdbus call --session --dest org.own_keyboard_switch.Gnome --object-path /org/own_keyboard_switch/Gnome --method org.own_keyboard_switch.Gnome.SetLayout 1
gdbus call --session --dest org.own_keyboard_switch.Gnome --object-path /org/own_keyboard_switch/Gnome --method org.own_keyboard_switch.Gnome.GetState
gdbus call --session --dest org.own_keyboard_switch.Gnome --object-path /org/own_keyboard_switch/Gnome --method org.own_keyboard_switch.Gnome.SetLayout 0
gdbus call --session --dest org.own_keyboard_switch.Gnome --object-path /org/own_keyboard_switch/Gnome --method org.own_keyboard_switch.Gnome.SetPanel '{"id":"hint","visible":true,"position":[24,24],"text":"OKBS isolated hint","opacity":1}'
gdbus call --session --dest org.own_keyboard_switch.Gnome --object-path /org/own_keyboard_switch/Gnome --method org.own_keyboard_switch.Gnome.SetPanel '{"id":"list","visible":true,"position":[40,80],"rows":["first","second"],"opacity":0.8}'
gdbus call --session --dest org.own_keyboard_switch.Gnome --object-path /org/own_keyboard_switch/Gnome --method org.own_keyboard_switch.Gnome.TakePanelEvents
gdbus call --session --dest org.own_keyboard_switch.Gnome --object-path /org/own_keyboard_switch/Gnome --method org.own_keyboard_switch.Gnome.SetPanel '{"id":"indicator","visible":true,"position":[980,740],"locked":false,"settings":"Settings","hide":"Hide","lock":"Lock","opacity":1}'
if [[ ${OKBS_TEST_APP:-} == native ]]; then
    timeout 120s "$OKBS_TEST_BROWSER_BINARY" real_wayland_writer_and_terminal_correct_paste_and_submit --ignored --nocapture
    exit
fi
if [[ ${OKBS_TEST_APP:-} == browser ]]; then
    if [[ ${OKBS_TEST_BROWSER_INPUT:-0} == 1 ]]; then
        timeout 90s "$OKBS_TEST_BROWSER_BINARY" real_firefox_engine_corrects_and_pastes_without_crossing_fields --ignored --nocapture
    else
        timeout 90s cargo test -p okbs-platform-linux real_firefox_fields_password_and_editable_document_have_distinct_targets --locked -- --ignored --nocapture
    fi
    exit
fi
cargo test -p okbs-platform-linux gnome_panel_is_clamped_on_a_second_scaled_monitor --locked -- --ignored --nocapture
cargo test -p okbs-platform-linux gnome_setup_reenables_the_component_and_preserves_other_settings --locked -- --ignored --nocapture
cargo test -p okbs-platform-linux gnome_places_a_real_window_on_the_requested_secondary_monitor --locked -- --ignored --nocapture
if [[ ${OKBS_TEST_OLD_EXTENSION:-0} != 1 ]]; then
    # An older loaded protocol must stop at the setup/re-login gate.
    timeout 60s cargo test -p okbs-platform-linux real_qt_fields_and_kwrite_preserve_caret_password_and_window_identity --locked -- --ignored --nocapture
    OKBS_TEST_QT_FRAMELESS=1 timeout 60s cargo test -p okbs-platform-linux real_qt_fields_and_kwrite_preserve_caret_password_and_window_identity --locked -- --ignored --nocapture
    timeout 60s cargo test -p okbs-ui real_wayland_egui_popups_and_caret_use_the_requested_monitor --locked -- --ignored --nocapture
    timeout 90s cargo test -p okbs-platform-linux real_firefox_fields_password_and_editable_document_have_distinct_targets --locked -- --ignored --nocapture
fi
cargo test -p okbs-platform-linux gnome_repositions_a_panel_when_the_second_monitor_disappears --locked -- --ignored --nocapture
