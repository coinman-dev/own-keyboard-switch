#!/usr/bin/env bash
# Prints the release description of <version> in Russian: the files, the
# section of CHANGELOG.ru_RU.md for that version, the license note.
#
#   tools/release-notes.sh 0.2.0-beta > notes.md
set -euo pipefail

version="${1:?usage: tools/release-notes.sh <version>}"
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"

# The lines under «## <version>» or «## <version> — <date>», up to the next
# section, without the blank lines at its start. GitHub shows every line break
# of a release description, so the wrapped lines of an item are joined.
changes="$(awk -v heading="## $version" '
    /^## / {
        if (found) exit
        found = ($0 == heading || index($0, heading " ") == 1)
        next
    }
    found
' "$root/CHANGELOG.ru_RU.md" | sed '/./,$!d' | awk '
    /^ +[^ ]/ && have { sub(/^ +/, ""); line = line " " $0; next }
    { if (have) print line; line = $0; have = 1 }
    END { if (have) print line }
')"
if [[ -z "${changes//[[:space:]]/}" ]]; then
    echo "CHANGELOG.ru_RU.md has no section for $version" >&2
    exit 1
fi

if [[ "$version" == *-* ]]; then
    kind='Предварительная версия'
else
    kind='Версия'
fi

cat <<EOF
$kind для Windows 10/11 x64 и Linux x86_64.

Windows:

- **okbswitch-install.exe** — мастер установки: для себя или для всех пользователей. Ставится поверх прежней версии, настройки сохраняются.
- **okbswitch-portable.exe** — без установки: положите в папку, доступную для записи, и запустите.

Linux (X11, GNOME 45–50 и KDE Plasma 6 на Wayland):

- **okbswitch_${version}_amd64.deb** — пакет для Ubuntu 24.04 и новее: \`sudo apt install ./okbswitch_${version}_amd64.deb\`.
- **okbswitch-${version}-x86_64.AppImage** — без установки: сделайте файл исполняемым и запустите.

При первом запуске в Linux программа просит разрешение (pkexec), чтобы дать сеансу доступ к клавиатурам.

$changes

Portable-версия и AppImage хранят настройки и журналы рядом с программой (\`data\` и \`Logs\`).

Лицензия — PolyForm Noncommercial 1.0.0, сторонние компоненты — под своими лицензиями (в программе: «О программе → Лицензии...»). Контрольные суммы файлов — в SHA256SUMS.txt.
EOF
