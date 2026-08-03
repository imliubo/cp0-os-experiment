#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
protocol="$repo_root/crates/cp0-power-protocol/src/lib.rs"
daemon="$repo_root/crates/cp0-powerd/src/lib.rs"
daemon_main="$repo_root/crates/cp0-powerd/src/main.rs"
service="$repo_root/appd/systemd/cardputerzero-powerd.service"
socket="$repo_root/appd/systemd/cardputerzero-powerd.socket"
tmpfiles="$repo_root/appd/systemd/cardputerzero-power.conf"
shell_service="$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/files/cardputerzero-system-shell.service"
shell_main="$repo_root/system-shell/src/main.c"
stage="$repo_root/image/pi-gen/stage-cardputerzero-os/02-app-platform/01-run.sh"

grep -Fq 'PowerCommand::Restart {}' "$daemon"
grep -Fq 'PowerCommand::PowerOff {}' "$daemon"
grep -Fq 'DEFAULT_SYSTEMCTL_PATH: &str = "/usr/bin/systemctl"' "$daemon"
grep -Fq 'PowerAction::Restart => ["--no-block", "reboot"]' "$daemon"
grep -Fq 'PowerAction::PowerOff => ["--no-block", "poweroff"]' "$daemon"
grep -Fq '.args(systemd_arguments(action))' "$daemon"
grep -Fq 'trusted_uids.contains' "$daemon"
grep -Fq 'libc::SO_PEERCRED' "$daemon"
grep -Fq 'user_uid(c"cp0-shell")' "$daemon_main"
grep -Fq '#[serde(tag = "name", rename_all = "kebab-case", deny_unknown_fields)]' "$protocol"

grep -qx 'User=root' "$service"
grep -qx 'ExecStart=/usr/libexec/cardputerzero/cp0-powerd serve' "$service"
grep -qx 'NoNewPrivileges=yes' "$service"
grep -qx 'CapabilityBoundingSet=' "$service"
grep -qx 'RestrictAddressFamilies=AF_UNIX' "$service"
grep -qx 'MemoryDenyWriteExecute=yes' "$service"
if grep -Eq 'sudo|/bin/(sh|bash)|systemctl[[:space:]]+(start|stop|restart)' "$service" "$daemon"; then
    echo "error: power broker contains a general privilege path" >&2
    exit 1
fi

grep -qx 'FileDescriptorName=power' "$socket"
grep -qx 'SocketGroup=cp0-power-control' "$socket"
grep -qx 'SocketMode=0660' "$socket"
grep -qx 'd /run/cardputerzero-powerd 0750 root cp0-power-control -' "$tmpfiles"
grep -q 'cp0-power-control' "$shell_service"
grep -Fq 'cp0_power_request(CP0_POWER_RESTART)' "$shell_main"
grep -Fq 'cp0_power_request(CP0_POWER_OFF)' "$shell_main"
if grep -Fq 'broker unavailable' "$shell_main"; then
    echo "error: System Shell still contains the placeholder power action" >&2
    exit 1
fi

grep -Fq 'groupadd --system cp0-power-control' "$stage"
grep -Fq 'usermod -a -G cp0-power-control cp0-shell' "$stage"
grep -Fq 'cardputerzero-powerd.socket' "$stage"
grep -Fq 'cardputerzero-powerd.service cardputerzero-powerd.socket' "$stage"

printf '%s\n' 'power control boundary tests passed'
