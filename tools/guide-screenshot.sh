#!/usr/bin/env bash
# Redraws one settings section of the user guide without a desktop: the
# section is rendered by okbs-ui at the guide's 125% scale and placed under the
# window frame of the existing screenshot (taken on Windows), so all images
# keep one look. Only sections that do not differ between Windows and Linux.
#
#   tools/guide-screenshot.sh About about     # doc/images/{ru,en}/about.png
set -euo pipefail
cd "$(dirname "$0")/.."
section=${1:?section, e.g. About}
image=${2:?guide image name, e.g. about}
OKBS_PREVIEW_SCALE=1.25 OKBS_PREVIEW_SECTION=$section \
    cargo test -q -p okbs-ui --locked render_previews -- --ignored >/dev/null
python3 -I - "$section" "$image" <<'PY'
import sys
from PIL import Image
section, image = sys.argv[1:3]
for lang, rendered in [("ru", f"{section}.png"), ("en", f"{section}-en.png")]:
    path = f"doc/images/{lang}/{image}.png"
    frame = Image.open(path).convert("RGB")
    content = Image.open(f"target/settings-preview/{rendered}").convert("RGB")
    if frame.size != (1002, 789) or content.size != (1000, 750):
        raise SystemExit(f"unexpected sizes {frame.size} {content.size}")
    frame.paste(content, (1, 39))  # below the 39 px title bar, inside the border
    frame.save(path, optimize=True)
    print(path)
PY
