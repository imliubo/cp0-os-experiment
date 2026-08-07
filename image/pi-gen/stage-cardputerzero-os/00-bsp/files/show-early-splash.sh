#!/bin/sh
set -eu

effective_uid=${CP0_EARLY_SPLASH_UID:-$(id -u)}
if [ "$effective_uid" -ne 0 ]; then
    echo "error: early splash requires root" >&2
    exit 2
fi

sysfs_root=${CP0_EARLY_SPLASH_SYSFS_ROOT:-/sys}
device_root=${CP0_EARLY_SPLASH_DEVICE_ROOT:-/dev}
splash=${CP0_EARLY_SPLASH_FILE:-/usr/share/cardputerzero/boot/splash.rgb565}
sleep_command=${CP0_EARLY_SPLASH_SLEEP:-sleep}
max_attempts=${CP0_EARLY_SPLASH_ATTEMPTS:-80}
state_root=${CP0_EARLY_SPLASH_STATE_ROOT:-/run/cardputerzero-early-splash}
shown_marker="$state_root/framebuffer-shown"
render_lock="$state_root/framebuffer-rendering"
expected_bytes=108800
lock_held=0

release_lock() {
    if [ "$lock_held" -eq 1 ]; then
        rmdir "$render_lock" 2>/dev/null || true
        lock_held=0
    fi
}

trap release_lock 0 1 2 15

case "$max_attempts" in
    ''|*[!0-9]*|0)
        echo "error: invalid early splash attempt count: $max_attempts" >&2
        exit 2
        ;;
esac
if [ ! -f "$splash" ] || [ "$(wc -c <"$splash" | tr -d ' ')" -ne "$expected_bytes" ]; then
    echo "error: early splash must be a 320x170 RGB565 frame" >&2
    exit 1
fi
if [ -L "$state_root" ]; then
    echo "error: early splash state root must not be a symlink" >&2
    exit 1
fi
mkdir -p "$state_root"
[ -f "$shown_marker" ] && exit 0

attempt=1
while [ "$attempt" -le "$max_attempts" ]; do
    [ -f "$shown_marker" ] && exit 0
    for candidate in "$sysfs_root"/class/graphics/fb*; do
        [ -d "$candidate" ] || continue
        [ "$(cat "$candidate/name" 2>/dev/null || true)" = panel-mipi-dbid ] || continue
        [ "$(cat "$candidate/virtual_size" 2>/dev/null || true)" = 320,170 ] || continue
        [ "$(cat "$candidate/bits_per_pixel" 2>/dev/null || true)" = 16 ] || continue

        framebuffer=${candidate##*/}
        case "$framebuffer" in
            fb[0-9]*) ;;
            *) continue ;;
        esac
        framebuffer="$device_root/$framebuffer"
        [ -w "$framebuffer" ] || continue
        if ! mkdir "$render_lock" 2>/dev/null; then
            continue
        fi
        lock_held=1
        if [ -f "$shown_marker" ]; then
            release_lock
            exit 0
        fi
        if [ -w "$candidate/blank" ]; then
            printf '0\n' >"$candidate/blank"
        fi
        dd if="$splash" of="$framebuffer" bs="$expected_bytes" count=1 2>/dev/null
        : >"$shown_marker"
        release_lock
        echo "early-splash: displayed on ${candidate##*/}"
        exit 0
    done
    attempt=$((attempt + 1))
    "$sleep_command" 0.05
done

# Splash is cosmetic. A missing framebuffer must not replace the product shell
# with emergency mode; the display retry still handles the late DRM path.
echo "early-splash: LCD framebuffer unavailable after $max_attempts attempts" >&2
exit 0
