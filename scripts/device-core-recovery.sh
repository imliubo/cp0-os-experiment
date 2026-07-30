#!/bin/bash
set -euo pipefail

if ((EUID != 0)); then
    echo "error: device-core-recovery.sh must run as root" >&2
    exit 2
fi

compositor=cardputerzero-compositor.service
shell=cardputerzero-system-shell.service
appd=cardputerzero-appd.service

unit_value() {
    systemctl show --property="$2" --value "$1"
}

assert_active() {
    local unit=$1
    if [[ $(unit_value "$unit" ActiveState) != active ]] ||
        [[ $(unit_value "$unit" MainPID) == 0 ]]; then
        echo "error: $unit is not active with a main process" >&2
        return 1
    fi
}

wait_for_new_pid() {
    local unit=$1
    local old_pid=$2
    local attempt state new_pid

    for ((attempt = 0; attempt < 80; attempt++)); do
        state=$(unit_value "$unit" ActiveState)
        new_pid=$(unit_value "$unit" MainPID)
        if [[ $state == active && $new_pid != 0 && $new_pid != "$old_pid" ]]; then
            printf '%s\n' "$new_pid"
            return 0
        fi
        sleep 0.25
    done
    echo "error: $unit did not recover with a new PID" >&2
    journalctl --unit "$unit" --lines 20 --no-pager >&2 || true
    return 1
}

kill_and_verify_restart() {
    local unit=$1
    local old_pid old_restarts new_pid new_restarts

    assert_active "$unit"
    old_pid=$(unit_value "$unit" MainPID)
    old_restarts=$(unit_value "$unit" NRestarts)
    systemctl kill --kill-whom=main --signal=KILL "$unit"
    new_pid=$(wait_for_new_pid "$unit" "$old_pid")
    new_restarts=$(unit_value "$unit" NRestarts)
    if ((new_restarts <= old_restarts)); then
        echo "error: $unit restart counter did not increase" >&2
        return 1
    fi
    printf 'PASS restart %-42s pid=%s->%s restarts=%s->%s\n' \
        "$unit" "$old_pid" "$new_pid" "$old_restarts" "$new_restarts"
}

for unit in "$compositor" "$shell" "$appd"; do
    assert_active "$unit"
done

catalog=$(/usr/bin/cp0ctl app list 0 8)
if [[ $catalog == *'"running": true'* ]]; then
    echo "error: stop the foreground application before recovery testing" >&2
    exit 1
fi

compositor_pid=$(unit_value "$compositor" MainPID)
kill_and_verify_restart "$appd"
[[ $(unit_value "$compositor" MainPID) == "$compositor_pid" ]]
/usr/bin/cp0ctl app ping >/dev/null

compositor_pid=$(unit_value "$compositor" MainPID)
kill_and_verify_restart "$shell"
[[ $(unit_value "$compositor" MainPID) == "$compositor_pid" ]]

old_compositor_pid=$(unit_value "$compositor" MainPID)
old_compositor_restarts=$(unit_value "$compositor" NRestarts)
old_shell_pid=$(unit_value "$shell" MainPID)
appd_pid=$(unit_value "$appd" MainPID)
systemctl kill --kill-whom=main --signal=KILL "$compositor"
new_compositor_pid=$(wait_for_new_pid "$compositor" "$old_compositor_pid")
new_shell_pid=$(wait_for_new_pid "$shell" "$old_shell_pid")
new_compositor_restarts=$(unit_value "$compositor" NRestarts)
if ((new_compositor_restarts <= old_compositor_restarts)); then
    echo "error: compositor restart counter did not increase" >&2
    exit 1
fi
[[ $(unit_value "$appd" MainPID) == "$appd_pid" ]]
printf 'PASS restart %-42s pid=%s->%s restarts=%s->%s\n' \
    "$compositor" "$old_compositor_pid" "$new_compositor_pid" \
    "$old_compositor_restarts" "$new_compositor_restarts"
printf 'PASS rebound %-42s pid=%s->%s\n' \
    "$shell" "$old_shell_pid" "$new_shell_pid"

for unit in "$compositor" "$shell" "$appd"; do
    assert_active "$unit"
done
[[ -S /run/cardputerzero/wayland-0 ]]
[[ -S /run/cardputerzero-appd/control.sock ]]
[[ -S /run/cardputerzero-broker/runtime.sock ]]
/usr/bin/cp0ctl app ping >/dev/null
/usr/bin/cp0ctl app list 0 8 >/dev/null
echo "PASS core recovery and control paths"
