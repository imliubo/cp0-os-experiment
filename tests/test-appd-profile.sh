#!/bin/bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
service="$repo_root/appd/systemd/cardputerzero-appd.service"
control="$repo_root/appd/systemd/cardputerzero-appd.socket"
broker="$repo_root/appd/systemd/cardputerzero-broker.socket"
tmpfiles="$repo_root/appd/systemd/cardputerzero-appd.conf"

grep -q '^Requires=.*cardputerzero-appd.socket.*cardputerzero-broker.socket' "$service"
grep -qx 'ReadWritePaths=/var/lib/cardputerzero/registry' "$service"
grep -qx 'RestrictAddressFamilies=AF_UNIX' "$service"
grep -qx 'SupplementaryGroups=cp0-wayland' "$service"
grep -qx 'FileDescriptorName=control' "$control"
grep -qx 'SocketMode=0660' "$control"
grep -qx 'FileDescriptorName=broker' "$broker"
grep -qx 'SocketMode=0666' "$broker"
grep -qx 'Service=cardputerzero-appd.service' "$broker"
grep -qx 'd /run/cardputerzero-broker 0711 root root -' "$tmpfiles"
