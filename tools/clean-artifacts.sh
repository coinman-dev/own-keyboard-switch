#!/usr/bin/env bash
#
# clean-artifacts.sh — убрать из дерева результаты сборки, журналы и мусор ОС,
# чтобы архив для передачи нёс только исходники и то, без чего не собрать
# бинарники.
#
# ХРАНИТСЯ (не трогаем):
#   - весь код, Cargo.toml/Cargo.lock, tools/, packaging/, .github/;
#   - data/generated/ — готовые языковые модели и словари, 1,9 МБ. Они лежат
#     в git, программа встраивает их в бинарник, и без них не собрать вообще
#     ничего. Пересборка требует сети и data/sources/;
#   - data/hunspell/, data/LICENSES.md — лицензии встроенных словарей, их
#     печатает `okbswitch --licenses`;
#   - .ai/ — планы и журнал работ, .git — история.
#
# УДАЛЯЕТСЯ по --force (пересоберётся само):
#   - target/ — обычно 6–7 ГБ: сборки Linux и Windows, кэш зависимостей,
#     PNG-превью настроек и журналы рядом с собранным exe;
#   - log/ в корне — журналы, скопированные с Windows для разбора;
#   - мусор ОС и редактора: *Zone.Identifier (их плодит Проводник Windows
#     при копировании в WSL), .DS_Store, Thumbs.db, *.swp, *.orig, *.rs.bk,
#     __pycache__, забытые okbswitch.*.log где угодно в дереве.
#
# ПОКАЗЫВАЕТСЯ, но удаляется только по --sources:
#   - data/sources/ — 233 МБ исходных корпусов Tatoeba и словарей Hunspell.
#     Не в git и в архив не идёт. Восстанавливается tools/fetch-lang-sources.sh,
#     но это сеть и внешние сайты, поэтому решение за человеком.
#
# Использование:
#   tools/clean-artifacts.sh                     # ПРОБНЫЙ ПРОГОН — что уйдёт и сколько это
#   tools/clean-artifacts.sh --force             # собственно удалить артефакты
#   tools/clean-artifacts.sh --force --sources   # ...вместе с загруженными корпусами
#   tools/clean-artifacts.sh --archive [OUT]     # собрать лёгкий .tar.gz (ничего не удаляя)
#                                                # по умолчанию — в ТЕКУЩИЙ каталог
#   tools/clean-artifacts.sh --archive --with-git [OUT]   # ...вместе с историей .git
#   tools/clean-artifacts.sh --help
#
set -euo pipefail

INVOKE_DIR="$(pwd)"          # откуда запустили — туда и ляжет архив
# Абсолютный путь к самому скрипту — ЗАПОМНИТЬ ДО перехода в корень: `--help`
# читает справку из собственного текста, а после `cd` относительный $0
# («clean-artifacts.sh», если запускали из tools/) уже никуда не ведёт.
SELF="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BASE="$(basename "$ROOT")"
cd "$ROOT"

# Чистый результат сборки и кэши (пути от корня репозитория).
ARTIFACT_DIRS=(
  "target"
  "log"
  "tools/__pycache__"
)

# Скачанные корпуса: восстановимы, но только из сети.
HEAVY_REGENERABLE=("data/sources")

# Мусор ОС и редактора (ищется по всему дереву).
JUNK_GLOBS=(
  "*.DS_Store" "Thumbs.db" "*Zone.Identifier" "*.swp" "*.swo" "*.orig" "*.rej"
  "*~" "*.rs.bk" "*.pyc" "*.pdb" "okbswitch.*.log" ".okbswitch-write-test"
)

# Не попадает В АРХИВ, но --force это не трогает.
ARCHIVE_EXTRA_EXCLUDES=(".idea" ".vscode" "*.iml")

human() { du -sh "$1" 2>/dev/null | cut -f1; }

usage() { sed -n '2,/^set -euo/p' "$SELF" | grep '^#' | sed 's/^# \{0,1\}//'; exit 0; }

# find по дереву без target/.git/data/sources — иначе обход семи гигабайт ради
# десятка файлов мусора.
prune_find() {
  find . -path ./.git -prune -o -path ./target -prune -o -path ./data/sources -prune \
       -o "$@"
}

# ── разбор аргументов ────────────────────────────────────────────────────────
MODE="dry"          # dry | force | archive
WITH_GIT=0
WITH_SOURCES=0
OUT=""
while [ $# -gt 0 ]; do
  case "$1" in
    --force|-f)   MODE="force" ;;
    --archive|-a) MODE="archive" ;;
    --with-git)   WITH_GIT=1 ;;
    --sources)    WITH_SOURCES=1 ;;
    -h|--help)    usage ;;
    -*)           echo "неизвестный ключ: $1" >&2; exit 2 ;;
    *)            OUT="$1" ;;   # позиционный → путь архива
  esac
  shift
done

if [ "$WITH_SOURCES" -eq 1 ] && [ "$MODE" != "force" ]; then
  echo "--sources работает только вместе с --force" >&2
  exit 2
fi

