#!/usr/bin/env bash
# Builds or tests the Windows version from Linux/WSL with llvm-mingw
# (https://github.com/mstorsjo/llvm-mingw) and the x86_64-pc-windows-gnullvm target.
# Under WSL with interop enabled, test binaries (.exe) run directly on Windows.
#
#   LLVM_MINGW=~/.local/opt/llvm-mingw-... tools/cross-windows.sh build --release -p okbswitch
#   LLVM_MINGW=... tools/cross-windows.sh test -p okbs-core -p okbs-engine
#
# Release builds for distribution use the MSVC target on Windows (see CI).
set -euo pipefail

if [[ -z "${LLVM_MINGW:-}" ]]; then
    LLVM_MINGW="$(ls -d "$HOME"/.local/opt/llvm-mingw-* 2>/dev/null | sort | tail -1 || true)"
fi
if [[ ! -x "$LLVM_MINGW/bin/x86_64-w64-mingw32-clang" ]]; then
    echo "llvm-mingw not found; set LLVM_MINGW to its directory" >&2
    exit 1
fi
rustup target add x86_64-pc-windows-gnullvm >/dev/null

export CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_LINKER="$LLVM_MINGW/bin/x86_64-w64-mingw32-clang"
export CC_x86_64_pc_windows_gnullvm="$LLVM_MINGW/bin/x86_64-w64-mingw32-clang"
export AR_x86_64_pc_windows_gnullvm="$LLVM_MINGW/bin/llvm-ar"
# Resource compiler discovery also needs llvm-mingw's windres on PATH.
export PATH="$LLVM_MINGW/bin:$PATH"
# Link libunwind and the C runtime statically: a single .exe without libunwind.dll.
export CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_RUSTFLAGS="-C target-feature=+crt-static"
command="$1"
shift
exec cargo "$command" --target x86_64-pc-windows-gnullvm "$@"
