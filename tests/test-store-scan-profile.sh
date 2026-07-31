#!/bin/sh
set -eu

repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
service="$repo/crates/cp0-store-scan-worker/systemd/cp0-store-scan-worker.service"
environment="$repo/crates/cp0-store-scan-worker/systemd/store-scan-worker.env.example"

test -f "$service"
test -f "$environment"
grep -qx 'User=cp0-store-scan' "$service"
grep -qx 'NoNewPrivileges=yes' "$service"
grep -qx 'PrivateDevices=yes' "$service"
grep -qx 'PrivateNetwork=yes' "$service"
grep -qx 'ProtectSystem=strict' "$service"
grep -qx 'MemoryDenyWriteExecute=yes' "$service"
grep -qx 'RestrictAddressFamilies=AF_UNIX' "$service"
grep -qx 'RestrictNamespaces=yes' "$service"
grep -qx 'SystemCallFilter=@system-service' "$service"
grep -qx 'ReadOnlyPaths=/var/lib/cardputerzero-store/objects' "$service"
grep -qx 'MemoryMax=256M' "$service"
grep -qx 'CPUQuota=100%' "$service"
grep -q 'host=/run/postgresql' "$environment"

if grep -Eq '^RestrictAddressFamilies=.*AF_INET' "$service"; then
    echo "error: Store scanner must not receive IP network access" >&2
    exit 1
fi
if grep -Eq '^ReadWritePaths=.*/objects' "$service"; then
    echo "error: Store scanner must not write the content object root" >&2
    exit 1
fi
