#!/bin/sh
set -eu

delay=${CP0_DISPLAY_RETRY_DELAY:-8}
sleep "$delay"

# This service is ordered before the System Shell. The SPI panel can miss its
# early fbdev initialization, so force a fresh DRM disable/enable and complete
# the ST7789 reset before any Setup or Home state becomes visible.
systemctl restart cardputerzero-compositor.service

attempt=0
while ! systemctl is-active --quiet cardputerzero-compositor.service; do
    attempt=$((attempt + 1))
    if [ "$attempt" -ge 40 ]; then
        echo "Compositor did not recover after the display retry" >&2
        exit 1
    fi
    sleep 0.25
done
