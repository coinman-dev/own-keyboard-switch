#!/usr/bin/env bash
# Explicit root-only acceptance in an isolated development VM/WSL. The private
# compositor receives only events from the fixture keyboard and OKBS uinput.
# Build first as a regular user: cargo test -p okbs-platform-linux --locked --no-run
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ $EUID != 0 ]]; then echo 'Run this device acceptance as root in an isolated development VM.' >&2; exit 1; fi
modprobe evdev
mapfile -t linux_test_binaries < <(find target/debug/deps -maxdepth 1 -name 'okbs_platform_linux-*' -type f -executable -printf '%T@ %p\n' | sort -nr | cut -d' ' -f2-)
if [[ ${#linux_test_binaries[@]} == 0 ]]; then echo 'Build the Linux backend tests first.' >&2; exit 1; fi
export OKBS_TEST_BROWSER_BINARY="$PWD/${linux_test_binaries[0]}"
export OKBS_TEST_APP=${OKBS_TEST_APP:-browser} OKBS_TEST_BROWSER_INPUT=1
if [[ $OKBS_TEST_APP == native ]]; then
    "$OKBS_TEST_BROWSER_BINARY" --list | grep '^native_input_acceptance::real_wayland_writer_and_terminal_correct_paste_and_submit: test$' >/dev/null
elif [[ $OKBS_TEST_APP == browser ]]; then
    "$OKBS_TEST_BROWSER_BINARY" --list | grep '^browser_acceptance::real_firefox_engine_corrects_and_pastes_without_crossing_fields: test$' >/dev/null
else
    echo 'OKBS_TEST_APP must be browser or native.' >&2; exit 1
fi
case ${OKBS_TEST_DESKTOP:-gnome} in
    gnome) exec bash tools/test-linux-gnome.sh "$@" ;;
    kde)
        bash tools/build-linux-kde-keyboard-fixture.sh
        exec bash tools/test-linux-kde.sh "$@" ;;
    *) echo 'OKBS_TEST_DESKTOP must be gnome or kde.' >&2; exit 1 ;;
esac
