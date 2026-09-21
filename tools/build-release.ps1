param(
    [string]$Tag = '',
    [string]$SourceExe = '',
    [string]$OutputDirectory = '',
    [switch]$SkipBuild
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    $manifest = Get-Content -LiteralPath 'Cargo.toml' -Raw
    $version = [regex]::Match($manifest, '(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value
    if ($version -notmatch '^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$') { throw 'Invalid workspace version.' }
    if ($Tag -and $Tag -ne "v$version") { throw "Tag $Tag does not match Cargo.toml ($version)." }
    if (-not $OutputDirectory) { $OutputDirectory = Join-Path $root 'target\dist' }
    $OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
    New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
    if (-not $SkipBuild) {
        & cargo build --release --locked -p okbswitch --target x86_64-pc-windows-msvc
        if ($LASTEXITCODE -ne 0) { throw 'Windows release build failed.' }
    }
    if (-not $SourceExe) { $SourceExe = Join-Path $root 'target\x86_64-pc-windows-msvc\release\okbswitch.exe' }
    $SourceExe = (Resolve-Path -LiteralPath $SourceExe).ProviderPath
    $binaryVersion = [Diagnostics.FileVersionInfo]::GetVersionInfo($SourceExe).ProductVersion
    if ($binaryVersion -ne $version) { throw "Executable version $binaryVersion does not match $version." }
    $makensis = Get-Command makensis -ErrorAction SilentlyContinue
    $nsisExe = if ($makensis) { $makensis.Source } else { Join-Path ${env:ProgramFiles(x86)} 'NSIS\makensis.exe' }
    if (-not (Test-Path -LiteralPath $nsisExe)) { throw 'NSIS is required to package the installer.' }
    $numeric = ($version -split '-')[0] + '.0'
    & $nsisExe /V2 "/DVERSION=$version" "/DVERSION_NUMERIC=$numeric" "/DSOURCE_EXE=$SourceExe" "/DROOT=$root" "/DOUTPUT=$OutputDirectory\okbswitch-install.exe" "$root\packaging\windows\okbswitch.nsi"
    if ($LASTEXITCODE -ne 0) { throw 'Installer packaging failed.' }
    Copy-Item -LiteralPath $SourceExe -Destination "$OutputDirectory\okbswitch-portable.exe" -Force
    Copy-Item -LiteralPath LICENSE -Destination "$OutputDirectory\LICENSE.txt" -Force
    Copy-Item -LiteralPath NOTICE -Destination "$OutputDirectory\NOTICE.txt" -Force
    Copy-Item -LiteralPath THIRD-PARTY-NOTICES.txt -Destination $OutputDirectory -Force
    $names = @('okbswitch-portable.exe', 'okbswitch-install.exe', 'LICENSE.txt', 'NOTICE.txt', 'THIRD-PARTY-NOTICES.txt')
    $checksums = foreach ($name in $names) {
        $hash = (Get-FileHash -LiteralPath (Join-Path $OutputDirectory $name) -Algorithm SHA256).Hash.ToLowerInvariant()
        "$hash  $name"
    }
    [IO.File]::WriteAllLines((Join-Path $OutputDirectory 'SHA256SUMS.txt'), $checksums, [Text.UTF8Encoding]::new($false))
    Write-Output "Release $version packaged in $OutputDirectory"
} finally { Pop-Location }
