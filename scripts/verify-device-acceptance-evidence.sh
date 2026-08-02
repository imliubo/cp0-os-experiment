#!/usr/bin/env bash
set -euo pipefail

die() {
    echo "error: $*" >&2
    exit 1
}

usage() {
    cat >&2 <<'EOF'
usage:
  verify-device-acceptance-evidence.sh factory RUN_DIR
  verify-device-acceptance-evidence.sh performance RUN_DIR
  verify-device-acceptance-evidence.sh capability FULL_DIR PERSISTENCE_DIR
  verify-device-acceptance-evidence.sh store REFRESH_V1 RESUME_V1 REFRESH_V2 UPGRADE_V2 OFFLINE_V2 STALE_V2
EOF
    exit 2
}

declare -A summary=()
declare -A check_pass=()
declare -A check_warn=()
declare -A check_fail=()
declare -A check_detail=()
check_rows=0
check_warnings=0
check_failures=0

require_regular() {
    [[ -f $1 && ! -L $1 ]] || die "required evidence is missing or symbolic: $1"
}

load_summary() {
    local dir=$1 line key value field
    shift
    require_regular "$dir/summary.env"
    summary=()
    while IFS= read -r line || [[ -n $line ]]; do
        [[ $line =~ ^([a-z][a-z0-9_]*)=([A-Za-z0-9._:/+-]+)$ ]] ||
            die "invalid summary field in $dir"
        key=${BASH_REMATCH[1]}
        value=${BASH_REMATCH[2]}
        [[ ! ${summary[$key]+present} ]] || die "duplicate summary field: $key"
        summary[$key]=$value
    done <"$dir/summary.env"
    ((${#summary[@]} == $#)) || die "unexpected summary field set in $dir"
    for field in "$@"; do
        [[ ${summary[$field]+present} ]] || die "missing summary field: $field"
    done
    [[ ${summary[run_id]} == "$(basename "$dir")" ]] ||
        die "summary run_id does not match directory"
    [[ ${summary[run_id]} =~ ^[0-9]{8}T[0-9]{6}Z-[1-9][0-9]*$ ]] ||
        die "invalid run_id"
    [[ ${summary[finished_epoch]} =~ ^[1-9][0-9]*$ ]] ||
        die "invalid finished_epoch"
    [[ ${summary[failure_count]} == 0 ]] || die "summary reports failures"
    [[ ${summary[warning_count]} =~ ^[0-9]+$ ]] || die "invalid warning_count"
}

load_checks() {
    local dir=$1 header result name detail extra
    require_regular "$dir/checks.tsv"
    IFS= read -r header <"$dir/checks.tsv" || die "checks file is empty"
    [[ $header == $'result\tcheck\tdetail' ]] || die "invalid checks header"
    check_pass=()
    check_warn=()
    check_fail=()
    check_detail=()
    check_rows=0
    check_warnings=0
    check_failures=0
    while IFS=$'\t' read -r result name detail extra; do
        [[ -n $result && -n $name && -z ${extra:-} ]] ||
            die "invalid checks row in $dir"
        [[ $result == PASS || $result == WARN || $result == FAIL ]] ||
            die "unknown check result: $result"
        [[ $name =~ ^[A-Za-z0-9_./:-]+$ ]] || die "invalid check name: $name"
        check_rows=$((check_rows + 1))
        check_detail[$name]=$detail
        case "$result" in
            PASS) check_pass[$name]=$((${check_pass[$name]:-0} + 1)) ;;
            WARN)
                check_warn[$name]=$((${check_warn[$name]:-0} + 1))
                check_warnings=$((check_warnings + 1))
                ;;
            FAIL)
                check_fail[$name]=$((${check_fail[$name]:-0} + 1))
                check_failures=$((check_failures + 1))
                ;;
        esac
    done < <(tail -n +2 "$dir/checks.tsv")
    ((check_rows > 0)) || die "checks file has no rows"
    ((check_failures == 0)) || die "checks file contains FAIL rows"
}

load_base() {
    local dir=$1
    [[ -d $dir && ! -L $dir ]] || die "run directory is missing or symbolic: $dir"
    require_regular "$dir/status"
    [[ $(cat "$dir/status") == PASS ]] || die "status is not an exact PASS"
    load_checks "$dir"
}

require_pass() {
    [[ ${check_pass[$1]:-0} -ge ${2:-1} ]] || die "required PASS check is missing: $1"
}

