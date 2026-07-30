#!/bin/bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
service="$repo_root/appd/systemd/cardputerzero-appd.service"
control="$repo_root/appd/systemd/cardputerzero-appd.socket"
broker="$repo_root/appd/systemd/cardputerzero-broker.socket"
tmpfiles="$repo_root/appd/systemd/cardputerzero-appd.conf"
network_service="$repo_root/appd/systemd/cardputerzero-networkd.service"
network_socket="$repo_root/appd/systemd/cardputerzero-networkd.socket"

grep -q '^Requires=.*cardputerzero-appd.socket.*cardputerzero-broker.socket.*cardputerzero-networkd.socket' "$service"
grep -qx 'ReadWritePaths=/var/lib/cardputerzero/registry' "$service"
grep -qx 'RestrictAddressFamilies=AF_UNIX' "$service"
grep -qx 'SupplementaryGroups=cp0-wayland' "$service"
grep -qx 'FileDescriptorName=control' "$control"
grep -qx 'SocketMode=0660' "$control"
grep -qx 'FileDescriptorName=broker' "$broker"
grep -qx 'SocketMode=0666' "$broker"
grep -qx 'Service=cardputerzero-appd.service' "$broker"
grep -qx 'd /run/cardputerzero-broker 0711 root root -' "$tmpfiles"
grep -qx 'd /run/cardputerzero-networkd 0755 root root -' "$tmpfiles"
grep -qx 'User=cp0-network' "$network_service"
grep -qx 'RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6' "$network_service"
grep -qx 'NoNewPrivileges=yes' "$network_service"
grep -qx 'FileDescriptorName=network' "$network_socket"
grep -qx 'SocketMode=0600' "$network_socket"
grep -qx 'Service=cardputerzero-networkd.service' "$network_socket"
