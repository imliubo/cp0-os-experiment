#!/bin/sh
set -eu

runtime_dir=${XDG_RUNTIME_DIR:-/run/cardputerzero}
display=${WAYLAND_DISPLAY:-wayland-0}
socket="$runtime_dir/$display"

attempt=0
while [ ! -S "$socket" ]; do
    attempt=$((attempt + 1))
    if [ "$attempt" -ge 100 ]; then
        echo "Wayland socket did not appear: $socket" >&2
        exit 1
    fi
    sleep 0.1
done