require_pass_or_warn() {
    (( ${check_pass[$1]:-0} + ${check_warn[$1]:-0} >= 1 )) ||
        die "required PASS/WARN check is missing: $1"
}

validate_common_counts() {
    local factory_mode=${1:-no}
    [[ ${summary[failure_count]} == "$check_failures" ]] ||
        die "failure count does not match checks"
    if [[ $factory_mode == yes ]]; then
        ((summary[warning_count] >= check_warnings)) ||
            die "factory warning count is smaller than checks"
    else
        [[ ${summary[warning_count]} == "$check_warnings" ]] ||
            die "warning count does not match checks"
    fi
}

validate_factory() {
    local dir=$1 item smoke_summary smoke_warnings
    load_base "$dir"
    load_summary "$dir" schema run_id finished_epoch failure_count warning_count \
        mem_available_bytes sd_sectors_written
    [[ ${summary[schema]} == cardputerzero-factory-v1 ]] || die "wrong factory schema"
    [[ ${summary[mem_available_bytes]} =~ ^[1-9][0-9]*$ ]] || die "invalid available memory"
    [[ ${summary[sd_sectors_written]} =~ ^[0-9]+$ ]] || die "invalid SD counter"
    require_regular "$dir/hardware-smoke.txt"
    [[ -s $dir/hardware-smoke.txt ]] || die "hardware smoke evidence is empty"
    smoke_summary=$(tail -n 1 "$dir/hardware-smoke.txt")
    [[ $smoke_summary =~ warnings=([0-9]+) ]] ||
        die "hardware smoke summary does not contain a warning count"
    smoke_warnings=${BASH_REMATCH[1]}
    for item in hardware-smoke image-profile immutable-root data-partition \
        data-expanded data-filesystem-expanded data-layout \
        default-mode:developer-mode default-mode:recovery-mode appd-control failed-units; do
        require_pass "$item"
    done
    for item in cardputerzero-overlay-root-status.service \
        cardputerzero-compositor.service cardputerzero-system-shell.service \
        cardputerzero-appd.service seatd.service cardputerzero-appd.socket \
        cardputerzero-broker.socket cardputerzero-networkd.socket \
        cardputerzero-documentd.socket cardputerzero-audiod.socket \
        cardputerzero-camerad.socket cardputerzero-displayd.socket \
        cardputerzero-gpiod.socket \
        cardputerzero-radiod.socket cardputerzero-storaged.socket \
        cardputerzero-stored.socket; do
        require_pass "unit:$item"
    done
    for item in appd runtime network documents audio camera display gpio radio storage store; do
        require_pass "socket:$item"
    done
    [[ ${summary[warning_count]} == $((check_warnings + smoke_warnings)) ]] ||
        die "factory warning count does not match hardware smoke and checks"
    validate_common_counts yes
    printf 'PASS independently verified factory evidence: %s\n' "$dir"
}

