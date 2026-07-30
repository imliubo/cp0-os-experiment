#!/bin/sh
set -eu

runtime_dir=${XDG_RUNTIME_DIR:-/run/cardputerzero}
display=${WAYLAND_DISPLAY:-wayland-0}
socket="$runtime_dir/$display"

if [ -n "${CP0_FB_BLANK_PATH:-}" ]; then
    blank_path=$CP0_FB_BLANK_PATH
else
    fb_device=$(basename "$(readlink -f /dev/fb_lcd)")
    case "$fb_device" in
        fb[0-9]*) ;;
        *)
            echo "Invalid LCD framebuffer alias: $fb_device" >&2
            exit 1
            ;;
    esac
    blank_path="/sys/class/graphics/$fb_device/blank"
fi

attempt=0
while [ ! -S "$socket" ]; do
    attempt=$((attempt + 1))
    if [ "$attempt" -ge 100 ]; then
        echo "Wayland socket did not appear before display unblank: $socket" >&2
        exit 1
    fi
    sleep 0.1
done

if [ ! -w "$blank_path" ]; then
    echo "LCD framebuffer blank control is not writable: $blank_path" >&2
    exit 1
fi

printf '0\n' > "$blank_path"
