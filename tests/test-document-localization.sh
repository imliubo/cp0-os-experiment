#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
test_parent="$repo_root/target/test-tmp"
mkdir -p "$test_parent"
document_list=$(mktemp "$test_parent/cp0-document-localization.XXXXXX")
case "$document_list" in
    "$test_parent"/cp0-document-localization.*) ;;
    *)
        echo "error: unsafe document localization test path" >&2
        exit 1
        ;;
esac
trap 'rm -f -- "$document_list"' EXIT

(
    cd "$repo_root"
    git ls-files --cached --others --exclude-standard -- '*.md' \
        | LC_ALL=C sort
) >"$document_list"

documents=0
failures=0
while IFS= read -r relative; do
    [[ $relative == *.zh-CN.md ]] && continue
    documents=$((documents + 1))

    default="$repo_root/$relative"
    localized_relative="${relative%.md}.zh-CN.md"
    localized="$repo_root/$localized_relative"
    localized_name=$(basename "$localized_relative")
    default_name=$(basename "$relative")

    if [[ ! -s $localized ]]; then
        echo "error: missing Simplified Chinese document for $relative" >&2
        failures=$((failures + 1))
        continue
    fi
    if [[ $(sed -n '1,16p' "$default" \
        | grep -Fxc '<!-- doc-locale: en -->' || true) -ne 1 ]]; then
        echo "error: invalid English locale marker in $relative" >&2
        failures=$((failures + 1))
    fi
    if ! grep -Fqx "> **English** | [简体中文]($localized_name)" "$default"; then
        echo "error: invalid language switch in $relative" >&2
        failures=$((failures + 1))
    fi
    if [[ $(sed -n '1,16p' "$localized" \
        | grep -Fxc '<!-- doc-locale: zh-CN -->' || true) -ne 1 ]]; then
        echo "error: invalid Simplified Chinese locale marker in $localized_relative" >&2
        failures=$((failures + 1))
    fi
    if ! grep -Fqx "> [English]($default_name) | **简体中文**" "$localized"; then
        echo "error: invalid language switch in $localized_relative" >&2
        failures=$((failures + 1))
    fi
    if grep -Eq '系统壳|系统外壳|合成器|组合器' "$localized"; then
        echo "error: translated component identifier in $localized_relative" >&2
        failures=$((failures + 1))
    fi
    if [[ $relative != docs/LOCALIZATION.md ]] \
        && perl -Mutf8 -Mopen=:std,:encoding\(UTF-8\) -ne '
        if (/^\s*(```+|~~~+)/) { $in_fence = !$in_fence; next }
        next if $in_fence || /^> \*\*English\*\* \| \[简体中文\]/;
        s/`+[^`\n]*`+//g;
        $found = 1 if /[\x{3400}-\x{4dbf}\x{4e00}-\x{9fff}]/;
        END { exit($found ? 0 : 1) }
    ' "$default"; then
        echo "error: untranslated Chinese prose in default English document $relative" >&2
        failures=$((failures + 1))
    fi
done <"$document_list"

if ((failures != 0)); then
    echo "FAIL documentation localization ($failures error(s))" >&2
    exit 1
fi

echo "PASS documentation localization ($documents bilingual document pairs)"