validate_performance() {
    local dir=$1 stats line key value header unit start_pid end_pid start_cpu end_cpu
    local cpu max_memory start_restarts end_restarts extra unit_count=0 total_delta=0
    declare -A seen_unit=()
    load_base "$dir"
    load_summary "$dir" schema run_id finished_epoch duration_seconds interval_seconds \
        elapsed_seconds \
        failure_count warning_count boot_ready_ms shell_ready_ms \
        maximum_idle_used_bytes minimum_idle_available_bytes core_cpu_millipercent \
        sd_baseline_sectors_written sd_final_sectors_written sd_write_bytes \
        battery_sample_count battery_average_estimated_uw maximum_boot_ready_ms \
        maximum_idle_used_bytes_limit minimum_idle_available_bytes_limit \
        maximum_core_cpu_millipercent maximum_short_sd_write_bytes
    [[ ${summary[schema]} == cardputerzero-performance-v1 ]] || die "wrong performance schema"
    [[ ${summary[duration_seconds]} =~ ^[1-9][0-9]*$ &&
       ${summary[interval_seconds]} =~ ^[1-9][0-9]*$ &&
       ${summary[elapsed_seconds]} =~ ^[1-9][0-9]*$ ]] ||
        die "invalid performance duration"
    ((summary[duration_seconds] >= summary[interval_seconds] &&
      summary[duration_seconds] <= 3600 &&
      summary[elapsed_seconds] >= summary[duration_seconds])) ||
        die "invalid performance duration bounds"
    [[ ${summary[maximum_boot_ready_ms]} == 35000 &&
       ${summary[maximum_idle_used_bytes_limit]} == 188743680 &&
       ${summary[minimum_idle_available_bytes_limit]} == 209715200 &&
       ${summary[maximum_core_cpu_millipercent]} == 10000 &&
       ${summary[maximum_short_sd_write_bytes]} == 1048576 ]] ||
        die "performance limits do not match the V0.6 release contract"
    for key in boot_ready_ms shell_ready_ms maximum_idle_used_bytes \
        minimum_idle_available_bytes core_cpu_millipercent sd_baseline_sectors_written \
        sd_final_sectors_written sd_write_bytes battery_sample_count; do
        [[ ${summary[$key]} =~ ^[0-9]+$ ]] || die "invalid numeric performance field: $key"
    done
    ((summary[boot_ready_ms] > 0 && summary[boot_ready_ms] <= 35000 &&
      summary[shell_ready_ms] > 0 && summary[shell_ready_ms] <= 35000 &&
      summary[maximum_idle_used_bytes] <= 188743680 &&
      summary[minimum_idle_available_bytes] >= 209715200 &&
      summary[core_cpu_millipercent] <= 10000 &&
      summary[sd_write_bytes] <= 1048576)) || die "performance threshold exceeded"

    require_regular "$dir/samples.tsv"
    stats=$(awk -F '\t' -v duration="${summary[duration_seconds]}" '
        NR == 1 {
            expected="epoch\tuptime_seconds\tmem_available_bytes\tmem_used_bytes\tdirty_bytes\twriteback_bytes\tmmc_sectors_written\tvoltage_uv\tcurrent_ua\testimated_battery_uw"
            if ($0 != expected) exit 10
            next
        }
        NF != 10 { exit 11 }
        $1 !~ /^[0-9]+$/ || $2 !~ /^[0-9]+([.][0-9]+)?$/ ||
        $3 !~ /^[0-9]+$/ || $4 !~ /^[0-9]+$/ || $5 !~ /^[0-9]+$/ ||
        $6 !~ /^[0-9]+$/ || $7 !~ /^[0-9]+$/ { exit 12 }
        rows == 0 { first_epoch=$1; first_up=$2; first_sd=$7; min_avail=$3; max_used=$4 }
        rows > 0 && ($1 < last_epoch || $2 < last_up || $7 < last_sd) { exit 13 }
        $3 < min_avail { min_avail=$3 }
        $4 > max_used { max_used=$4 }
        $10 ~ /^[0-9]+$/ { battery_count++; battery_sum += $10 }
        $8 != "unknown" && $8 !~ /^[0-9]+$/ { exit 14 }
        $9 != "unknown" && $9 !~ /^-?[0-9]+$/ { exit 15 }
        $10 != "unknown" && $10 !~ /^[0-9]+$/ { exit 16 }
        { rows++; last_epoch=$1; last_up=$2; last_sd=$7 }
        END {
            if (rows < 2 || last_epoch - first_epoch < duration) exit 17
            print "rows=" rows
            print "span=" last_epoch-first_epoch
            print "max_used=" max_used
            print "min_avail=" min_avail
            print "first_sd=" first_sd
            print "last_sd=" last_sd
            print "sd_bytes=" (last_sd-first_sd)*512
            print "battery_count=" battery_count+0
            if (battery_count > 0) print "battery_average=" int(battery_sum/battery_count)
            else print "battery_average=unknown"
            print "last_epoch=" last_epoch
        }
    ' "$dir/samples.tsv") || die "invalid performance samples"
    declare -A sample=()
    while IFS='=' read -r key value; do sample[$key]=$value; done <<<"$stats"
    [[ ${sample[max_used]} == ${summary[maximum_idle_used_bytes]} &&
       ${sample[min_avail]} == ${summary[minimum_idle_available_bytes]} &&
       ${sample[first_sd]} == ${summary[sd_baseline_sectors_written]} &&
       ${sample[last_sd]} == ${summary[sd_final_sectors_written]} &&
       ${sample[sd_bytes]} == ${summary[sd_write_bytes]} &&
       ${sample[battery_count]} == ${summary[battery_sample_count]} &&
       ${sample[battery_average]} == ${summary[battery_average_estimated_uw]} ]] ||
        die "performance summary does not match raw samples"
    ((summary[finished_epoch] >= sample[last_epoch])) || die "invalid performance finish time"

    require_regular "$dir/services.tsv"
    IFS= read -r header <"$dir/services.tsv" || die "services file is empty"
    [[ $header == $'unit\tstart_pid\tend_pid\tstart_cpu_ns\tend_cpu_ns\tcpu_millipercent\tmax_memory_bytes\tstart_restarts\tend_restarts' ]] ||
        die "invalid services header"
    while IFS=$'\t' read -r unit start_pid end_pid start_cpu end_cpu cpu max_memory \
        start_restarts end_restarts extra; do
        [[ -z ${extra:-} && $unit =~ ^cardputerzero-(compositor|system-shell|appd)\.service$ ]] ||
            die "invalid performance service row"
        [[ ! ${seen_unit[$unit]+present} ]] || die "duplicate performance service"
        seen_unit[$unit]=1
        for value in "$start_pid" "$end_pid" "$start_cpu" "$end_cpu" "$cpu" \
            "$max_memory" "$start_restarts" "$end_restarts"; do
            [[ $value =~ ^[0-9]+$ ]] || die "nonnumeric performance service value"
        done
        ((start_pid > 0 && start_pid == end_pid && start_restarts == 0 &&
          end_restarts == 0 && end_cpu >= start_cpu)) || die "service continuity failed"
        case "$unit" in
            cardputerzero-appd.service) ((max_memory <= 25165824)) || die "appd memory exceeded" ;;
            *) ((max_memory <= 33554432)) || die "core memory exceeded" ;;
        esac
        value=$(((end_cpu - start_cpu) * 100000 / (summary[elapsed_seconds] * 1000000000)))
        [[ $value == "$cpu" ]] || die "service CPU row does not match counters"
        total_delta=$((total_delta + end_cpu - start_cpu))
        unit_count=$((unit_count + 1))
    done < <(tail -n +2 "$dir/services.tsv")
    ((unit_count == 3)) || die "performance evidence must contain three core services"
    value=$((total_delta * 100000 / (summary[elapsed_seconds] * 1000000000)))
    [[ $value == ${summary[core_cpu_millipercent]} ]] || die "aggregate CPU does not match services"
    for key in stability-interlock image-profile foreground-precondition boot-ready shell-ready \
        idle-used-memory idle-available-memory core-idle-cpu short-sd-write; do
        require_pass "$key"
    done
    for unit in cardputerzero-compositor.service cardputerzero-system-shell.service \
        cardputerzero-appd.service; do
        require_pass "unit-active:$unit"
        require_pass "continuity:$unit"
        require_pass "memory:$unit"
    done
    require_pass_or_warn battery-telemetry
    validate_common_counts
    printf 'PASS independently verified performance evidence: %s\n' "$dir"
}

