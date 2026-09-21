# Mirrors this checkout into a WSL build copy.
#
# The Rust toolchain lives in WSL and this drive is not mounted there
# (`automount` is off in /etc/wsl.conf), so sources are copied over the
# \\wsl.localhost share before every build. Large inputs of tools/build-lang-data
# and build outputs stay out of the copy.
#
#   tools\sync-to-wsl.ps1
#   wsl -d Ubuntu -- bash ~/tmp/okbs-finish/tools/wsl-check.sh test --workspace
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
    "\\wsl.localhost\$distro$($wslHome -replace '/', '\')\tmp\okbs-finish"
}
robocopy $source $destination /MIR /NFL /NDL /NJH /NJS /NP `
    /XD target .git tmp "$source\data\sources" `
    /XF '*.log' | Out-Null
# Robocopy reports what it did in the exit code: below 8 nothing went wrong.
if ($LASTEXITCODE -ge 8) { throw "robocopy failed with $LASTEXITCODE" }
Write-Output "synced to $destination"
exit 0
