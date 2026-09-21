#!/usr/bin/env bash
# Downloads the sources of the language data into data/sources/ (not committed).
# Afterwards run: cargo run --release -p build-lang-data
#
# Sources and licenses (see data/LICENSES.md):
#   * Hunspell ru_RU (Alexander I. Lebedev, BSD-style) and en_US (SCOWL, permissive)
#     from the LibreOffice dictionaries repository.
#   * Tatoeba sentences, CC-BY 2.0 FR, https://tatoeba.org
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
src="$root/data/sources"
mkdir -p "$src"
cd "$src"

lo="https://raw.githubusercontent.com/LibreOffice/dictionaries/master"
fetch() {
    local url="$1" out="$2"
    if [[ -s "$out" ]]; then
        echo "have   $out"
    else
        echo "fetch  $out"
        curl -fsSL --retry 3 -o "$out.part" "$url"
        mv "$out.part" "$out"
    fi
}

fetch "$lo/ru_RU/ru_RU.aff" ru_RU.aff
fetch "$lo/ru_RU/ru_RU.dic" ru_RU.dic
fetch "$lo/ru_RU/README_ru_RU.txt" README_ru_RU.txt
fetch "$lo/en/en_US.aff" en_US.aff
fetch "$lo/en/en_US.dic" en_US.dic
fetch "$lo/en/README_en_US.txt" README_en_US.txt

for lang in rus eng; do
    fetch "https://downloads.tatoeba.org/exports/per_language/$lang/${lang}_sentences.tsv.bz2" "${lang}_sentences.tsv.bz2"
    if [[ ! -s "${lang}_sentences.tsv" ]]; then
        echo "unpack ${lang}_sentences.tsv"
        bunzip2 -kc "${lang}_sentences.tsv.bz2" > "${lang}_sentences.tsv"
    fi
done

cp README_ru_RU.txt README_en_US.txt "$root/data/hunspell/"
echo "done: $src"
