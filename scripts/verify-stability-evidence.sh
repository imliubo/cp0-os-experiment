#!/usr/bin/env bash
set -euo pipefail

if (($# != 1)); then
    echo "usage: $0 STABILITY_RUN_DIR" >&2
    exit 2
fi

run_dir=$1
if [[ ! -d $run_dir || -L $run_dir ]]; then
    echo "error: stability run directory is missing or symbolic" >&2
    exit 1
fi
run_dir=$(cd "$run_dir" && pwd -P)

for name in status summary.env samples.tsv block-io.tsv foreground.tsv; do
    path="$run_dir/$name"
    if [[ ! -f $path || -L $path ]]; then
        echo "error: required stability evidence is missing or symbolic: $name" >&2
        exit 1
    fi
done
if [[ -e $run_dir/failures.log || -L $run_dir/failures.log ]]; then
    if [[ ! -f $run_dir/failures.log || -L $run_dir/failures.log ]]; then
        echo "error: stability failure evidence is not a regular file" >&2
        exit 1
    fi
    if [[ -s $run_dir/failures.log ]]; then
        echo "error: stability evidence contains recorded failures" >&2
        exit 1
    fi
fi
if [[ $(wc -l <"$run_dir/status" | tr -d '[:space:]') != 1 ]] ||
    [[ $(<"$run_dir/status") != PASS ]]; then
    echo "error: stability status is not an exact PASS" >&2
    exit 1
fi

units=(
    cardputerzero-compositor
    cardputerzero-system-shell
    cardputerzero-appd
)
summary_keys=(
    run_id
    started_epoch
    finished_epoch
    duration_seconds
    interval_seconds
    failure_count
    sd_baseline_sectors_written
    sd_final_sectors_written
    sd_write_bytes
    maximum_sd_write_bytes
)
for unit in "${units[@]}"; do
    summary_keys+=(
        "${unit}_baseline_memory"
        "${unit}_final_memory"
        "${unit}_restarts"
    )
done

declare -A expected_summary=()
declare -A summary=()
for key in "${summary_keys[@]}"; do
    expected_summary[$key]=1
done
while IFS='=' read -r key value remainder; do
    if [[ -n ${remainder:-} || -z $key || -z $value ||
          -z ${expected_summary[$key]+present} ||
          -n ${summary[$key]+present} ]]; then
        echo "error: summary.env contains an unknown, duplicate, or malformed field" >&2
        exit 1
    fi
    summary[$key]=$value
done <"$run_dir/summary.env"
if ((${#summary[@]} != ${#summary_keys[@]})); then
    echo "error: summary.env is incomplete" >&2
    exit 1
fi
if [[ ! ${summary[run_id]} =~ ^[0-9]{8}T[0-9]{6}Z-[0-9]+$ ]]; then
    echo "error: stability run ID is invalid" >&2
    exit 1
fi
for key in "${summary_keys[@]:1}"; do
    value=${summary[$key]}
    if [[ ! $value =~ ^(0|[1-9][0-9]*)$ ]] ||
        ((${#value} > 16)) || ((value > 9007199254740991)); then
        echo "error: summary field is not a bounded canonical integer: $key" >&2
        exit 1
    fi
done

started=${summary[started_epoch]}
finished=${summary[finished_epoch]}
duration=${summary[duration_seconds]}
interval=${summary[interval_seconds]}
failures=${summary[failure_count]}
sd_baseline=${summary[sd_baseline_sectors_written]}
sd_final=${summary[sd_final_sectors_written]}
sd_bytes=${summary[sd_write_bytes]}
sd_limit=${summary[maximum_sd_write_bytes]}

if ((duration < 1 || interval < 1 || duration < interval)); then
    echo "error: stability duration or interval is invalid" >&2
    exit 1
fi
if ((finished < started || finished - started < duration ||
     finished - started > duration + interval * 2)); then
    echo "error: summary wall-clock duration does not match the requested run" >&2
    exit 1
fi
if ((failures != 0)); then
    echo "error: stability summary reports failures" >&2
    exit 1
fi
if ((sd_final < sd_baseline || (sd_final - sd_baseline) * 512 != sd_bytes ||
     sd_bytes > sd_limit)); then
    echo "error: stability SD write summary is inconsistent or over limit" >&2
    exit 1
fi

sample_stats=$(awk -F '\t' \
    -v started="$started" \
    -v finished="$finished" \
    -v duration="$duration" \
    -v interval="$interval" \
    -v compositor_base="${summary[cardputerzero-compositor_baseline_memory]}" \
    -v compositor_final="${summary[cardputerzero-compositor_final_memory]}" \
    -v compositor_restarts="${summary[cardputerzero-compositor_restarts]}" \
    -v shell_base="${summary[cardputerzero-system-shell_baseline_memory]}" \
    -v shell_final="${summary[cardputerzero-system-shell_final_memory]}" \
    -v shell_restarts="${summary[cardputerzero-system-shell_restarts]}" \
    -v appd_base="${summary[cardputerzero-appd_baseline_memory]}" \
    -v appd_final="${summary[cardputerzero-appd_final_memory]}" \
    -v appd_restarts="${summary[cardputerzero-appd_restarts]}" '
    function reject(message) {
        print "error: samples.tsv " message > "/dev/stderr"
        bad = 1
        exit 1
    }
    function finish_epoch_group() {
        if (current_epoch != "" && group_core_units != 3)
            reject("does not contain exactly three core units at an epoch")
        if (current_epoch != "" && group_stored_units > 1)
            reject("contains duplicate stored service rows at an epoch")
        if (current_epoch != "")
            stored_epoch_groups += group_stored_units
    }
    BEGIN {
        core["cardputerzero-compositor.service"] = 1
        core["cardputerzero-system-shell.service"] = 1
        core["cardputerzero-appd.service"] = 1
        stored = "cardputerzero-stored.service"
        memory_limit["cardputerzero-compositor.service"] = 32 * 1024 * 1024
        memory_limit["cardputerzero-system-shell.service"] = 32 * 1024 * 1024
        memory_limit["cardputerzero-appd.service"] = 24 * 1024 * 1024
        memory_limit[stored] = 40 * 1024 * 1024
        growth_limit["cardputerzero-compositor.service"] = 4 * 1024 * 1024
        growth_limit["cardputerzero-system-shell.service"] = 2 * 1024 * 1024
        growth_limit["cardputerzero-appd.service"] = 4 * 1024 * 1024
        summary_base["cardputerzero-compositor.service"] = compositor_base
        summary_base["cardputerzero-system-shell.service"] = shell_base
        summary_base["cardputerzero-appd.service"] = appd_base
        summary_final["cardputerzero-compositor.service"] = compositor_final
        summary_final["cardputerzero-system-shell.service"] = shell_final
        summary_final["cardputerzero-appd.service"] = appd_final
        summary_restarts["cardputerzero-compositor.service"] = compositor_restarts
        summary_restarts["cardputerzero-system-shell.service"] = shell_restarts
        summary_restarts["cardputerzero-appd.service"] = appd_restarts
    }
    NR == 1 {
        if ($0 != "epoch\tuptime\tunit\tactive\tsub\tpid\trestarts\tmemory_bytes")
            reject("has an invalid header")
        next
    }
    {
        if (NF != 8 || $1 !~ /^[0-9]+$/ ||
            $2 !~ /^[0-9]+([.][0-9]+)?$/ ||
            !(($3 in core) || $3 == stored) ||
            $4 != "active" || $5 != "running" || $6 !~ /^[1-9][0-9]*$/ ||
            $7 !~ /^[0-9]+$/ || $8 !~ /^[0-9]+$/)
            reject("contains a malformed or unhealthy row")

        epoch = $1 + 0
        uptime = $2 + 0
        unit = $3
        if (current_epoch == "" || epoch != current_epoch) {
            finish_epoch_group()
            if (current_epoch != "" &&
                (epoch < current_epoch || epoch - current_epoch > interval * 2 + 5 ||
                 uptime < current_uptime))
                reject("contains a non-monotonic or oversized sampling gap")
            current_epoch = epoch
            current_uptime = uptime
            group_core_units = 0
            group_stored_units = 0
            epoch_groups++
            if (first_epoch == "") {
                first_epoch = epoch
                first_uptime = uptime
            }
            last_epoch = epoch
            last_uptime = uptime
        } else if (uptime != current_uptime) {
            reject("contains inconsistent uptime values for one epoch")
        }
        row_key = epoch SUBSEP unit
        if (row_key in seen)
            reject("contains a duplicate unit row")
        seen[row_key] = 1
        if (unit in core)
            group_core_units++
        else
            group_stored_units++

        if (!(unit in first_memory)) {
            first_memory[unit] = $8 + 0
            first_pid[unit] = $6 + 0
            first_restarts[unit] = $7 + 0
        }
        if (($6 + 0) != first_pid[unit])
            reject("contains a service PID change")
        if (unit in core) {
            if (($7 + 0) != summary_restarts[unit])
                reject("contains a core service restart count change")
        } else if (($7 + 0) != first_restarts[unit]) {
            reject("contains a stored service restart count change")
        }
        if (($8 + 0) > memory_limit[unit])
            reject("exceeds a service memory limit")
        final_memory[unit] = $8 + 0
    }
    END {
        if (bad)
            exit 1
        finish_epoch_group()
        if (NR < 7 || epoch_groups < 2)
            reject("does not contain enough samples")
        if (stored_epoch_groups != 0 && stored_epoch_groups != epoch_groups)
            reject("does not continuously track the stored service")
        if (first_epoch < started || first_epoch - started > interval ||
            last_epoch > finished || finished - last_epoch > interval * 2)
            reject("does not align with summary start and finish times")
        if (last_uptime - first_uptime < duration)
            reject("does not cover the requested monotonic duration")
        for (unit in core) {
            if (first_memory[unit] != summary_base[unit] ||
                final_memory[unit] != summary_final[unit])
                reject("does not match summary memory values")
            if (final_memory[unit] - first_memory[unit] > growth_limit[unit])
                reject("exceeds a core service memory growth limit")
        }
        printf "%d\t%d\t%d\t%.2f\t%.2f\n", epoch_groups,
            first_epoch, last_epoch, first_uptime, last_uptime
    }
' "$run_dir/samples.tsv")

IFS=$'\t' read -r sample_count sample_first_epoch sample_last_epoch \
    sample_first_uptime sample_last_uptime <<<"$sample_stats"

sample_timeline=$(awk -F '\t' '
    NR > 1 && $1 != previous_epoch {
        print $1 "\t" $2
        previous_epoch = $1
    }
' "$run_dir/samples.tsv")
block_timeline=$(awk -F '\t' 'NR > 1 { print $1 "\t" $2 }' \
    "$run_dir/block-io.tsv")
foreground_timeline=$(awk -F '\t' 'NR > 1 { print $1 "\t" $2 }' \
    "$run_dir/foreground.tsv")
if [[ $sample_timeline != "$block_timeline" ||
      $sample_timeline != "$foreground_timeline" ]]; then
    echo "error: service, block-I/O and foreground sampling timelines do not match" >&2
    exit 1
fi

awk -F '\t' \
    -v expected_count="$sample_count" \
    -v expected_first_epoch="$sample_first_epoch" \
    -v expected_last_epoch="$sample_last_epoch" \
    -v expected_first_uptime="$sample_first_uptime" \
    -v expected_last_uptime="$sample_last_uptime" '
    function reject(message) {
        print "error: foreground.tsv " message > "/dev/stderr"
        bad = 1
        exit 1
    }
    NR == 1 {
        if ($0 != "epoch\tuptime\trunning_apps")
            reject("has an invalid header")
        next
    }
    {
        if (NF != 3 || $1 !~ /^[0-9]+$/ ||
            $2 !~ /^[0-9]+([.][0-9]+)?$/ || $3 !~ /^[0-9]+$/)
            reject("contains a malformed row")
        if (($3 + 0) != 0)
            reject("records a running foreground application")
        if (rows > 0 && (($1 + 0) <= last_epoch || ($2 + 0) < last_uptime))
            reject("contains a non-monotonic or duplicate sample")
        if (rows == 0) {
            first_epoch = $1 + 0
            first_uptime = $2 + 0
        }
        last_epoch = $1 + 0
        last_uptime = $2 + 0
        rows++
    }
    END {
        if (bad)
            exit 1
        if (rows != expected_count || first_epoch != expected_first_epoch ||
            last_epoch != expected_last_epoch ||
            first_uptime != expected_first_uptime ||
            last_uptime != expected_last_uptime)
            reject("does not align one-to-one with service samples")
    }
' "$run_dir/foreground.tsv"

awk -F '\t' \
    -v expected_count="$sample_count" \
    -v expected_first_epoch="$sample_first_epoch" \
    -v expected_last_epoch="$sample_last_epoch" \
    -v expected_first_uptime="$sample_first_uptime" \
    -v expected_last_uptime="$sample_last_uptime" \
    -v summary_baseline="$sd_baseline" \
    -v summary_final="$sd_final" \
    -v summary_bytes="$sd_bytes" \
    -v maximum_bytes="$sd_limit" '
    function reject(message) {
        print "error: block-io.tsv " message > "/dev/stderr"
        bad = 1
        exit 1
    }
    NR == 1 {
        if ($0 != "epoch\tuptime\tsectors_written\tbytes_written")
            reject("has an invalid header")
        next
    }
    {
        if (NF != 4 || $1 !~ /^[0-9]+$/ ||
            $2 !~ /^[0-9]+([.][0-9]+)?$/ || $3 !~ /^[0-9]+$/ ||
            $4 !~ /^[0-9]+$/ || ($3 + 0) * 512 != ($4 + 0))
            reject("contains a malformed row")
        if (rows > 0 && (($1 + 0) < last_epoch || ($2 + 0) < last_uptime ||
                         ($3 + 0) < last_sectors))
            reject("contains a non-monotonic counter")
        if (rows == 0) {
            first_epoch = $1 + 0
            first_uptime = $2 + 0
            first_sectors = $3 + 0
        }
        last_epoch = $1 + 0
        last_uptime = $2 + 0
        last_sectors = $3 + 0
        rows++
    }
    END {
        if (bad)
            exit 1
        if (rows != expected_count || first_epoch != expected_first_epoch ||
            last_epoch != expected_last_epoch ||
            first_uptime != expected_first_uptime ||
            last_uptime != expected_last_uptime)
            reject("does not align one-to-one with service samples")
        if (first_sectors != summary_baseline || last_sectors != summary_final ||
            (last_sectors - first_sectors) * 512 != summary_bytes ||
            summary_bytes > maximum_bytes)
            reject("does not match the bounded SD write summary")
    }
' "$run_dir/block-io.tsv"

echo "PASS independently verified stability evidence: $run_dir"