validate_capability_dir() {
    local dir=$1 expected_mode=$2 app property path
    load_base "$dir"
    load_summary "$dir" schema run_id mode boot_id finished_epoch failure_count warning_count
    [[ ${summary[schema]} == cardputerzero-capability-v1 &&
       ${summary[mode]} == "$expected_mode" ]] || die "wrong capability schema or mode"
    [[ ${summary[boot_id]} =~ ^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$ ]] ||
        die "invalid capability boot_id"
    require_pass stability-interlock
    require_pass installed:dev.cardputerzero.acceptance
    require_pass foreground-precondition
    for property in CPUQuotaPerSecUSec CPUWeight MemoryMax MemorySwapMax TasksMax; do
        require_pass "resource-limit:dev.cardputerzero.acceptance:$property"
    done
    for path in /sys/class/leds/grove_fun/brightness \
        /sys/class/leds/ext_usb_gpio_fun/brightness \
        /sys/class/leds/grove_5v_out/brightness \
        /sys/class/leds/ext_5v_out/brightness; do
        require_pass "gpio-sysfs:$path"
        require_pass "gpio-bypass:$path"
    done
    require_pass storage-root-mode
    require_pass storage-directory:/var/lib/cardputerzero/data/dev.cardputerzero.acceptance
    require_pass storage-entries:/var/lib/cardputerzero/data/dev.cardputerzero.acceptance
    if [[ $expected_mode == full ]]; then
        require_pass installed:dev.cardputerzero.isolation
        for property in CPUQuotaPerSecUSec CPUWeight MemoryMax MemorySwapMax TasksMax; do
            require_pass "resource-limit:dev.cardputerzero.isolation:$property"
        done
        for app in audio.playback audio.capture hardware.gpio notifications.post; do
            require_pass "permission-reset:dev.cardputerzero.acceptance:$app" 2
        done
        require_pass capability-denial
        require_pass audio-playback-broker
        if [[ ${check_pass[audio-capture-broker]:-0} == 0 ]]; then
            [[ ${check_warn[audio-capture-signal]:-0} -ge 1 ]] ||
                die "silent audio capture warning is missing"
        fi
        require_pass gpio-read-write-restore
        require_pass storage-quota-and-restart
        require_pass_or_warn audio-observed
        require_pass storage-cross-app-isolation
        require_pass storage-directory:/var/lib/cardputerzero/data/dev.cardputerzero.isolation
        require_pass storage-entries:/var/lib/cardputerzero/data/dev.cardputerzero.isolation
        require_regular "$dir/result-acceptance-deny.txt"
        require_regular "$dir/result-acceptance-allow.txt"
        require_regular "$dir/result-isolation-allow.txt"
        [[ $(cat "$dir/result-acceptance-deny.txt") == \
            'audio-play=denied;audio-capture=denied;gpio=denied;storage=persist-ok' ]] ||
            die "invalid denied capability result"
        [[ $(cat "$dir/result-acceptance-allow.txt") == audio-play=ok\;audio-capture=ok-*\;gpio=ok\;storage=persist-ok ]] ||
            die "invalid allowed capability result"
        [[ $(cat "$dir/result-isolation-allow.txt") == storage-isolation=ok ]] ||
            die "invalid isolation result"
    else
        require_pass storage-reboot-persistence
        require_regular "$dir/result-acceptance-persistence.txt"
        [[ $(cat "$dir/result-acceptance-persistence.txt") == *';storage=persist-ok' ]] ||
            die "invalid persistence result"
    fi
    validate_common_counts
}