# ── режим архива ─────────────────────────────────────────────────────────────
if [ "$MODE" = "archive" ]; then
  ts="$(date +%Y%m%d-%H%M%S)"
  if [ -z "$OUT" ]; then
    OUT="$INVOKE_DIR/${BASE}-${ts}.tar.gz"
  elif [ "${OUT#/}" = "$OUT" ]; then
    OUT="$INVOKE_DIR/$OUT"                                    # относительный — от каталога запуска
  fi

  excludes=(--exclude='*.tar.gz')       # не паковать архив в самого себя и не тащить старые
  for d in "${ARTIFACT_DIRS[@]}";      do excludes+=(--exclude="$BASE/$d"); done
  for d in "${HEAVY_REGENERABLE[@]}";  do excludes+=(--exclude="$BASE/$d"); done
  for g in "${JUNK_GLOBS[@]}";         do excludes+=(--exclude="$g"); done
  for e in "${ARCHIVE_EXTRA_EXCLUDES[@]}"; do
    case "$e" in
      *.*|.*) excludes+=(--exclude="$e") ;;                   # шаблон (*.iml) или скрытый каталог (.idea)
    esac
    excludes+=(--exclude="$BASE/$e")                          # и копия в корне репозитория
  done
  [ "$WITH_GIT" -eq 0 ] && excludes+=(--exclude="$BASE/.git")

  echo "Собираю архив (код, .ai/, packaging/, tools/ и data/generated/ —"
  echo "без готовых моделей проект не собрать, поэтому они входят):"
  echo "  -> $OUT"
  [ "$WITH_GIT" -eq 0 ] && echo "  (история .git не входит — нужна, добавьте --with-git)"
  echo "  (data/sources/ не входит: 233 МБ, качается tools/fetch-lang-sources.sh)"
  tar -C "$(dirname "$ROOT")" -czf "$OUT" "${excludes[@]}" "$BASE"
  echo "Готово. Размер архива: $(human "$OUT")"
  exit 0
fi

# ── чистка / пробный прогон ──────────────────────────────────────────────────
total_kb=0
found=0
note() { printf '  %-9s %s\n' "$1" "$2"; }

echo "Артефакты под $ROOT:"
for d in "${ARTIFACT_DIRS[@]}"; do
  if [ -d "$d" ]; then
    found=1
    note "$(human "$d")" "$d/"
    total_kb=$(( total_kb + $(du -sk "$d" | cut -f1) ))
    [ "$MODE" = "force" ] && rm -rf -- "$d"
  fi
done

# Мусор ОС и редактора: считаем, по --force удаляем.
junk_args=()
for g in "${JUNK_GLOBS[@]}"; do junk_args+=(-name "$g" -o); done
unset 'junk_args[${#junk_args[@]}-1]'   # убрать хвостовой -o
junk_count=$(prune_find -type f \( "${junk_args[@]}" \) -print 2>/dev/null | wc -l | tr -d ' ')
if [ "$junk_count" -gt 0 ]; then
  found=1
  note "" "$junk_count файл(ов) мусора ОС/редактора (Zone.Identifier, .DS_Store, swap …)"
  [ "$MODE" = "force" ] && prune_find -type f \( "${junk_args[@]}" \) -print0 2>/dev/null | xargs -0 -r rm -f
fi
if [ -d "tools/__pycache__" ] || prune_find -type d -name __pycache__ -print -quit 2>/dev/null | grep -q .; then
  [ "$MODE" = "force" ] && prune_find -type d -name __pycache__ -print0 2>/dev/null | xargs -0 -r rm -rf
fi

# Скачанные корпуса: по умолчанию только показываем.
for d in "${HEAVY_REGENERABLE[@]}"; do
  [ -d "$d" ] || continue
  if [ "$WITH_SOURCES" -eq 1 ]; then
    found=1
    note "$(human "$d")" "$d/ (по --sources)"
    total_kb=$(( total_kb + $(du -sk "$d" | cut -f1) ))
    [ "$MODE" = "force" ] && rm -rf -- "$d"
  else
    echo
    echo "Скачанные корпуса (НЕ удаляются без --sources, в архив не идут):"
    note "$(human "$d")" "$d/"
    echo "  вернуть: tools/fetch-lang-sources.sh"
  fi
done

if [ "$found" -eq 0 ]; then
  echo "  (чисто — удалять нечего)"
else
  echo
  if [ "$total_kb" -ge 1048576 ]; then
    printf 'Освободится: ~%s ГБ\n' "$(( total_kb / 1048576 ))"
  elif [ "$total_kb" -ge 1024 ]; then
    printf 'Освободится: ~%s МБ\n' "$(( total_kb / 1024 ))"
  else
    printf 'Освободится: ~%s КБ\n' "$total_kb"
  fi
fi

if [ "$MODE" = "force" ]; then
  echo
  echo "Удалено. ✓  (код, data/generated/, .ai/ на месте)"
  echo "  вернуть сборку:    cargo build"
  echo "  сборка Windows:    tools/cross-windows.sh build --release -p okbswitch"
  [ "$WITH_SOURCES" -eq 1 ] && echo "  вернуть корпуса:   tools/fetch-lang-sources.sh"
elif [ "$found" -ne 0 ]; then
  echo
  echo "Это был ПРОБНЫЙ ПРОГОН — ничего не удалено."
  echo "  удалить:            tools/clean-artifacts.sh --force"
  echo "  вместо этого архив: tools/clean-artifacts.sh --archive"
fi
