<#
.SYNOPSIS
  Builds the project page (GitHub Pages) from site/ and, with -Publish, pushes it
  to the gh-pages branch.

.DESCRIPTION
  The page sources live in site/. The build copies them to tmp/site-dist, puts the
  version from Cargo.toml in place of {{VERSION}} and takes the screenshots from
  doc/images and the logo from images/, so the page always shows the same pictures
  as the user guide. The page itself asks the GitHub API for the newest release
  and updates the version and the download links, so a new release does not
  need a rebuild; the built version is the fallback. {{SITE_URL}} becomes -SiteUrl and {{DATE}} the build date
  (robots.txt, sitemap.xml, canonical and hreflang links, schema.org data).

  -Ping tells search engines that support IndexNow (Bing, Yandex, Seznam, Naver)
  that the page changed. It proves ownership with the key file site/<key>.txt,
  so it works only after that file is published.

  -Publish commits the result to the gh-pages branch through a separate worktree
  (tmp/gh-pages) and pushes it. No GitHub Actions minutes are used for the build.

.EXAMPLE
  tools/build-site.ps1            # build only; open tmp/site-dist/index.html
  tools/build-site.ps1 -Publish   # build and push to gh-pages
  tools/build-site.ps1 -Publish -Ping
#>
[CmdletBinding()]
param(
    [switch]$Publish,
    [switch]$Ping,
    # Change it here if the site gets its own domain.
    [string]$SiteUrl = 'https://coinman-dev.github.io/own-keyboard-switch/'
)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$dist = Join-Path $root 'tmp/site-dist'
$utf8 = [System.Text.UTF8Encoding]::new($false)

$cargo = [System.IO.File]::ReadAllText((Join-Path $root 'Cargo.toml'))
if ($cargo -notmatch '(?m)^version\s*=\s*"([^"]+)"') { throw 'Version not found in Cargo.toml' }
$version = $Matches[1]
if (-not $SiteUrl.EndsWith('/')) { $SiteUrl += '/' }
$date = (Get-Date).ToUniversalTime().ToString('yyyy-MM-dd')
$build = (Get-Date).ToUniversalTime().ToString('yyyyMMddHHmm')  # cache-busting for styles.css and main.js

if (Test-Path $dist) { Remove-Item $dist -Recurse -Force }
New-Item -ItemType Directory -Force (Join-Path $dist 'img') | Out-Null

foreach ($file in Get-ChildItem (Join-Path $root 'site') -File) {
    $target = Join-Path $dist $file.Name
    if ($file.Extension -in '.html', '.css', '.js', '.xml', '.txt') {
        $text = [System.IO.File]::ReadAllText($file.FullName, $utf8).
            Replace('{{VERSION}}', $version).Replace('{{SITE_URL}}', $SiteUrl).Replace('{{DATE}}', $date).Replace('{{BUILD}}', $build)
        [System.IO.File]::WriteAllText($target, $text, $utf8)
    } else {
        Copy-Item $file.FullName $target
    }
}

# Screenshots used by the page, in both languages.
$shots = 'general', 'rules', 'exclusions', 'autoreplace', 'spellcheck', 'hotkeys'
foreach ($lang in 'ru', 'en') {
    $dir = New-Item -ItemType Directory -Force (Join-Path $dist "img/$lang")
    foreach ($shot in $shots) {
        Copy-Item (Join-Path $root "doc/images/$lang/$shot.png") $dir.FullName
    }
}
Copy-Item (Join-Path $root 'images/logo-256.png') (Join-Path $dist 'img/logo-256.png')
Copy-Item (Join-Path $root 'images/logo-settings.png') (Join-Path $dist 'img/logo.png')
Copy-Item (Join-Path $root 'images/okbswitch.ico') (Join-Path $dist 'img/favicon.ico')
# GitHub Pages must serve the files as they are, without Jekyll.
[System.IO.File]::WriteAllText((Join-Path $dist '.nojekyll'), '')

Write-Host "Site $version built: $dist"
if (-not $Publish) { return }

$tree = Join-Path $root 'tmp/gh-pages'
Push-Location $root
try {
    git worktree prune
    # A leftover folder from an earlier run (for example, one an editor kept open).
    if (Test-Path $tree) {
        git worktree remove --force $tree 2>$null
        if (Test-Path $tree) { Remove-Item $tree -Recurse -Force }
        git worktree prune
    }

    git fetch origin gh-pages 2>$null
    git rev-parse --verify --quiet refs/remotes/origin/gh-pages | Out-Null
    if ($LASTEXITCODE -eq 0) {
        git worktree add --force -B gh-pages $tree origin/gh-pages
    } else {
        git worktree add --detach $tree
        git -C $tree switch --orphan gh-pages
    }
    if ($LASTEXITCODE) { throw 'Cannot prepare the gh-pages worktree' }

    # Replace the branch content with the fresh build (.git stays).
    Get-ChildItem $tree -Force | Where-Object Name -ne '.git' | Remove-Item -Recurse -Force
    Copy-Item (Join-Path $dist '*') $tree -Recurse -Force
    Copy-Item (Join-Path $dist '.nojekyll') $tree -Force

    git -C $tree add -A
    git -C $tree diff --cached --quiet
    if ($LASTEXITCODE -eq 0) {
        Write-Host 'gh-pages is already up to date.'
    } else {
        $source = git rev-parse --short HEAD
        git -C $tree commit -m "Site for $version (from $source)"
        if ($LASTEXITCODE) { throw 'Commit failed' }
        git -C $tree push origin gh-pages
        if ($LASTEXITCODE) { throw 'Push failed' }
        Write-Host 'Published to gh-pages.'
    }
    git worktree remove --force $tree
} finally {
    Pop-Location
}

if (-not $Ping) { return }

# IndexNow: the engines fetch <site>/<key>.txt to check that we own the site,
# so wait until GitHub Pages serves the fresh build.
$keyFile = Get-ChildItem (Join-Path $root 'site') -File | Where-Object Name -match '^[0-9a-f]{32}\.txt$' | Select-Object -First 1
if (-not $keyFile) { throw 'IndexNow key file site/<key>.txt not found' }
$key = $keyFile.BaseName
$deadline = (Get-Date).AddMinutes(10)
do {
    try {
        $served = (Invoke-WebRequest "$SiteUrl$key.txt?nocache=$(Get-Random)" -UseBasicParsing -TimeoutSec 20).Content.Trim()
    } catch { $served = '' }
    if ($served -eq $key) { break }
    Write-Host 'Waiting for GitHub Pages to serve the key file...'
    Start-Sleep 15
} while ((Get-Date) -lt $deadline)
if ($served -ne $key) { throw "The site does not serve $key.txt yet; run again with -Ping later." }

$body = @{
    host        = ([uri]$SiteUrl).Host
    key         = $key
    keyLocation = "$SiteUrl$key.txt"
    urlList     = @($SiteUrl, "${SiteUrl}?lang=en")
} | ConvertTo-Json
$response = Invoke-WebRequest 'https://api.indexnow.org/indexnow' -Method Post -UseBasicParsing `
    -ContentType 'application/json; charset=utf-8' -Body $body -SkipHttpErrorCheck
# 200 = accepted, 202 = accepted, key check pending.
Write-Host "IndexNow: HTTP $($response.StatusCode)"
if ($response.StatusCode -notin 200, 202) { throw "IndexNow refused the submission: $($response.Content)" }
