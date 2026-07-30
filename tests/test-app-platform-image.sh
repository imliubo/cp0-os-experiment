#!/bin/bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
build="$repo_root/image/build-image.sh"
stage="$repo_root/image/pi-gen/stage-cardputerzero-os/02-app-platform/01-run.sh"
compositor="$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/01-run.sh"

grep -q 'scripts/build-appd.sh' "$build"
grep -q 'scripts/build-app-runtime.sh' "$build"
grep -q 'scripts/build-example-app.sh' "$build"
grep -q '02-app-platform/payload' "$build"
grep -q 'cp0-appd register-installed' "$stage"
grep -q 'useradd --system --uid 20000' "$stage"
grep -q 'cardputerzero-appd.socket cardputerzero-broker.socket' "$stage"
grep -q 'chown -R root:root' "$stage"
grep -q 'cp0-app-20000 -g cp0-app-20000 -m 0700' "$stage"
grep -qx 'systemctl enable cardputerzero-compositor.service' "$compositor"
