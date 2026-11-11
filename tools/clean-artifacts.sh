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
#   - target/ — обычно 6–15 ГБ: сборки Linux и Windows, кэш зависимостей,
#     PNG-превью настроек и журналы рядом с собранным exe. КРОМЕ
#     target/linux-dist/: там готовые .deb и AppImage выпуска, их загружают
#     в релиз вручную;
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
# ПОКАЗЫВАЕТСЯ, но удаляется только по --tmp:
#   - tmp/ — черновики, журналы проверок и отдельные target-каталоги агентов
#     (бывало до 40 ГБ). В архив не идёт никогда. По --force --tmp удаляется
#     всё, кроме того, что нужно для выпуска и качается из сети:
#       tmp/linux-build-noble — корень Ubuntu 24.04 для сборки Linux-выпуска;
#       tmp/linux-browser     — браузер для проверок ввода (fetch-linux-test-browser.py);
#       tmp/appimage-runtime  — среда запуска AppImage для упаковки.
#     Часть файлов создана проверками от root: их скрипт не удалит и скажет,
#     что нужен sudo.
#
# Использование:
#   tools/clean-artifacts.sh                     # ПРОБНЫЙ ПРОГОН — что уйдёт и сколько это
#   tools/clean-artifacts.sh --force             # собственно удалить артефакты
#   tools/clean-artifacts.sh --force --sources   # ...вместе с загруженными корпусами
#   tools/clean-artifacts.sh --force --tmp       # ...и временные файлы в tmp/
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

# Готовые пакеты выпуска внутри target/: остаются при чистке.
KEEP_IN_TARGET=("linux-dist")

# Что остаётся в tmp/ при --tmp: нужно для выпуска, качается из сети.
TMP_KEEP=("linux-build-noble" "linux-browser" "appimage-runtime")

# Мусор ОС и редактора (ищется по всему дереву).
JUNK_GLOBS=(
  "*.DS_Store" "Thumbs.db" "*Zone.Identifier" "*.swp" "*.swo" "*.orig" "*.rej"
  "*~" "*.rs.bk" "*.pyc" "*.pdb" "okbswitch.*.log" ".okbswitch-write-test"
)

# Не попадает В АРХИВ, но --force это не трогает (tmp/ — только по --tmp).
ARCHIVE_EXTRA_EXCLUDES=("tmp" ".idea" ".vscode" "*.iml")

human() { du -sh "$1" 2>/dev/null | cut -f1; }
# Размер в КБ. du возвращает ошибку, если внутри есть закрытые каталоги (файлы
# проверок от root в tmp/), но итог всё равно печатает; под pipefail такая
# ошибка в присваивании молча оборвала бы скрипт.
kb_of() { { du -sk "$1" 2>/dev/null || true; } | cut -f1; }
# КБ → «12K», «340M», «13G» для подсчётов, где du вызвать нельзя (разность).
fmt_kb() {
  if [ "$1" -ge 1048576 ]; then echo "$(( $1 / 1048576 ))G"
  elif [ "$1" -ge 1024 ]; then echo "$(( $1 / 1024 ))M"
  else echo "${1}K"; fi
}

usage() { sed -n '2,/^set -euo/p' "$SELF" | grep '^#' | sed 's/^# \{0,1\}//'; exit 0; }

# find по дереву без target/.git/data/sources/tmp — иначе обход десятков гигабайт
# ради десятка файлов мусора. В tmp/ лежит корень Ubuntu для сборки выпуска и
# файлы проверок от root: их «мусор» — файлы чужой системы, а закрытый каталог
# заставил бы find вернуть ошибку, и pipefail молча оборвал бы весь прогон.
prune_find() {
  find . -path ./.git -prune -o -path ./target -prune -o -path ./data/sources -prune \
       -o -path ./tmp -prune -o "$@" || true
}

