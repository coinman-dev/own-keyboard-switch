#!/usr/bin/env bash
# Focus-only acceptance: own GTK fixtures and private compositor, no evdev grab.
# Build first: cargo test -p okbs-platform-linux --locked --no-run
set -euo pipefail
cd "$(dirname "$0")/.."
mapfile -t linux_test_binaries < <(find target/debug/deps -maxdepth 1 -name 'okbs_platform_linux-*' -type f -executable -printf '%T@ %p\n' | sort -nr | cut -d' ' -f2-)
[[ ${#linux_test_binaries[@]} != 0 ]] || { echo 'Build Linux platform tests first.' >&2; exit 1; }
export OKBS_TEST_BROWSER_BINARY="$PWD/${linux_test_binaries[0]}"
"$OKBS_TEST_BROWSER_BINARY" --list | grep '^desktop::picker_capture_acceptance::wayland_picker_capture_recovers_metadata_and_rejects_stale_fields: test$' >/dev/null
export OKBS_TEST_APP=capture OKBS_TEST_BROWSER_INPUT=1
case ${OKBS_TEST_DESKTOP:-gnome} in
    gnome) exec bash tools/test-linux-gnome.sh ;;
    kde) exec bash tools/test-linux-kde.sh ;;
    *) echo 'OKBS_TEST_DESKTOP must be gnome or kde.' >&2; exit 1 ;;
esac