validate_capability() {
    local full=$1 persistence=$2 full_boot full_finish
    validate_capability_dir "$full" full
    full_boot=${summary[boot_id]}
    full_finish=${summary[finished_epoch]}
    validate_capability_dir "$persistence" persistence
    [[ ${summary[boot_id]} != "$full_boot" ]] || die "capability persistence used the same boot"
    ((summary[finished_epoch] > full_finish)) || die "persistence evidence predates full run"
    printf 'PASS independently verified capability evidence pair: %s %s\n' "$full" "$persistence"
}

validate_store_dir() {
    local dir=$1 expected_action=$2 expected_version=$3 expected_sequence=$4
    load_base "$dir"
    load_summary "$dir" schema run_id action expected_version expected_sequence \
        finished_epoch failure_count warning_count
    if [[ $expected_sequence == self ]]; then
        expected_sequence=${summary[expected_sequence]}
    fi
    [[ ${summary[schema]} == cardputerzero-store-acceptance-v1 &&
       ${summary[action]} == "$expected_action" &&
       ${summary[expected_version]} == "$expected_version" &&
       ${summary[expected_sequence]} == "$expected_sequence" ]] ||
        die "wrong Store schema or action metadata"
    require_pass stability-interlock
    require_pass device-dependency
    require_pass store-config
    require_pass store-trust
    require_pass foreground-precondition
    require_pass metrics-default-off
    require_pass store-cache-mode
    require_regular "$dir/app-list.json"
    require_regular "$dir/metrics.json"
    jq -e '.outcome.status == "ok" and
        .outcome.data.kind == "metrics-status" and
        .outcome.data.enabled == false and .outcome.data.pending == false and
        (.outcome.data.policy_allowed | type) == "boolean" and
        (.outcome.data.configured | type) == "boolean"' "$dir/metrics.json" >/dev/null ||
        die "invalid Store metrics evidence"
    jq -e '.outcome.status == "ok" and .outcome.data.kind == "applications" and
        (.outcome.data.apps | type == "array") and
        (.outcome.data.apps | all(.running == false))' "$dir/app-list.json" >/dev/null ||
        die "invalid Store foreground evidence"
    case "$expected_action" in
        refresh-v1 | refresh-v2)
            require_pass refresh-accepted
            require_pass catalog-visible
            require_regular "$dir/catalog.json"
            jq -e --argjson sequence "$expected_sequence" --arg version "$expected_version" '
                .outcome.status == "ok" and .outcome.data.kind == "catalog" and
                .outcome.data.sequence == $sequence and .outcome.data.stale == false and
                (.outcome.data.apps | length) == 1 and
                .outcome.data.apps[0].app_id == "dev.cardputerzero.store-test" and
                .outcome.data.apps[0].version == $version and
                (.outcome.data.apps[0].package_bytes | type == "number") and
                .outcome.data.apps[0].package_bytes > 0
            ' "$dir/catalog.json" >/dev/null || die "invalid Store catalog evidence"
            ;;
        resume-v1)
            for expected_action in install-accepted partial-created store-restart \
                partial-survived resume-accepted installed-version range-resume installed-launch \
                runtime-observer runtime-observer-stopped; do
                require_pass "$expected_action"
            done
            [[ ${check_detail[partial-created]} =~ ^[1-9][0-9]*\ bytes$ &&
               ${check_detail[partial-survived]} =~ ^[1-9][0-9]*\ bytes$ &&
               ${check_detail[store-restart]} =~ ^main\ PID\ ([1-9][0-9]*)\ -\>\ ([1-9][0-9]*)$ &&
               ${BASH_REMATCH[1]} != ${BASH_REMATCH[2]} ]] ||
                die "invalid Store resume continuity details"
            ;;
        upgrade-v2)
            require_pass upgrade-accepted
            require_pass installed-version
            require_pass installed-launch
            require_pass runtime-observer
            require_pass runtime-observer-stopped
            ;;
        offline-v2)
            require_pass offline-cache-before
            require_pass offline-refresh
            require_pass offline-cache-after
            ;;
        stale-v2)
            require_pass stale-catalog
            require_pass stale-install-rejected
            ;;
    esac
    validate_common_counts
}

