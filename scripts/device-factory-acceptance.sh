#!/usr/bin/env bash
set -euo pipefail

if (($# != 0)); then
    echo "usage: device-factory-acceptance" >&2
    exit 2
fi
if ((EUID != 0)); then
    echo "error: device-factory-acceptance must run as root" >&2
    exit 2
fi

umask 077
result_root=/run/cardputerzero-factory
run_id=$(date -u +%Y%m%dT%H%M%SZ)-$$
run_dir="$result_root/$run_id"
install -d -o root -g root -m 0700 "$run_dir"
checks="$run_dir/checks.tsv"
status_file="$run_dir/status"
failures=0
warnings=0
printf 'RUNNING\n' >"$status_file"
printf 'result\tcheck\tdetail\n' >"$checks"

record() {
    local result=$1 check=$2 detail=${3:-}
    detail=${detail//$'\t'/ }
    detail=${detail//$'\r'/ }
    detail=${detail//$'\n'/ }
    printf '%s\t%s\t%s\n' "$result" "$check" "${detail:0:512}" >>"$checks"
    case "$result" in
        FAIL) failures=$((failures + 1)) ;;
        WARN) warnings=$((warnings + 1)) ;;
    esac
}

require_active() {
    local unit=$1 active sub restarts
    active=$(systemctl show "$unit" --property=ActiveState --value 2>/dev/null || true)
    sub=$(systemctl show "$unit" --property=SubState --value 2>/dev/null || true)
    restarts=$(systemctl show "$unit" --property=NRestarts --value 2>/dev/null || true)
    if [[ $active != active || ($sub != running && $sub != listening &&
        $sub != exited) ]]; then
        record FAIL "unit:$unit" \
            "${active:-missing}/${sub:-missing} restarts=${restarts:-unknown}"
    elif [[ $unit == *.service && $restarts != 0 ]]; then
        record FAIL "unit:$unit" "$active/$sub restarts=${restarts:-unknown}"
    else
        record PASS "unit:$unit" "$active/$sub restarts=${restarts:-not-applicable}"
    fi
}

require_socket() {
    local name=$1 path=$2 expected=$3 actual
    if [[ ! -S $path ]]; then
        record FAIL "socket:$name" "$path missing"
        return
    fi
    actual=$(stat -c '%a:%U:%G' "$path" 2>/dev/null || true)
    if [[ $actual == "$expected" ]]; then
        record PASS "socket:$name" "$actual"
    else
        record FAIL "socket:$name" "${actual:-unreadable}; expected $expected"
    fi
}

if /usr/libexec/cardputerzero/device-smoke.sh >"$run_dir/hardware-smoke.txt" 2>&1; then
    smoke_summary=$(tail -1 "$run_dir/hardware-smoke.txt" 2>/dev/null || true)
    if [[ $smoke_summary =~ warnings=([0-9]+) ]]; then
        warnings=$((warnings + BASH_REMATCH[1]))
    fi
    record PASS hardware-smoke \
        "${smoke_summary:-all required V0.6 hardware checks passed}"
else
    smoke_summary=$(tail -1 "$run_dir/hardware-smoke.txt" 2>/dev/null || true)
    record FAIL hardware-smoke "${smoke_summary:-device-smoke failed}"
fi

if [[ $(cat /etc/cardputerzero/image-profile 2>/dev/null || true) == product ]]; then
    record PASS image-profile product
else
    record FAIL image-profile "factory gate requires a product image"
fi

cmdline=" $(cat /proc/cmdline 2>/dev/null) "
if [[ $cmdline == *" cp0.overlay_root=volatile "* ]] &&
    [[ $(findmnt -n -o FSTYPE / 2>/dev/null) == overlay ]] &&
    systemctl is-active --quiet cardputerzero-overlay-root-status.service; then
    record PASS immutable-root "volatile overlay and filesystem validation active"
else
    record FAIL immutable-root "product overlay profile is not active"
fi

data_source=$(findmnt -n -o SOURCE --target /run/cardputerzero-data 2>/dev/null || true)
data_label=$(blkid -s LABEL -o value "$data_source" 2>/dev/null || true)
if [[ $data_source == /dev/mmcblk0p3 && $data_label == cp0-data ]]; then
    record PASS data-partition "$data_source label=$data_label"
else
    record FAIL data-partition "source=${data_source:-missing} label=${data_label:-missing}"
fi
partition_start=$(cat /sys/class/block/mmcblk0p3/start 2>/dev/null || true)
partition_size=$(cat /sys/class/block/mmcblk0p3/size 2>/dev/null || true)
device_size=$(cat /sys/class/block/mmcblk0/size 2>/dev/null || true)
if [[ $partition_start =~ ^[0-9]+$ && $partition_size =~ ^[0-9]+$ &&
    $device_size =~ ^[0-9]+$ ]] &&
    ((partition_start + partition_size >= device_size - 2048)); then
    record PASS data-expanded \
        "end=$((partition_start + partition_size)) device=$device_size"
else
    record FAIL data-expanded "cp0-data does not reach the final 1 MiB"
fi
partition_bytes=$(blockdev --getsize64 /dev/mmcblk0p3 2>/dev/null || true)
filesystem_bytes=$(findmnt -b -n -o FS-SIZE \
    --target /run/cardputerzero-data 2>/dev/null || true)
if [[ $partition_bytes =~ ^[0-9]+$ && $filesystem_bytes =~ ^[0-9]+$ ]] &&
    ((filesystem_bytes >= partition_bytes - 16 * 1024 * 1024)); then
    record PASS data-filesystem-expanded \
        "filesystem=$filesystem_bytes partition=$partition_bytes"
else
    record FAIL data-filesystem-expanded \
        "filesystem=${filesystem_bytes:-unknown} partition=${partition_bytes:-unknown}"
fi
if [[ $(cat /run/cardputerzero-data/layout-version 2>/dev/null || true) == \
    cp0-data-layout-v1 ]]; then
    record PASS data-layout cp0-data-layout-v1
else
    record FAIL data-layout "missing or unsupported layout marker"
fi

for marker in developer-mode recovery-mode; do
    if [[ -e /var/lib/cardputerzero/registry/$marker ]]; then
        record FAIL "default-mode:$marker" "must be absent on an unprovisioned unit"
    else
        record PASS "default-mode:$marker" absent
    fi
done

for unit in \
    cardputerzero-overlay-root-status.service \
    cardputerzero-compositor.service \
    cardputerzero-system-shell.service \
    cardputerzero-appd.service \
    seatd.service \
    cardputerzero-appd.socket \
    cardputerzero-broker.socket \
    cardputerzero-networkd.socket \
    cardputerzero-documentd.socket \
    cardputerzero-audiod.socket \
    cardputerzero-camerad.socket \
    cardputerzero-gpiod.socket \
    cardputerzero-radiod.socket \
    cardputerzero-storaged.socket \
    cardputerzero-stored.socket; do
    require_active "$unit"
done

require_socket appd /run/cardputerzero-appd/control.sock 660:root:cp0-control
require_socket runtime /run/cardputerzero-broker/runtime.sock 666:root:root
require_socket network /run/cardputerzero-networkd/network.sock 600:root:root
require_socket documents /run/cardputerzero-documentd/documents.sock 600:root:root
require_socket audio /run/cardputerzero-audiod/audio.sock 600:root:root
require_socket camera /run/cardputerzero-camerad/camera.sock 600:root:root
require_socket gpio /run/cardputerzero-gpiod/gpio.sock 600:root:root
require_socket radio /run/cardputerzero-radiod/radio.sock 600:root:root
require_socket storage /run/cardputerzero-storaged/storage.sock 600:root:root
require_socket store /run/cardputerzero-store/control.sock 660:root:cp0-control

if /usr/bin/cp0ctl app ping >/dev/null 2>&1; then
    record PASS appd-control "authenticated ping succeeded"
else
    record FAIL appd-control "authenticated ping failed"
fi
failed_units=$(systemctl --failed --no-legend --plain 2>/dev/null) || {
    record FAIL failed-units "systemd failed-unit query failed"
    failed_units=
}
if [[ -n $failed_units ]]; then
    record FAIL failed-units "systemd has failed units"
elif ! grep -q $'^FAIL\tfailed-units\t' "$checks"; then
    record PASS failed-units none
fi

sd_sectors_written=$(awk '{ print $7 }' /sys/block/mmcblk0/stat 2>/dev/null || true)
mem_available=$(awk '/^MemAvailable:/ { print $2 * 1024 }' /proc/meminfo 2>/dev/null || true)
finished_epoch=$(date +%s)
{
    printf 'schema=cardputerzero-factory-v1\n'
    printf 'run_id=%s\n' "$run_id"
    printf 'finished_epoch=%s\n' "$finished_epoch"
    printf 'failure_count=%s\n' "$failures"
    printf 'warning_count=%s\n' "$warnings"
    printf 'mem_available_bytes=%s\n' "${mem_available:-unknown}"
    printf 'sd_sectors_written=%s\n' "${sd_sectors_written:-unknown}"
} >"$run_dir/summary.env"

if ((failures == 0)); then
    printf 'PASS\n' >"$status_file"
    printf 'PASS factory acceptance %s warnings=%s\n' "$run_dir" "$warnings"
    exit 0
fi
printf 'FAILED failures=%s\n' "$failures" >"$status_file"
printf 'FAILED factory acceptance %s failures=%s warnings=%s\n' \
    "$run_dir" "$failures" "$warnings" >&2
exit 1