# ── разбор аргументов ────────────────────────────────────────────────────────
MODE="dry"          # dry | force | archive
WITH_GIT=0
WITH_SOURCES=0
WITH_TMP=0
tmp_failed=()
OUT=""
while [ $# -gt 0 ]; do
  case "$1" in
    --force|-f)   MODE="force" ;;
    --archive|-a) MODE="archive" ;;
    --with-git)   WITH_GIT=1 ;;
    --sources)    WITH_SOURCES=1 ;;
    --tmp)        WITH_TMP=1 ;;
    -h|--help)    usage ;;
    -*)           echo "неизвестный ключ: $1" >&2; exit 2 ;;
    *)            OUT="$1" ;;   # позиционный → путь архива
  esac
  shift
done

if { [ "$WITH_SOURCES" -eq 1 ] || [ "$WITH_TMP" -eq 1 ]; } && [ "$MODE" != "force" ]; then
  echo "--sources и --tmp работают только вместе с --force" >&2
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
  echo "  (tmp/ не входит: черновики, проверки и корень Ubuntu для сборки)"
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
    if [ "$d" = "target" ]; then
      # Готовые пакеты выпуска остаются.
      kb=$(kb_of target)
      kept=""
      keep_args=()
      for k in "${KEEP_IN_TARGET[@]}"; do
        keep_args+=(! -name "$k")
        [ -d "target/$k" ] || continue
        kb=$(( kb - $(kb_of "target/$k") ))
        kept="$kept target/$k/ ($(human "target/$k"))"
      done
      if [ -n "$kept" ]; then
        note "$(fmt_kb "$kb")" "target/ (остаются:$kept)"
      else
        note "$(human target)" "target/"
      fi
      total_kb=$(( total_kb + kb ))
      [ "$MODE" = "force" ] && find target -mindepth 1 -maxdepth 1 "${keep_args[@]}" -exec rm -rf -- {} +
      continue
    fi
    note "$(human "$d")" "$d/"
    total_kb=$(( total_kb + $(kb_of "$d") ))
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
    total_kb=$(( total_kb + $(kb_of "$d") ))
    [ "$MODE" = "force" ] && rm -rf -- "$d"
  else
    echo
    echo "Скачанные корпуса (НЕ удаляются без --sources, в архив не идут):"
    note "$(human "$d")" "$d/"
    echo "  вернуть: tools/fetch-lang-sources.sh"
  fi
done

# Временные файлы: по умолчанию только показываем.
if [ -d tmp ]; then
  tmp_kb=0
  tmp_list=()
  for e in tmp/* tmp/.[!.]*; do
    [ -e "$e" ] || continue
    keep=0
    for k in "${TMP_KEEP[@]}"; do [ "$e" = "tmp/$k" ] && keep=1; done
    [ "$keep" -eq 1 ] && continue
    tmp_list+=("$e")
    tmp_kb=$(( tmp_kb + $(kb_of "$e") ))
  done
  if [ "${#tmp_list[@]}" -gt 0 ]; then
    if [ "$WITH_TMP" -eq 1 ]; then
      found=1
      note "$(fmt_kb "$tmp_kb")" "tmp/ — ${#tmp_list[@]} шт. (по --tmp; остаются ${TMP_KEEP[*]})"
      total_kb=$(( total_kb + tmp_kb ))
      if [ "$MODE" = "force" ]; then
        for e in "${tmp_list[@]}"; do rm -rf -- "$e" 2>/dev/null || tmp_failed+=("$e"); done
      fi
    else
      echo
      echo "Временные файлы (НЕ удаляются без --tmp, в архив не идут):"
      note "$(fmt_kb "$tmp_kb")" "tmp/ — ${#tmp_list[@]} шт., кроме ${TMP_KEEP[*]}"
    fi
  fi
fi

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
  if [ "${#tmp_failed[@]}" -gt 0 ]; then
    echo
    echo "Не удалось удалить из tmp/ (файлы от root, созданные проверками):"
    for e in "${tmp_failed[@]}"; do echo "  $e"; done
    echo "  удалить их: sudo rm -rf ${tmp_failed[*]}"
  fi
elif [ "$found" -ne 0 ]; then
  echo
  echo "Это был ПРОБНЫЙ ПРОГОН — ничего не удалено."
  echo "  удалить:            tools/clean-artifacts.sh --force [--tmp] [--sources]"
  echo "  вместо этого архив: tools/clean-artifacts.sh --archive"
fi
