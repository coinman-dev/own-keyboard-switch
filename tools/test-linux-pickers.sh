#!/usr/bin/env bash
# Isolated controller/real-UI acceptance; root creates only the owned uinput.
# Build first: cargo test -p okbswitch --locked --no-run
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $EUID == 0 ]] || { echo 'Run in an isolated development VM as root.' >&2; exit 1; }
modprobe evdev
mapfile -t linux_test_binaries < <(find target/debug/deps -maxdepth 1 -name 'okbswitch-*' -type f -executable -printf '%T@ %p\n' | sort -nr | cut -d' ' -f2-)
[[ ${#linux_test_binaries[@]} != 0 ]] || { echo 'Build the application tests first.' >&2; exit 1; }
export OKBS_TEST_BROWSER_BINARY="$PWD/${linux_test_binaries[0]}"
# Read the complete list: grep -q can close the pipe early and turn a valid
# test binary into a sporadic BrokenPipe failure under pipefail.
"$OKBS_TEST_BROWSER_BINARY" --list | grep '^controller::linux_picker_acceptance::real_linux_pickers_and_process_restart_preserve_editor_and_history: test$' >/dev/null
export OKBS_TEST_APP=picker OKBS_TEST_BROWSER_INPUT=1
case ${OKBS_TEST_DESKTOP:-gnome} in
    gnome) exec bash tools/test-linux-gnome.sh ;;
    kde) bash tools/build-linux-kde-keyboard-fixture.sh; exec bash tools/test-linux-kde.sh ;;
    *) echo 'OKBS_TEST_DESKTOP must be gnome or kde.' >&2; exit 1 ;;
esac
