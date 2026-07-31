#!/bin/bash
set -euo pipefail

duration_seconds=${1:-86400}
interval_seconds=${2:-60}
result_root=${3:-/run/cardputerzero-stability}
maximum_sd_write_bytes=${4:-67108864}

if ((EUID != 0)); then
    echo "error: device-stability-monitor.sh must run as root" >&2
    exit 2
fi
if [[ ! $duration_seconds =~ ^[0-9]+$ ]] ||
    [[ ! $interval_seconds =~ ^[0-9]+$ ]] ||
    [[ ! $maximum_sd_write_bytes =~ ^[0-9]+$ ]] ||
    ((duration_seconds < interval_seconds || interval_seconds < 1)); then
    echo "error: duration, interval and SD write limit must be positive integers" >&2
    exit 2
fi
case "$result_root" in
    /run/cardputerzero-stability | /run/cardputerzero-stability/*) ;;
    *)
        echo "error: result directory must be below /run/cardputerzero-stability" >&2
        exit 2
        ;;
esac

run_id=$(date -u +%Y%m%dT%H%M%SZ)-$$
run_dir="$result_root/$run_id"
install -d -o root -g root -m 0700 "$run_dir"
samples="$run_dir/samples.tsv"
failures="$run_dir/failures.log"
status_file="$run_dir/status"
block_samples="$run_dir/block-io.tsv"
printf 'RUNNING\n' >"$status_file"
printf 'epoch\tuptime\tunit\tactive\tsub\tpid\trestarts\tmemory_bytes\n' \
    >"$samples"
printf 'epoch\tuptime\tsectors_written\tbytes_written\n' >"$block_samples"

units=(
    cardputerzero-compositor.service
    cardputerzero-system-shell.service
    cardputerzero-appd.service
)
declare -A memory_limit=(
    [cardputerzero-compositor.service]=$((32 * 1024 * 1024))
    [cardputerzero-system-shell.service]=$((32 * 1024 * 1024))
    [cardputerzero-appd.service]=$((24 * 1024 * 1024))
)
declare -A allowed_growth=(
    [cardputerzero-compositor.service]=$((4 * 1024 * 1024))
    [cardputerzero-system-shell.service]=$((2 * 1024 * 1024))
    [cardputerzero-appd.service]=$((4 * 1024 * 1024))
)
declare -A baseline_restarts baseline_memory final_memory
failure_count=0
finished=0
baseline_sectors_written=
final_sectors_written=

on_exit() {
    local code=$?
    if ((finished == 0)); then
        printf 'FAILED exit=%s\n' "$code" >"$status_file"
    fi
}
trap on_exit EXIT

record_failure() {
    failure_count=$((failure_count + 1))
    printf '%s\t%s\n' "$(date +%s)" "$1" >>"$failures"
}

read_unit_properties() {
    local unit=$1 key value
    property_active=
    property_sub=
    property_pid=
    property_restarts=
    property_memory=
    while IFS='=' read -r key value; do
        case "$key" in
            ActiveState) property_active=$value ;;
            SubState) property_sub=$value ;;
            MainPID) property_pid=$value ;;
            NRestarts) property_restarts=$value ;;
            MemoryCurrent) property_memory=$value ;;
        esac
    done < <(systemctl show "$unit" \
        --property=ActiveState,SubState,MainPID,NRestarts,MemoryCurrent)
    [[ -n $property_active && -n $property_sub &&
       $property_pid =~ ^[0-9]+$ && $property_restarts =~ ^[0-9]+$ &&
       $property_memory =~ ^[0-9]+$ ]]
}

sample_once() {
    local epoch uptime unit sectors_written store_unit
    epoch=$(date +%s)
    uptime=${EPOCHREALTIME:-$epoch}
    if [[ -r /proc/uptime ]]; then
        read -r uptime _ </proc/uptime
    fi
    if ! /usr/bin/cp0ctl app ping >/dev/null; then
        record_failure "appd-ping-failed"
    fi
    sectors_written=$(awk '{ print $7 }' /sys/block/mmcblk0/stat 2>/dev/null || true)
    if [[ $sectors_written =~ ^[0-9]+$ ]]; then
        if [[ -z $baseline_sectors_written ]]; then
            baseline_sectors_written=$sectors_written
        fi
        final_sectors_written=$sectors_written
        printf '%s\t%s\t%s\t%s\n' "$epoch" "$uptime" \
            "$sectors_written" "$((sectors_written * 512))" >>"$block_samples"
    else
        record_failure "mmcblk0-write-counter-unavailable"
    fi
    for unit in "${units[@]}"; do
        if ! read_unit_properties "$unit"; then
            record_failure "$unit invalid-properties"
            continue
        fi
        printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
            "$epoch" "$uptime" "$unit" "$property_active" \
            "$property_sub" "$property_pid" "$property_restarts" \
            "$property_memory" >>"$samples"
        if [[ $property_active != active || $property_sub != running ||
              $property_pid == 0 ]]; then
            record_failure "$unit inactive state=$property_active/$property_sub pid=$property_pid"
        fi
        if [[ -z ${baseline_restarts[$unit]+set} ]]; then
            baseline_restarts[$unit]=$property_restarts
            baseline_memory[$unit]=$property_memory
        elif [[ $property_restarts != "${baseline_restarts[$unit]}" ]]; then
            record_failure "$unit restart-count=${baseline_restarts[$unit]}->$property_restarts"
        fi
        if ((property_memory > memory_limit[$unit])); then
            record_failure "$unit memory-limit=$property_memory>${memory_limit[$unit]}"
        fi
        final_memory[$unit]=$property_memory
    done
    store_unit=cardputerzero-stored.service
    if systemctl is-active --quiet "$store_unit"; then
        if ! read_unit_properties "$store_unit"; then
            record_failure "$store_unit invalid-properties"
        else
            printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
                "$epoch" "$uptime" "$store_unit" "$property_active" \
                "$property_sub" "$property_pid" "$property_restarts" \
                "$property_memory" >>"$samples"
            if [[ $property_sub != running || $property_pid == 0 ]]; then
                record_failure "$store_unit inactive state=$property_active/$property_sub pid=$property_pid"
            fi
            if ((property_memory > 40 * 1024 * 1024)); then
                record_failure "$store_unit memory-limit=$property_memory>$((40 * 1024 * 1024))"
            fi
        fi
    elif systemctl is-failed --quiet "$store_unit"; then
        record_failure "$store_unit failed"
    fi
    [[ -S /run/cardputerzero/wayland-0 ]] || record_failure "wayland-socket-missing"
    [[ -S /run/cardputerzero-appd/control.sock ]] || record_failure "control-socket-missing"
    [[ -S /run/cardputerzero-broker/runtime.sock ]] || record_failure "broker-socket-missing"
    [[ -S /run/cardputerzero-store/control.sock ]] || record_failure "store-socket-missing"
}

start_epoch=$(date +%s)
end_epoch=$((start_epoch + duration_seconds))
while (($(date +%s) < end_epoch)); do
    sample_once
    sleep "$interval_seconds"
done
sample_once

for unit in "${units[@]}"; do
    growth=$((final_memory[$unit] - baseline_memory[$unit]))
    if ((growth > allowed_growth[$unit])); then
        record_failure "$unit memory-growth=$growth>${allowed_growth[$unit]}"
    fi
done
sd_write_bytes=-1
if [[ $baseline_sectors_written =~ ^[0-9]+$ &&
      $final_sectors_written =~ ^[0-9]+$ ]]; then
    sd_write_bytes=$(((final_sectors_written - baseline_sectors_written) * 512))
    if ((sd_write_bytes > maximum_sd_write_bytes)); then
        record_failure "sd-write-bytes=$sd_write_bytes>$maximum_sd_write_bytes"
    fi
fi

finish_epoch=$(date +%s)
{
    printf 'run_id=%s\n' "$run_id"
    printf 'started_epoch=%s\n' "$start_epoch"
    printf 'finished_epoch=%s\n' "$finish_epoch"
    printf 'duration_seconds=%s\n' "$duration_seconds"
    printf 'interval_seconds=%s\n' "$interval_seconds"
    printf 'failure_count=%s\n' "$failure_count"
    printf 'sd_baseline_sectors_written=%s\n' "$baseline_sectors_written"
    printf 'sd_final_sectors_written=%s\n' "$final_sectors_written"
    printf 'sd_write_bytes=%s\n' "$sd_write_bytes"
    printf 'maximum_sd_write_bytes=%s\n' "$maximum_sd_write_bytes"
    for unit in "${units[@]}"; do
        printf '%s_baseline_memory=%s\n' "${unit%.service}" \
            "${baseline_memory[$unit]}"
        printf '%s_final_memory=%s\n' "${unit%.service}" \
            "${final_memory[$unit]}"
        printf '%s_restarts=%s\n' "${unit%.service}" \
            "${baseline_restarts[$unit]}"
    done
} >"$run_dir/summary.env"

finished=1
if ((failure_count == 0)); then
    printf 'PASS\n' >"$status_file"
    printf 'PASS stability run %s\n' "$run_dir"
    exit 0
fi
printf 'FAILED failures=%s\n' "$failure_count" >"$status_file"
printf 'FAILED stability run %s failures=%s\n' "$run_dir" "$failure_count" >&2
exit 1
