# Mirrors this checkout into the WSL checkout that builds it.
#
# The Rust toolchain lives in WSL and this drive is not mounted there
# (`automount` is off in /etc/wsl.conf), so sources are copied over the
# \\wsl.localhost share before every build. The destination keeps its own
# .git and target directories; large inputs of tools/build-lang-data and build
# outputs stay out of the copy. Private notes in .ai are only added or updated,
# never deleted.
#
#   tools\sync-to-wsl.ps1
#   wsl -d Ubuntu --cd ~/Development/own-keyboard-switch -- bash tools/cross-windows.sh build --release -p okbswitch
#
# Override the distribution or the destination with the environment variables
# OKBS_WSL_DISTRO and OKBS_WSL_DEST (a path inside WSL, as seen from Windows).
$ErrorActionPreference = 'Stop'
$source = Split-Path -Parent $PSScriptRoot
$distro = if ($env:OKBS_WSL_DISTRO) { $env:OKBS_WSL_DISTRO } else { 'Ubuntu' }
$destination = if ($env:OKBS_WSL_DEST) {
    $env:OKBS_WSL_DEST
} else {
    $wslHome = (wsl -d $distro -- bash -lc 'echo $HOME').Trim()
    "\\wsl.localhost\$distro$($wslHome -replace '/', '\')\Development\own-keyboard-switch"
}
robocopy $source $destination /MIR /NFL /NDL /NJH /NJS /NP `
    /XD target .git tmp .ai "$source\data\sources" `
    /XF '*.log' | Out-Null
# Robocopy reports what it did in the exit code: below 8 nothing went wrong.
if ($LASTEXITCODE -ge 8) { throw "robocopy failed with $LASTEXITCODE" }
robocopy "$source\.ai" "$destination\.ai" /E /XO /NFL /NDL /NJH /NJS /NP | Out-Null
if ($LASTEXITCODE -ge 8) { throw "robocopy failed with $LASTEXITCODE" }
Write-Output "synced to $destination"
exit 0