mode=${1:-}
case "$mode" in
    factory)
        (($# == 2)) || usage
        validate_factory "$2"
        ;;
    performance)
        (($# == 2)) || usage
        validate_performance "$2"
        ;;
    capability)
        (($# == 3)) || usage
        validate_capability "$2" "$3"
        ;;
    store)
        (($# == 7)) || usage
        command -v jq >/dev/null 2>&1 || die "jq is required for Store evidence"
        # Sequence values are read from the two refresh summaries, then bound to
        # the offline and stale evidence instead of being accepted as arguments.
        validate_store_dir "$2" refresh-v1 1.0.0 self
        store_v1_sequence=${summary[expected_sequence]}
        store_finish=${summary[finished_epoch]}
        store_package_bytes=$(jq -r '.outcome.data.apps[0].package_bytes' "$2/catalog.json")
        validate_store_dir "$3" resume-v1 1.0.0 not-applicable
        ((summary[finished_epoch] >= store_finish)) || die "Store evidence is out of order"
        store_created=${check_detail[partial-created]%% *}
        store_survived=${check_detail[partial-survived]%% *}
        ((store_created < store_package_bytes && store_survived < store_package_bytes)) ||
            die "Store resume evidence does not contain a partial download"
        store_finish=${summary[finished_epoch]}
        validate_store_dir "$4" refresh-v2 1.1.0 self
        store_v2_sequence=${summary[expected_sequence]}
        [[ $store_v1_sequence =~ ^[1-9][0-9]*$ && $store_v2_sequence =~ ^[1-9][0-9]*$ ]] ||
            die "invalid Store catalog sequences"
        ((store_v2_sequence > store_v1_sequence && summary[finished_epoch] >= store_finish)) ||
            die "Store v2 sequence or run order is invalid"
        store_finish=${summary[finished_epoch]}
        validate_store_dir "$5" upgrade-v2 1.1.0 not-applicable
        ((summary[finished_epoch] >= store_finish)) || die "Store evidence is out of order"
        store_finish=${summary[finished_epoch]}
        validate_store_dir "$6" offline-v2 1.1.0 "$store_v2_sequence"
        ((summary[finished_epoch] >= store_finish)) || die "Store evidence is out of order"
        store_finish=${summary[finished_epoch]}
        validate_store_dir "$7" stale-v2 1.1.0 "$store_v2_sequence"
        ((summary[finished_epoch] > store_finish)) || die "stale Store evidence is out of order"
        printf 'PASS independently verified Store acceptance sequence: %s -> %s\n' \
            "$store_v1_sequence" "$store_v2_sequence"
        ;;
    *) usage ;;
esac
