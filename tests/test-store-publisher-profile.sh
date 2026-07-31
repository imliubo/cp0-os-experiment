#!/bin/sh
set -eu

repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
service="$repo/crates/cp0-store-publisher/systemd/cp0-store-publisher.service"
environment="$repo/crates/cp0-store-publisher/systemd/store-publisher.env.example"

test -f "$service"
test -f "$environment"
grep -qx 'User=cp0-store-publisher' "$service"
grep -qx 'NoNewPrivileges=yes' "$service"
grep -qx 'PrivateDevices=yes' "$service"
grep -qx 'PrivateNetwork=yes' "$service"
grep -qx 'ProtectSystem=strict' "$service"
grep -qx 'MemoryDenyWriteExecute=yes' "$service"
grep -qx 'RestrictAddressFamilies=AF_UNIX' "$service"
grep -qx 'RestrictNamespaces=yes' "$service"
grep -qx 'SystemCallFilter=@system-service' "$service"
grep -qx 'ReadOnlyPaths=/var/lib/cardputerzero-store/objects' "$service"
grep -qx 'ReadOnlyPaths=/etc/cardputerzero/store-signing.key' "$service"
grep -qx 'ReadWritePaths=/var/lib/cardputerzero-store/origin' "$service"
grep -qx 'MemoryMax=192M' "$service"
grep -qx 'CPUQuota=100%' "$service"
grep -q 'host=/run/postgresql' "$environment"

if grep -Eq '^RestrictAddressFamilies=.*AF_INET' "$service"; then
    echo "error: Store Publisher must not receive IP network access" >&2
    exit 1
fi
if grep -Eq '^ReadWritePaths=.*/objects' "$service"; then
    echo "error: Store Publisher must not write the Submission object root" >&2
    exit 1
fi
if grep -Eq '^ReadWritePaths=.*/store-signing\.key' "$service"; then
    echo "error: Store Publisher must not modify its signing key" >&2
    exit 1
fi
