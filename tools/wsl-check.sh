#!/usr/bin/env bash
# Runs a cargo command inside the WSL checkout updated by tools/sync-to-wsl.ps1.
#
#   wsl -d Ubuntu -- bash ~/Development/own-keyboard-switch/tools/wsl-check.sh test --workspace
#   wsl -d Ubuntu -- bash ~/Development/own-keyboard-switch/tools/wsl-check.sh clippy \
#       --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings
set -uo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
exec cargo "$@"
