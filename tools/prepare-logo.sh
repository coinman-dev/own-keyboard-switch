#!/usr/bin/env bash
# Rebuild the application artwork from the original JPEG with ImageMagick 7.
# Usage: tools/prepare-logo.sh
#        MAGICK=/path/to/magick tools/prepare-logo.sh
set -euo pipefail

root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
magick="${MAGICK:-magick}"
source_image="$root/images/Gemini_Generated_Image_hj42kdhj42kdhj42.jpg"
if ! command -v "$magick" >/dev/null; then
    echo 'Install ImageMagick 7: sudo apt install imagemagick' >&2
    exit 1
fi
if [[ "$("$magick" identify -format '%wx%h' "$source_image")" != '2760x1504' ]]; then
    echo 'The cutout is calibrated for the original 2760x1504 JPEG.' >&2
    exit 1
fi

work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT

# Exclude the paper edges and its shadow before finding the logo silhouette.
"$magick" "$source_image" -crop 1000x1000+910+250 +repage "$work/crop.png"
# Join the narrow white separators, then fill enclosed areas so the letters
# and white details remain opaque. Only the exterior becomes transparent.
"$magick" "$work/crop.png" -colorspace gray -threshold 85% -negate \
    -morphology Close Disk:18 \
    -fill '#808080' -draw 'color 0,0 floodfill' \
    -fill white +opaque '#808080' -fill black -opaque '#808080' \
    -morphology Erode Disk:1 -blur 0x0.5 "$work/alpha.png"
# Linear contrast +5% about middle gray: (channel - 0.5) * 1.05 + 0.5.
# Use Polynomial to avoid intermediate clipping with non-HDRI builds.
"$magick" "$work/crop.png" "$work/alpha.png" \
    -alpha off -compose CopyOpacity -composite \
    -channel RGB -function Polynomial '1.05,-0.025' +channel \
    -trim +repage -strip -depth 8 "PNG32:$root/images/logo.png"

# Transparent square padding leaves room around the arrow tips at small sizes.
"$magick" "$root/images/logo.png" -filter Lanczos -resize 240x240 \
    -gravity center -background none -extent 256x256 \
    -strip -depth 8 "PNG32:$root/images/logo-256.png"
"$magick" "$root/images/logo-256.png" \
    -define icon:auto-resize=256,128,64,48,32,24,16 "$root/images/okbswitch.ico"

# Sidebar artwork: 174 logical pixels at 3× resolution. Dilate the alpha by
# 9 asset pixels to create a white 3 pt outline around the logo silhouette.
"$magick" "$root/images/logo.png" -filter Lanczos -resize 498x498 \
    -gravity center -background none -extent 522x522 "$work/settings.png"
"$magick" "$work/settings.png" \
    \( +clone -alpha extract -morphology Dilate Disk:9 \
       -background white -alpha shape \) \
    +swap -compose Over -composite -strip -depth 8 \
    "PNG32:$root/images/logo-settings.png"

"$magick" identify "$root/images/logo.png" "$root/images/logo-256.png" \
    "$root/images/logo-settings.png" "$root/images/okbswitch.ico"
