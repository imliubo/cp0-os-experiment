#!/bin/sh
set -eu

not_before_ms=${CP0_DISPLAY_RETRY_NOT_BEFORE_MS:-8000}
uptime_file=${CP0_DISPLAY_RETRY_UPTIME_FILE:-/proc/uptime}
sleep_command=${CP0_DISPLAY_RETRY_SLEEP:-sleep}

case "$not_before_ms" in
    ''|*[!0-9]*)
        echo "Invalid display retry minimum uptime: $not_before_ms" >&2
        exit 2
        ;;
esac
uptime_ms=$(awk 'NR == 1 { printf "%.0f", $1 * 1000; found=1 } END { exit !found }' \
    "$uptime_file")
case "$uptime_ms" in
    ''|*[!0-9]*)
        echo "Invalid system uptime for display retry: $uptime_ms" >&2
        exit 1
        ;;
esac
if [ "$uptime_ms" -lt "$not_before_ms" ]; then
    remaining_ms=$((not_before_ms - uptime_ms))
    remaining_seconds=$((remaining_ms / 1000))
    remaining_fraction=$((remaining_ms % 1000))
    delay=$(printf '%u.%03u' "$remaining_seconds" "$remaining_fraction")
    "$sleep_command" "$delay"
fi

# Keep the initramfs/framebuffer splash in panel RAM until the cold-boot LCD
# stabilization point, then let Weston take ownership exactly once. Restarting
# Weston here caused a visible black interval and discarded its first frame.
