#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
    echo "usage: collect-keyboard-diagnostics.sh user@device output.log" >&2
    exit 2
fi

device=$1
output=$2
case "$device" in
    *[!A-Za-z0-9@._:\[\]-]* | -* | '')
        echo "error: invalid SSH device target" >&2
        exit 2
        ;;
esac

ssh -- "$device" sudo cat \
    /var/lib/cardputerzero/data/dev.cardputerzero.keyboard-diagnostics/keyboard-test.log \
    >"$output"
test -s "$output"
printf 'keyboard diagnostics log: %s\n' "$output"
