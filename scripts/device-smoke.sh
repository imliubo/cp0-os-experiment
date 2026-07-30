#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
exec "$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/files/device-smoke.sh" "$@"

