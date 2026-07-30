#!/bin/sh
set -eu

drm_alias=/dev/dri/cardputer-zero-internal
drm_device=$(basename "$(readlink -f "$drm_alias")")

case "$drm_device" in
    card[0-9]*) ;;
    *)
        echo "invalid internal DRM device: $drm_device" >&2
        exit 1
        ;;
esac

exec /usr/bin/weston \
    --backend=drm \
    --drm-device="$drm_device" \
    --seat=seat-cardputer-zero \
    --renderer=pixman \
    --shell=kiosk \
    --socket=wayland-0 \
    --idle-time=0 \
    --log=/run/cardputerzero/weston.log \
    -- weston-simple-shm
