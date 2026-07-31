#!/usr/bin/env bash
set -euo pipefail

duration_seconds=${1:-60}
interval_seconds=${2:-5}
if (($# > 2)) || [[ ! $duration_seconds =~ ^[0-9]+$ ]] ||
    [[ ! $interval_seconds =~ ^[0-9]+$ ]] ||
    ((duration_seconds < interval_seconds || duration_seconds > 3600 ||
      interval_seconds < 1)); then
    echo "usage: device-performance-acceptance [DURATION_SECONDS] [INTERVAL_SECONDS]" >&2
    exit 2
fi
if ((EUID != 0)); then
    echo "error: device-performance-acceptance must run as root" >&2
    exit 2
fi

readonly max_boot_ready_ms=35000
readonly max_idle_used_bytes=$((180 * 1024 * 1024))
readonly min_idle_available_bytes=$((200 * 1024 * 1024))
readonly max_core_cpu_millipercent=10000
readonly max_short_sd_write_bytes=$((1024 * 1024))
readonly result_root=/run/cardputerzero-performance
readonly run_id=$(date -u +%Y%m%dT%H%M%SZ)-$$
readonly run_dir="$result_root/$run_id"
readonly checks="$run_dir/checks.tsv"
readonly samples="$run_dir/samples.tsv"
readonly services="$run_dir/services.tsv"
readonly status_file="$run_dir/status"

umask 077
install -d -o root -g root -m 0700 "$run_dir"
printf 'RUNNING\n' >"$status_file"
printf 'result\tcheck\tdetail\n' >"$checks"
printf 'epoch\tuptime_seconds\tmem_available_bytes\tmem_used_bytes\tdirty_bytes\twriteback_bytes\tmmc_sectors_written\tvoltage_uv\tcurrent_ua\testimated_battery_uw\n' \
    >"$samples"
printf 'unit\tstart_pid\tend_pid\tstart_cpu_ns\tend_cpu_ns\tcpu_millipercent\tmax_memory_bytes\tstart_restarts\tend_restarts\n' \
    >"$services"

failures=0
warnings=0
finished=0
boot_ready_ms=unknown
shell_ready_ms=unknown
maximum_used_bytes=0
minimum_available_bytes=9223372036854775807
sd_baseline_sectors=unknown
sd_final_sectors=unknown
sd_write_bytes=unknown
core_cpu_millipercent=unknown
battery_sample_count=0
battery_power_sum_uw=0
battery_average_power_uw=unknown

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

on_exit() {
    local code=$?
    if ((finished == 0)); then
        printf 'FAILED exit=%s\n' "$code" >"$status_file"
    fi
}
trap on_exit EXIT

finish() {
    local finished_epoch
    finished_epoch=$(date +%s)
    {
        printf 'schema=cardputerzero-performance-v1\n'
        printf 'run_id=%s\n' "$run_id"
        printf 'finished_epoch=%s\n' "$finished_epoch"
        printf 'duration_seconds=%s\n' "$duration_seconds"
        printf 'interval_seconds=%s\n' "$interval_seconds"
        printf 'failure_count=%s\n' "$failures"
        printf 'warning_count=%s\n' "$warnings"
        printf 'boot_ready_ms=%s\n' "$boot_ready_ms"
        printf 'shell_ready_ms=%s\n' "$shell_ready_ms"
        printf 'maximum_idle_used_bytes=%s\n' "$maximum_used_bytes"
        printf 'minimum_idle_available_bytes=%s\n' "$minimum_available_bytes"
        printf 'core_cpu_millipercent=%s\n' "$core_cpu_millipercent"
        printf 'sd_baseline_sectors_written=%s\n' "$sd_baseline_sectors"
        printf 'sd_final_sectors_written=%s\n' "$sd_final_sectors"
        printf 'sd_write_bytes=%s\n' "$sd_write_bytes"
        printf 'battery_sample_count=%s\n' "$battery_sample_count"
        printf 'battery_average_estimated_uw=%s\n' "$battery_average_power_uw"
        printf 'maximum_boot_ready_ms=%s\n' "$max_boot_ready_ms"
        printf 'maximum_idle_used_bytes_limit=%s\n' "$max_idle_used_bytes"
        printf 'minimum_idle_available_bytes_limit=%s\n' "$min_idle_available_bytes"
        printf 'maximum_core_cpu_millipercent=%s\n' "$max_core_cpu_millipercent"
        printf 'maximum_short_sd_write_bytes=%s\n' "$max_short_sd_write_bytes"
    } >"$run_dir/summary.env"
    finished=1
    if ((failures == 0)); then
        printf 'PASS\n' >"$status_file"
        printf 'PASS performance acceptance %s warnings=%s\n' "$run_dir" "$warnings"
        exit 0
    fi
    printf 'FAILED failures=%s\n' "$failures" >"$status_file"
    printf 'FAILED performance acceptance %s failures=%s warnings=%s\n' \
        "$run_dir" "$failures" "$warnings" >&2
    exit 1
}

read_property() {
    local unit=$1 property=$2
    systemctl show "$unit" --property="$property" --value 2>/dev/null || true
}

read_meminfo_bytes() {
    local field=$1
    awk -v key="$field:" '$1 == key { print $2 * 1024 }' \
        /proc/meminfo 2>/dev/null
}

read_sd_sectors() {
    awk '{ print $7 }' /sys/block/mmcblk0/stat 2>/dev/null
}

if systemctl is-active --quiet cardputerzero-stability-acceptance.service; then
    record FAIL stability-interlock \
        "24-hour stability acceptance is active; performance sampling is deferred"
    finish
fi
record PASS stability-interlock inactive

if [[ $(cat /etc/cardputerzero/image-profile 2>/dev/null || true) != product ]]; then
    record FAIL image-profile "performance gate requires the product image"
fi

app_list=$(/usr/bin/cp0ctl app list 2>"$run_dir/app-list.err") || {
    record FAIL foreground-precondition "cannot query application list"
    finish
}
printf '%s\n' "$app_list" >"$run_dir/app-list.json"
if grep -q '"running": true' "$run_dir/app-list.json"; then
    record FAIL foreground-precondition "stop the foreground application before idle sampling"
    finish
fi
record PASS foreground-precondition "no application is running"

boot_ready_usec=$(systemctl show --property=FinishTimestampMonotonic --value \
    2>/dev/null || true)
shell_ready_usec=$(read_property cardputerzero-system-shell.service \
    ActiveEnterTimestampMonotonic)
if [[ $boot_ready_usec =~ ^[0-9]+$ && $boot_ready_usec -gt 0 ]]; then
    boot_ready_ms=$((boot_ready_usec / 1000))
    if ((boot_ready_ms <= max_boot_ready_ms)); then
        record PASS boot-ready "${boot_ready_ms}ms <= ${max_boot_ready_ms}ms"
    else
        record FAIL boot-ready "${boot_ready_ms}ms > ${max_boot_ready_ms}ms"
    fi
else
    record FAIL boot-ready "systemd monotonic finish timestamp unavailable"
fi
if [[ $shell_ready_usec =~ ^[0-9]+$ && $shell_ready_usec -gt 0 ]]; then
    shell_ready_ms=$((shell_ready_usec / 1000))
    if ((shell_ready_ms <= max_boot_ready_ms)); then
        record PASS shell-ready "${shell_ready_ms}ms <= ${max_boot_ready_ms}ms"
    else
        record FAIL shell-ready "${shell_ready_ms}ms > ${max_boot_ready_ms}ms"
    fi
else
    record FAIL shell-ready "System Shell monotonic activation timestamp unavailable"
fi

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
declare -A start_cpu max_memory start_pid start_restarts
for unit in "${units[@]}"; do
    active=$(read_property "$unit" ActiveState)
    sub=$(read_property "$unit" SubState)
    start_pid[$unit]=$(read_property "$unit" MainPID)
    start_cpu[$unit]=$(read_property "$unit" CPUUsageNSec)
    max_memory[$unit]=0
    start_restarts[$unit]=$(read_property "$unit" NRestarts)
    if [[ $active != active || $sub != running ||
        ! ${start_pid[$unit]} =~ ^[1-9][0-9]*$ ]]; then
        record FAIL "unit:$unit" \
            "unhealthy start state=$active/$sub pid=${start_pid[$unit]:-missing}"
    else
        record PASS "unit-active:$unit" \
            "state=$active/$sub pid=${start_pid[$unit]}"
    fi
    if [[ ! ${start_cpu[$unit]} =~ ^[0-9]+$ ||
        ! ${start_restarts[$unit]} =~ ^[0-9]+$ ]]; then
        record FAIL "unit:$unit" "CPU or restart counters unavailable"
    elif [[ ${start_restarts[$unit]} != 0 ]]; then
        record FAIL "unit:$unit" "restart count ${start_restarts[$unit]}"
    fi
done

power_supply=$(find /sys/class/power_supply -maxdepth 1 -type l \
    -name 'bq27220-*' 2>/dev/null | head -1)
sd_baseline_sectors=$(read_sd_sectors || true)
start_epoch=$(date +%s)
end_epoch=$((start_epoch + duration_seconds))
while :; do
    epoch=$(date +%s)
    uptime=$(awk '{ print $1 }' /proc/uptime 2>/dev/null || true)
    mem_total=$(read_meminfo_bytes MemTotal || true)
    mem_available=$(read_meminfo_bytes MemAvailable || true)
    dirty=$(read_meminfo_bytes Dirty || true)
    writeback=$(read_meminfo_bytes Writeback || true)
    sectors=$(read_sd_sectors || true)
    if [[ $mem_total =~ ^[0-9]+$ && $mem_available =~ ^[0-9]+$ &&
        $mem_total -ge $mem_available ]]; then
        mem_used=$((mem_total - mem_available))
        ((mem_used > maximum_used_bytes)) && maximum_used_bytes=$mem_used
        ((mem_available < minimum_available_bytes)) && \
            minimum_available_bytes=$mem_available
    else
        mem_used=unknown
        record FAIL memory-sample "MemTotal or MemAvailable unavailable"
    fi

    voltage=unknown
    current=unknown
    estimated_power=unknown
    if [[ -n $power_supply ]]; then
        voltage=$(cat "$power_supply/voltage_now" 2>/dev/null || true)
        current=$(cat "$power_supply/current_now" 2>/dev/null || true)
        if [[ $voltage =~ ^[0-9]+$ && $current =~ ^-?[0-9]+$ ]]; then
            absolute_current=${current#-}
            estimated_power=$((voltage * absolute_current / 1000000))
            battery_power_sum_uw=$((battery_power_sum_uw + estimated_power))
            battery_sample_count=$((battery_sample_count + 1))
        else
            voltage=unknown
            current=unknown
        fi
    fi

    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$epoch" "${uptime:-unknown}" "${mem_available:-unknown}" \
        "$mem_used" "${dirty:-unknown}" "${writeback:-unknown}" \
        "${sectors:-unknown}" "$voltage" "$current" "$estimated_power" \
        >>"$samples"
    for unit in "${units[@]}"; do
        memory=$(read_property "$unit" MemoryCurrent)
        if [[ $memory =~ ^[0-9]+$ ]]; then
            ((memory > max_memory[$unit])) && max_memory[$unit]=$memory
        else
            record FAIL "memory:$unit" "MemoryCurrent unavailable"
        fi
    done
    ((epoch >= end_epoch)) && break
    sleep "$interval_seconds"
done
finish_epoch=$(date +%s)
elapsed_ns=$(((finish_epoch - start_epoch) * 1000000000))

if ((maximum_used_bytes <= max_idle_used_bytes)); then
    record PASS idle-used-memory \
        "$maximum_used_bytes <= $max_idle_used_bytes"
else
    record FAIL idle-used-memory \
        "$maximum_used_bytes > $max_idle_used_bytes"
fi
if ((minimum_available_bytes >= min_idle_available_bytes)); then
    record PASS idle-available-memory \
        "$minimum_available_bytes >= $min_idle_available_bytes"
else
    record FAIL idle-available-memory \
        "$minimum_available_bytes < $min_idle_available_bytes"
fi

total_cpu_delta=0
for unit in "${units[@]}"; do
    end_cpu=$(read_property "$unit" CPUUsageNSec)
    end_pid=$(read_property "$unit" MainPID)
    end_restarts=$(read_property "$unit" NRestarts)
    end_active=$(read_property "$unit" ActiveState)
    end_sub=$(read_property "$unit" SubState)
    unit_cpu_millipercent=unknown
    if [[ ${start_cpu[$unit]} =~ ^[0-9]+$ && $end_cpu =~ ^[0-9]+$ &&
        $end_cpu -ge ${start_cpu[$unit]} && $elapsed_ns -gt 0 ]]; then
        delta=$((end_cpu - start_cpu[$unit]))
        total_cpu_delta=$((total_cpu_delta + delta))
        unit_cpu_millipercent=$((delta * 100000 / elapsed_ns))
    else
        record FAIL "cpu:$unit" "invalid CPU accounting counters"
    fi
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$unit" \
        "${start_pid[$unit]}" "${end_pid:-unknown}" \
        "${start_cpu[$unit]}" "${end_cpu:-unknown}" "$unit_cpu_millipercent" \
        "${max_memory[$unit]}" "${start_restarts[$unit]}" \
        "${end_restarts:-unknown}" >>"$services"
    if [[ $end_active != active || $end_sub != running ||
        $end_pid != "${start_pid[$unit]}" ||
        $end_restarts != "${start_restarts[$unit]}" ]]; then
        record FAIL "continuity:$unit" \
            "end=$end_active/$end_sub pid=${start_pid[$unit]}->$end_pid restarts=${start_restarts[$unit]}->$end_restarts"
    else
        record PASS "continuity:$unit" \
            "pid=$end_pid restarts=$end_restarts"
    fi
    if ((max_memory[$unit] > memory_limit[$unit])); then
        record FAIL "memory:$unit" \
            "${max_memory[$unit]} > ${memory_limit[$unit]}"
    else
        record PASS "memory:$unit" \
            "${max_memory[$unit]} <= ${memory_limit[$unit]}"
    fi
done
if ((elapsed_ns > 0)); then
    core_cpu_millipercent=$((total_cpu_delta * 100000 / elapsed_ns))
    if ((core_cpu_millipercent <= max_core_cpu_millipercent)); then
        record PASS core-idle-cpu \
            "$core_cpu_millipercent <= $max_core_cpu_millipercent millipercent"
    else
        record FAIL core-idle-cpu \
            "$core_cpu_millipercent > $max_core_cpu_millipercent millipercent"
    fi
fi

sd_final_sectors=$(read_sd_sectors || true)
if [[ $sd_baseline_sectors =~ ^[0-9]+$ && $sd_final_sectors =~ ^[0-9]+$ &&
    $sd_final_sectors -ge $sd_baseline_sectors ]]; then
    sd_write_bytes=$(((sd_final_sectors - sd_baseline_sectors) * 512))
    if ((sd_write_bytes <= max_short_sd_write_bytes)); then
        record PASS short-sd-write \
            "$sd_write_bytes <= $max_short_sd_write_bytes"
    else
        record FAIL short-sd-write \
            "$sd_write_bytes > $max_short_sd_write_bytes"
    fi
else
    record FAIL short-sd-write "mmcblk0 write counter unavailable or regressed"
fi

if ((battery_sample_count > 0)); then
    battery_average_power_uw=$((battery_power_sum_uw / battery_sample_count))
    record PASS battery-telemetry \
        "samples=$battery_sample_count estimated_average=${battery_average_power_uw}uW record-only"
else
    record WARN battery-telemetry \
        "bq27220 voltage/current unavailable; external power measurement remains required"
fi

finish
