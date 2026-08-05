#!/bin/sh
set -eu

fb_alias=${CP0_RECOVERY_FB_ALIAS:-/dev/fb_lcd}
con2fbmap_command=${CP0_RECOVERY_CON2FBMAP:-/usr/bin/con2fbmap}
sleep_command=${CP0_RECOVERY_SLEEP:-/usr/bin/sleep}

attempt=0
while [ ! -e "$fb_alias" ] && [ ! -L "$fb_alias" ]; do
    attempt=$((attempt + 1))
    if [ "$attempt" -ge 100 ]; then
        echo "LCD framebuffer alias did not appear: $fb_alias" >&2
        exit 1
    fi
    "$sleep_command" 0.1
done

fb_device=$(basename "$(readlink -f "$fb_alias")")
case "$fb_device" in
    fb*) fb_index=${fb_device#fb} ;;
    *)
        echo "Invalid LCD framebuffer alias: $fb_device" >&2
        exit 1
        ;;
esac
case "$fb_index" in
    '' | *[!0-9]*)
        echo "Invalid LCD framebuffer alias: $fb_device" >&2
        exit 1
        ;;
esac

exec "$con2fbmap_command" 1 "$fb_index"
