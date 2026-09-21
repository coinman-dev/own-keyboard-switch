param(
    [Parameter(Mandatory=$true)][string]$Installer,
    [ValidateSet('CurrentUser','AllUsers')][string]$Mode = 'CurrentUser'
)
$ErrorActionPreference = 'Stop'
if (-not $env:CI) { throw 'Run installer integration checks on an isolated CI runner.' }
$installerPath = (Resolve-Path -LiteralPath $Installer).ProviderPath
$testRoot = if ($Mode -eq 'AllUsers') {
    Join-Path $env:ProgramFiles 'Okbswitch CI install test'
} else {
    Join-Path $env:RUNNER_TEMP 'okbs-installer-test'
}
if (Test-Path -LiteralPath $testRoot) { throw 'The test directory already exists.' }
$setup = Start-Process -FilePath $installerPath -ArgumentList "/S /$Mode /D=$testRoot" -Wait -PassThru -WindowStyle Hidden
if ($setup.ExitCode -ne 0) { throw "Installer failed: $($setup.ExitCode)." }
foreach ($file in @('okbswitch.exe','uninstall.exe','LICENSE.txt','NOTICE.txt','THIRD-PARTY-NOTICES.txt')) {
    if (-not (Test-Path -LiteralPath (Join-Path $testRoot $file))) { throw "Missing installed file: $file" }
}
foreach ($directory in @('data','log')) {
    if (-not (Test-Path -LiteralPath (Join-Path $testRoot $directory) -PathType Container)) { throw "Missing data directory: $directory" }
}
$config = Join-Path $testRoot 'data\config.toml'
[IO.File]::WriteAllText($config, "version = 3`n[general]`nautoswitch = false`n", [Text.UTF8Encoding]::new($false))
$originalHash = (Get-FileHash -LiteralPath $config -Algorithm SHA256).Hash
$setup = Start-Process -FilePath $installerPath -ArgumentList "/S /$Mode /D=$testRoot" -Wait -PassThru -WindowStyle Hidden
if ($setup.ExitCode -ne 0 -or (Get-FileHash -LiteralPath $config -Algorithm SHA256).Hash -ne $originalHash) { throw 'Reinstallation changed user settings.' }
$uninstaller = Join-Path $testRoot 'uninstall.exe'
# _?= runs the uninstaller in place, making -Wait wait for the actual removal.
$removed = Start-Process -FilePath $uninstaller -ArgumentList "/S /$Mode _?=$testRoot" -Wait -PassThru -WindowStyle Hidden
if ($removed.ExitCode -ne 0) { throw 'Uninstaller failed.' }
if (Test-Path -LiteralPath (Join-Path $testRoot 'okbswitch.exe')) { throw 'Uninstaller left the program executable.' }
if ((Get-FileHash -LiteralPath $config -Algorithm SHA256).Hash -ne $originalHash) { throw 'Silent uninstall removed settings.' }
Write-Output "$Mode install, reinstall and uninstall checks passed; settings preserved."
