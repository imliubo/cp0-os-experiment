#!/bin/sh
set -eu

runtime_dir=${XDG_RUNTIME_DIR:-/run/cardputerzero}
display=${WAYLAND_DISPLAY:-wayland-0}
socket="$runtime_dir/$display"
boot_splash_ready=${CP0_BOOT_SPLASH_READY_PATH:-$runtime_dir/boot-splash-ready}
backlight_path=${CP0_BACKLIGHT_BRIGHTNESS_PATH:-/sys/class/backlight/backlight/brightness}
backlight_max_path=${CP0_BACKLIGHT_MAX_PATH:-/sys/class/backlight/backlight/max_brightness}
backlight_state_path=${CP0_BACKLIGHT_STATE_PATH:-$runtime_dir/backlight-before-sleep}

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

# Weston autolaunches a trusted splash surface. Give that surface the first
# scanout opportunity before restoring a previously-zero backlight. A failed
# marker is non-fatal so compositor recovery cannot strand the display off.
attempt=0
while [ ! -f "$boot_splash_ready" ]; do
    attempt=$((attempt + 1))
    if [ "$attempt" -ge 60 ]; then
        echo "Boot splash frame was not acknowledged: $boot_splash_ready" >&2
        break
    fi
    sleep 0.05
done

if [ -e "$backlight_path" ]; then
    if [ ! -r "$backlight_path" ] || [ ! -w "$backlight_path" ] || \
       [ ! -r "$backlight_max_path" ]; then
        echo "LCD backlight control is not accessible: $backlight_path" >&2
        exit 1
    fi
    brightness=$(tr -d '[:space:]' <"$backlight_path")
    maximum=$(tr -d '[:space:]' <"$backlight_max_path")
    case "$brightness" in
        ''|*[!0-9]*)
            echo "LCD backlight brightness is invalid: $brightness" >&2
            exit 1
            ;;
    esac
    case "$maximum" in
        ''|*[!0-9]*|0)
            echo "LCD backlight maximum is invalid: $maximum" >&2
            exit 1
            ;;
    esac
    if [ "$brightness" -eq 0 ]; then
        saved=''
        if [ -r "$backlight_state_path" ]; then
            saved=$(tr -d '[:space:]' <"$backlight_state_path")
        fi
        case "$saved" in
            ''|*[!0-9]*|0) saved=$(((maximum + 1) / 2)) ;;
        esac
        if [ "$saved" -gt "$maximum" ]; then
            saved=$maximum
        fi
        printf '%s\n' "$saved" >"$backlight_path"
    fi
fi

if [ ! -w "$blank_path" ]; then
    echo "LCD framebuffer blank control is not writable: $blank_path" >&2
    exit 1
fi

printf '0\n' > "$blank_path"
