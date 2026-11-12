#!/usr/bin/env bash
# Explicit acceptance in an isolated development VM/WSL or a desktop. The private
# compositor receives only events from the fixture keyboard and OKBS uinput.
# Build first as a regular user: cargo test -p okbs-platform-linux --locked --no-run
set -euo pipefail
cd "$(dirname "$0")/.."
# Root in a WSL/VM; on a desktop with the program's udev rule the session
# user already owns uinput and keyboards. Running as that user also keeps the
# logind session active, which the program requires. Test devices are named
# "OKBS ..." (never taken by a running program) and the output keyboard is
# grabbed, so nothing reaches the host desktop.
if [[ $EUID == 0 ]]; then
    modprobe evdev
elif [[ -w /dev/uinput ]]; then
    export OKBS_DEVICE_ACCEPTANCE=1
else
    echo 'Run as root in an isolated VM, or as the desktop user with uinput access.' >&2; exit 1
fi
mapfile -t linux_test_binaries < <(find target/debug/deps -maxdepth 1 -name 'okbs_platform_linux-*' -type f -executable -printf '%T@ %p\n' | sort -nr | cut -d' ' -f2-)
if [[ ${#linux_test_binaries[@]} == 0 ]]; then echo 'Build the Linux backend tests first.' >&2; exit 1; fi
export OKBS_TEST_BROWSER_BINARY="$PWD/${linux_test_binaries[0]}"
export OKBS_TEST_APP=${OKBS_TEST_APP:-browser} OKBS_TEST_BROWSER_INPUT=1
if [[ $OKBS_TEST_APP == native ]]; then
    "$OKBS_TEST_BROWSER_BINARY" --list | grep '^native_input_acceptance::real_wayland_writer_and_terminal_correct_paste_and_submit: test$' >/dev/null
elif [[ $OKBS_TEST_APP == layout ]]; then
    "$OKBS_TEST_BROWSER_BINARY" --list | grep '^native_input_acceptance::real_single_layout_follows_window_focus: test$' >/dev/null
elif [[ $OKBS_TEST_APP == kwrite ]]; then
    "$OKBS_TEST_BROWSER_BINARY" --list | grep '^native_input_acceptance::real_kwrite_corrects_converts_and_pastes: test$' >/dev/null
elif [[ $OKBS_TEST_APP == stress ]]; then
    "$OKBS_TEST_BROWSER_BINARY" --list | grep '^native_input_acceptance::real_fast_typing_across_conversions_loses_no_word: test$' >/dev/null
elif [[ $OKBS_TEST_APP == settings ]]; then
    "$OKBS_TEST_BROWSER_BINARY" --list | grep '^native_input_acceptance::real_engine_settings_follow_live_configuration: test$' >/dev/null
elif [[ $OKBS_TEST_APP == indicator ]]; then
    "$OKBS_TEST_BROWSER_BINARY" --list | grep '^native_input_acceptance::real_floating_indicator_pointer_actions: test$' >/dev/null
elif [[ $OKBS_TEST_APP == browser ]]; then
    "$OKBS_TEST_BROWSER_BINARY" --list | grep '^browser_acceptance::real_firefox_engine_corrects_and_pastes_without_crossing_fields: test$' >/dev/null
else
    echo 'OKBS_TEST_APP must be browser, native, kwrite, stress, settings, layout or indicator.' >&2; exit 1
fi
case ${OKBS_TEST_DESKTOP:-gnome} in
    gnome) exec bash tools/test-linux-gnome.sh "$@" ;;
    kde)
        bash tools/build-linux-kde-keyboard-fixture.sh
        exec bash tools/test-linux-kde.sh "$@" ;;
    *) echo 'OKBS_TEST_DESKTOP must be gnome or kde.' >&2; exit 1 ;;
esac
