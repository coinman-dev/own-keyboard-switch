#!/usr/bin/env bash
# Runs a cargo command inside the WSL build copy made by tools/sync-to-wsl.ps1.
#
# The copy is not a git repository and has its own target directory, so it
# never disturbs a checkout you build in directly.
#
#   wsl -d Ubuntu -- bash ~/tmp/okbs-finish/tools/wsl-check.sh test --workspace
#   wsl -d Ubuntu -- bash ~/tmp/okbs-finish/tools/wsl-check.sh clippy \
#       --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings
set -uo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/okbs-finish-target}"
exec cargo "$@"
