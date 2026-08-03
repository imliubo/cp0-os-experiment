#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
bsp="$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/01-run.sh"
gate="$repo_root/appd/systemd/cardputerzero-ssh-gate.conf"
generator="$repo_root/appd/systemd/cardputerzero-ssh-generator"
sshd_config="$repo_root/appd/systemd/cardputerzero-sshd.conf"
access_allowed="$repo_root/appd/systemd/cardputerzero-ssh-access-allowed"
owner_shell="$repo_root/appd/systemd/cardputerzero-owner-shell"
devd="$repo_root/crates/cp0-devd/src/main.rs"
avahi="$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/files/avahi-daemon.conf"

for path in \
    scripts/enable-maintenance-ssh.sh \
    scripts/device-hot-update-firstboot.sh \
    image/pi-gen/stage-cardputerzero-os/00-bsp/files/prepare-maintenance-ssh.sh \
    image/pi-gen/stage-cardputerzero-os/00-bsp/files/hot-update-firstboot.sh \
    image/pi-gen/stage-cardputerzero-os/00-bsp/files/cardputerzero-maintenance-ssh.service \
    image/pi-gen/stage-cardputerzero-os/00-bsp/files/maintenance-sshd_config; do
    if [[ -e $repo_root/$path || -L $repo_root/$path ]]; then
        echo "error: pre-Setup maintenance access remains: $path" >&2
        exit 1
    fi
done

if grep -Eq 'maintenance-ssh|cp0-maintenance|hot-update-firstboot' "$bsp"; then
    echo "error: production BSP still installs pre-Setup remote access" >&2
    exit 1
fi
if grep -q '^host-name=cardputerzero-maintenance$' "$avahi"; then
    echo "error: removed maintenance identity remains advertised" >&2
    exit 1
fi

test "$(grep -c '^ConditionPathExists=/var/lib/cardputerzero/provisioning/complete$' "$gate")" -eq 1
test "$(grep -c '^ConditionPathExists=/var/lib/cardputerzero/provisioning/ssh-enabled$' "$gate")" -eq 0
grep -qx 'ExecCondition=/usr/libexec/cardputerzero/ssh-access-allowed' "$gate"
grep -Fq 'complete=/var/lib/cardputerzero/provisioning/complete' "$generator"
grep -Fq 'enabled=/var/lib/cardputerzero/provisioning/ssh-enabled' "$generator"
grep -Fq 'developer=/var/lib/cardputerzero/registry/developer-mode' "$generator"
grep -Fq '[ -f "$complete" ] && [ ! -L "$complete" ] || exit 0' "$generator"
grep -qx 'PermitRootLogin no' "$sshd_config"
grep -qx 'AllowGroups cp0-ssh cp0-developer-access' "$sshd_config"
grep -qx 'PasswordAuthentication yes' "$sshd_config"
grep -qx 'AuthorizedKeysFile /etc/cardputerzero/authorized_keys/%u' "$sshd_config"
grep -qx 'DisableForwarding yes' "$sshd_config"
grep -Fq 'owner_shell=/var/lib/cardputerzero/provisioning/ssh-enabled' "$access_allowed"
grep -Fq 'developer=/var/lib/cardputerzero/registry/developer-mode' "$access_allowed"
grep -Fq 'owner_shell_group=1999' "$owner_shell"
grep -Fq '/usr/bin/id -G' "$owner_shell"
grep -Fq 'exec /usr/bin/cp0ctl dev-session' "$owner_shell"
grep -Fq 'CardputerZero owner SSH shell is disabled' "$owner_shell"
grep -Fq 'restrict,command=\"/usr/bin/cp0ctl dev-session\"' "$devd"

bash -n "$bsp" "$generator" "$access_allowed" "$owner_shell"

test_parent="$repo_root/target/test-tmp"
mkdir -p "$test_parent"
work_dir=$(mktemp -d "$test_parent/owner-shell.XXXXXX")
trap 'rm -rf "$work_dir"' EXIT
dispatcher="$work_dir/owner-shell"
mock_cp0ctl="$work_dir/cp0ctl"
mock_bash="$work_dir/bash"
mock_id="$work_dir/id"
sed \
    -e "s#/usr/bin/cp0ctl#$mock_cp0ctl#g" \
    -e "s#/usr/bin/id#$mock_id#g" \
    -e "s#/bin/bash#$mock_bash#g" \
    "$owner_shell" >"$dispatcher"
printf '%s\n' '#!/bin/sh' 'printf "cp0ctl:%s\n" "$*"' >"$mock_cp0ctl"
printf '%s\n' '#!/bin/sh' 'printf "bash:%s\n" "$*"' >"$mock_bash"
printf '%s\n' '#!/bin/sh' 'test "$1" = -G' 'printf "%s\n" "${CP0_TEST_GROUPS:-1000}"' >"$mock_id"
chmod 0755 "$dispatcher" "$mock_cp0ctl" "$mock_bash" "$mock_id"

test "$(env -u SSH_ORIGINAL_COMMAND "$dispatcher" -c cp0-dev)" = \
    'cp0ctl:dev-session'
test "$(SSH_ORIGINAL_COMMAND=cp0-dev "$dispatcher" -c \
    "$mock_cp0ctl dev-session")" = 'cp0ctl:dev-session'
if env -u SSH_ORIGINAL_COMMAND "$dispatcher" -c uname \
    >"$work_dir/denied.out" 2>"$work_dir/denied.err"; then
    echo "error: disabled owner shell accepted an arbitrary command" >&2
    exit 1
else
    test "$?" -eq 126
fi

test "$(env -u SSH_ORIGINAL_COMMAND CP0_TEST_GROUPS='1000 1999' "$dispatcher")" = \
    'bash:-l'
test "$(env -u SSH_ORIGINAL_COMMAND CP0_TEST_GROUPS='1000 1999' \
    "$dispatcher" -c 'printf owner-command')" = \
    'bash:-c printf owner-command'
test "$(SSH_ORIGINAL_COMMAND=ignored "$dispatcher" -c \
    "$mock_cp0ctl dev-session")" = 'cp0ctl:dev-session'

printf '%s\n' 'setup SSH boundary tests passed'
