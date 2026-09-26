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
$kind для Windows 10/11 x64.

- **okbswitch-install.exe** — мастер установки: для себя или для всех пользователей. Ставится поверх прежней версии, настройки сохраняются.
- **okbswitch-portable.exe** — без установки: положите в папку, доступную для записи, и запустите.

$changes

Настройки и журналы хранятся в папке программы (\`data\` и \`Logs\`). Версия для Linux в разработке.

Лицензия — PolyForm Noncommercial 1.0.0, сторонние компоненты — под своими лицензиями (в программе: «О программе → Лицензии...»). Контрольные суммы файлов — в SHA256SUMS.txt.
EOF
