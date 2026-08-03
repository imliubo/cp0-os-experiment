#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
devd="$repo_root/crates/cp0-devd/src/main.rs"
protocol="$repo_root/crates/cp0-devd/src/lib.rs"
remote="$repo_root/crates/cp0ctl/src/remote.rs"
owner_shell="$repo_root/appd/systemd/cardputerzero-owner-shell"
socket="$repo_root/appd/systemd/cardputerzero-devd.socket"
service="$repo_root/appd/systemd/cardputerzero-devd.service"
stage="$repo_root/image/pi-gen/stage-cardputerzero-os/02-app-platform/01-run.sh"

jq -e '.developer_mode_allowed == true and .recovery_mode_allowed == false' \
    "$repo_root/appd/device-policy-production.json" >/dev/null

grep -qx 'SocketMode=0666' "$socket"
grep -Fq 'uid == shell_uid && management_command' "$devd"
grep -Fq 'uid == OWNER_UID && !management_command' "$devd"
grep -Fq 'DeveloperErrorCode::PairingClosed' "$devd"
grep -Fq 'MAX_PAIRING_WINDOW_SECONDS: u16 = 600' "$protocol"
grep -Fq 'libc::CLOCK_BOOTTIME' "$devd"
grep -Fq 'pairing_remaining_seconds' "$protocol"
if grep -Fq 'expires_at_unix_seconds' "$devd"; then
    echo "error: pairing authorization depends on the wall clock" >&2
    exit 1
fi
grep -Fq 'DeveloperCommand::UnpairAll' "$devd"
grep -Fq 'package developer key is not paired with this device' "$devd"
grep -Fq 'restrict,command=\"/usr/bin/cp0ctl dev-session\"' "$devd"

grep -Fq 'owner_shell_group=1999' "$owner_shell"
grep -Fq '/usr/bin/id -G' "$owner_shell"
grep -Fq 'exec /usr/bin/cp0ctl dev-session' "$owner_shell"
grep -Fq 'exec /bin/bash -l' "$owner_shell"

grep -Fq 'DisableForwarding yes' "$repo_root/appd/systemd/cardputerzero-sshd.conf"
grep -Fq 'authorized.as_bytes(),' "$devd"
grep -Fq '0o644' "$devd"

grep -Fq '.arg("cp0-dev")' "$remote"
if grep -Eq 'Command::new\("scp"\)|\.arg\("sudo"\)' "$remote"; then
    echo "error: developer transport contains an scp or sudo fallback" >&2
    exit 1
fi

grep -Fq 'cardputerzero-devd.socket' "$stage"
grep -Fq 'cardputerzero-devd.service cardputerzero-devd.socket' "$stage"
grep -Fq '/run/cardputerzero-devd' "$service"

printf '%s\n' 'developer access boundary tests passed'
