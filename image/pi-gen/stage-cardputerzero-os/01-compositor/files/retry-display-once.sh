#!/bin/sh
set -eu

delay=${CP0_DISPLAY_RETRY_DELAY:-1}
sleep "$delay"

# The SPI panel can miss its early fbdev initialization. A compositor restart
# forces a fresh DRM disable/enable and therefore a full ST7789 reset sequence.
systemctl restart cardputerzero-compositor.service

attempt=0
while ! systemctl is-active --quiet cardputerzero-system-shell.service; do
    attempt=$((attempt + 1))
    if [ "$attempt" -ge 40 ]; then
        echo "System Shell did not recover after the display retry" >&2
        exit 1
    fi
    sleep 0.25
done
