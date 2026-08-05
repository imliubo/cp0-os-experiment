#!/bin/sh
set -eu

not_before_ms=${CP0_DISPLAY_RETRY_NOT_BEFORE_MS:-8000}
uptime_file=${CP0_DISPLAY_RETRY_UPTIME_FILE:-/proc/uptime}
sleep_command=${CP0_DISPLAY_RETRY_SLEEP:-sleep}
systemctl_command=${CP0_DISPLAY_RETRY_SYSTEMCTL:-systemctl}

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

# This service is ordered before the System Shell. The SPI panel can miss its
# early fbdev initialization, so force a fresh DRM disable/enable and complete
# the ST7789 reset before any Setup or Home state becomes visible.
"$systemctl_command" restart cardputerzero-compositor.service

attempt=0
while ! "$systemctl_command" is-active --quiet cardputerzero-compositor.service; do
    attempt=$((attempt + 1))
    if [ "$attempt" -ge 40 ]; then
        echo "Compositor did not recover after the display retry" >&2
        exit 1
    fi
    sleep 0.25
done
