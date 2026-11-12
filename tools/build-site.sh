#!/usr/bin/env bash
# Linux counterpart of tools/build-site.ps1: builds the project page from site/
# into tmp/site-dist and, with --publish, pushes it to the gh-pages branch.
# No GitHub Actions minutes are used. IndexNow pinging stays in the PowerShell
# script.
#
#   tools/build-site.sh                       # build only
#   tools/build-site.sh --publish             # build and push to gh-pages
#   OKBS_SITE_SOURCE=dir tools/build-site.sh  # build another copy of site/
set -euo pipefail
cd "$(dirname "$0")/.."
root=$PWD
publish=0
[[ ${1:-} == --publish ]] && publish=1
site_url=${OKBS_SITE_URL:-https://coinman-dev.github.io/own-keyboard-switch/}
[[ $site_url == */ ]] || site_url+=/
source_dir=${OKBS_SITE_SOURCE:-$root/site}
dist=$root/tmp/site-dist
version=$(sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)"/\1/p' Cargo.toml | head -1)
[[ -n $version ]] || { echo 'Version not found in Cargo.toml' >&2; exit 1; }
date=$(date -u +%F)
build=$(date -u +%Y%m%d%H%M) # cache-busting for styles.css and main.js

rm -rf "$dist"
mkdir -p "$dist/img"
for file in "$source_dir"/*; do
    [[ -f $file ]] || continue
    name=$(basename "$file")
    case $name in
        *.html | *.css | *.js | *.xml | *.txt)
            sed -e "s|{{VERSION}}|$version|g" -e "s|{{SITE_URL}}|$site_url|g" \
                -e "s|{{DATE}}|$date|g" -e "s|{{BUILD}}|$build|g" "$file" >"$dist/$name" ;;
        *) cp "$file" "$dist/$name" ;;
    esac
done
for lang in ru en; do
    mkdir -p "$dist/img/$lang"
    for shot in general rules exclusions autoreplace spellcheck hotkeys; do
        cp "doc/images/$lang/$shot.png" "$dist/img/$lang/"
    done
done
cp images/logo-256.png "$dist/img/logo-256.png"
cp images/logo-settings.png "$dist/img/logo.png"
cp images/okbswitch.ico "$dist/img/favicon.ico"
# GitHub Pages must serve the files as they are, without Jekyll.
: >"$dist/.nojekyll"
echo "Site $version built: $dist"
((publish)) || exit 0

tree=$root/tmp/gh-pages
git worktree prune
if [[ -e $tree ]]; then
    git worktree remove --force "$tree" 2>/dev/null || rm -rf "$tree"
    git worktree prune
fi
git fetch -q origin gh-pages
git worktree add -q --force -B gh-pages "$tree" origin/gh-pages
finish() { git worktree remove --force "$tree" 2>/dev/null || true; }
trap finish EXIT
# Replace the branch content with the fresh build (.git stays).
find "$tree" -mindepth 1 -maxdepth 1 ! -name .git -exec rm -rf {} +
cp -a "$dist"/. "$tree"/
git -C "$tree" add -A
if git -C "$tree" diff --cached --quiet; then
    echo 'gh-pages is already up to date.'
    exit 0
fi
git -C "$tree" commit -q -m "Site for $version (from $(git rev-parse --short HEAD))"
git -C "$tree" push -q origin gh-pages
echo 'Published to gh-pages.'
