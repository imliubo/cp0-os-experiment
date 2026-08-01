#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
verifier="$repo_root/scripts/verify-device-acceptance-evidence.sh"
test_parent="$repo_root/target/test-tmp"
mkdir -p "$test_parent"
test_root=$(mktemp -d "$test_parent/cp0-device-acceptance-evidence.XXXXXX")
case "$test_root" in
    "$test_parent"/cp0-device-acceptance-evidence.*) ;;
    *) echo "error: unsafe test directory" >&2; exit 1 ;;
esac
trap 'rm -rf -- "$test_root"' EXIT

new_run() {
    local run_id=$1 dir="$test_root/$1"
    mkdir -p "$dir"
    printf 'PASS\n' >"$dir/status"
    printf 'result\tcheck\tdetail\n' >"$dir/checks.tsv"
    printf '%s\n' "$dir"
}

check() {
    printf '%s\t%s\t%s\n' "$2" "$3" "${4:-ok}" >>"$1/checks.tsv"
}

expect_fail() {
    local label=$1
    shift
    if "$@" >/dev/null 2>&1; then
        echo "error: invalid evidence passed: $label" >&2
        exit 1
    fi
}

factory=$(new_run 20260801T010000Z-100)
printf 'PASS device smoke warnings=0\n' >"$factory/hardware-smoke.txt"
cat >"$factory/summary.env" <<'EOF'
schema=cardputerzero-factory-v1
run_id=20260801T010000Z-100
finished_epoch=1000000
failure_count=0
warning_count=0
mem_available_bytes=300000000
sd_sectors_written=1000
EOF
for name in hardware-smoke image-profile immutable-root data-partition data-expanded \
    data-filesystem-expanded data-layout default-mode:developer-mode \
    default-mode:recovery-mode appd-control failed-units; do
    check "$factory" PASS "$name"
done
for unit in cardputerzero-overlay-root-status.service \
    cardputerzero-compositor.service cardputerzero-system-shell.service \
    cardputerzero-appd.service seatd.service cardputerzero-appd.socket \
    cardputerzero-broker.socket cardputerzero-networkd.socket \
    cardputerzero-documentd.socket cardputerzero-audiod.socket \
    cardputerzero-camerad.socket cardputerzero-gpiod.socket \
    cardputerzero-radiod.socket cardputerzero-storaged.socket \
    cardputerzero-stored.socket; do
    check "$factory" PASS "unit:$unit"
done
for socket in appd runtime network documents audio camera gpio radio storage store; do
    check "$factory" PASS "socket:$socket"
done
"$verifier" factory "$factory" >/dev/null

performance=$(new_run 20260801T020000Z-200)
cat >"$performance/summary.env" <<'EOF'
schema=cardputerzero-performance-v1
run_id=20260801T020000Z-200
finished_epoch=1000061
duration_seconds=60
interval_seconds=5
elapsed_seconds=60
failure_count=0
warning_count=1
boot_ready_ms=20000
shell_ready_ms=19000
maximum_idle_used_bytes=100000000
minimum_idle_available_bytes=300000000
core_cpu_millipercent=10000
sd_baseline_sectors_written=1000
sd_final_sectors_written=1000
sd_write_bytes=0
battery_sample_count=0
battery_average_estimated_uw=unknown
maximum_boot_ready_ms=35000
maximum_idle_used_bytes_limit=188743680
minimum_idle_available_bytes_limit=209715200
maximum_core_cpu_millipercent=10000
maximum_short_sd_write_bytes=1048576
EOF
for name in stability-interlock image-profile foreground-precondition boot-ready shell-ready \
    idle-used-memory idle-available-memory core-idle-cpu short-sd-write; do
    check "$performance" PASS "$name"
done
for unit in cardputerzero-compositor.service cardputerzero-system-shell.service \
    cardputerzero-appd.service; do
    check "$performance" PASS "unit-active:$unit"
    check "$performance" PASS "continuity:$unit"
    check "$performance" PASS "memory:$unit"
done
check "$performance" WARN battery-telemetry unavailable
cat >"$performance/samples.tsv" <<'EOF'
epoch	uptime_seconds	mem_available_bytes	mem_used_bytes	dirty_bytes	writeback_bytes	mmc_sectors_written	voltage_uv	current_ua	estimated_battery_uw
1000000	100.0	310000000	90000000	0	0	1000	unknown	unknown	unknown
1000005	105.0	309000000	91000000	0	0	1000	unknown	unknown	unknown
1000010	110.0	308000000	92000000	0	0	1000	unknown	unknown	unknown
1000015	115.0	307000000	93000000	0	0	1000	unknown	unknown	unknown
1000020	120.0	306000000	94000000	0	0	1000	unknown	unknown	unknown
1000025	125.0	305000000	95000000	0	0	1000	unknown	unknown	unknown
1000030	130.0	304000000	96000000	0	0	1000	unknown	unknown	unknown
1000035	135.0	303000000	97000000	0	0	1000	unknown	unknown	unknown
1000040	140.0	302000000	98000000	0	0	1000	unknown	unknown	unknown
1000045	145.0	301000000	99000000	0	0	1000	unknown	unknown	unknown
1000050	150.0	300000000	100000000	0	0	1000	unknown	unknown	unknown
1000055	155.0	301000000	99000000	0	0	1000	unknown	unknown	unknown
1000060	160.0	302000000	98000000	0	0	1000	unknown	unknown	unknown
EOF
cat >"$performance/services.tsv" <<'EOF'
unit	start_pid	end_pid	start_cpu_ns	end_cpu_ns	cpu_millipercent	max_memory_bytes	start_restarts	end_restarts
cardputerzero-compositor.service	10	10	1000000000	4000000000	5000	8000000	0	0
cardputerzero-system-shell.service	11	11	1000000000	2000000000	1666	2000000	0	0
cardputerzero-appd.service	12	12	1000000000	3000000000	3333	3000000	0	0
EOF
"$verifier" performance "$performance" >/dev/null

make_capability() {
    local run_id=$1 mode=$2 boot_id=$3 finished=$4 warnings=$5 dir
    dir=$(new_run "$run_id")
    cat >"$dir/summary.env" <<EOF
schema=cardputerzero-capability-v1
run_id=$run_id
mode=$mode
boot_id=$boot_id
finished_epoch=$finished
failure_count=0
warning_count=$warnings
EOF
    check "$dir" PASS stability-interlock
    check "$dir" PASS installed:dev.cardputerzero.acceptance
    check "$dir" PASS foreground-precondition
    for property in CPUQuotaPerSecUSec CPUWeight MemoryMax MemorySwapMax TasksMax; do
        check "$dir" PASS "resource-limit:dev.cardputerzero.acceptance:$property"
    done
    for path in /sys/class/leds/grove_fun/brightness \
        /sys/class/leds/ext_usb_gpio_fun/brightness \
        /sys/class/leds/grove_5v_out/brightness \
        /sys/class/leds/ext_5v_out/brightness; do
        check "$dir" PASS "gpio-sysfs:$path"
        check "$dir" PASS "gpio-bypass:$path"
    done
    check "$dir" PASS storage-root-mode
    check "$dir" PASS storage-directory:/var/lib/cardputerzero/data/dev.cardputerzero.acceptance
    check "$dir" PASS storage-entries:/var/lib/cardputerzero/data/dev.cardputerzero.acceptance
    if [[ $mode == full ]]; then
        check "$dir" PASS installed:dev.cardputerzero.isolation
        for property in CPUQuotaPerSecUSec CPUWeight MemoryMax MemorySwapMax TasksMax; do
            check "$dir" PASS "resource-limit:dev.cardputerzero.isolation:$property"
        done
        for permission in audio.playback audio.capture hardware.gpio notifications.post; do
            check "$dir" PASS "permission-reset:dev.cardputerzero.acceptance:$permission"
            check "$dir" PASS "permission-reset:dev.cardputerzero.acceptance:$permission"
        done
        check "$dir" PASS capability-denial
        check "$dir" PASS audio-playback-broker
        check "$dir" WARN audio-capture-signal silent
        check "$dir" PASS gpio-read-write-restore
        check "$dir" PASS storage-quota-and-restart
        check "$dir" WARN audio-observed unconfirmed
        check "$dir" PASS storage-cross-app-isolation
        check "$dir" PASS storage-directory:/var/lib/cardputerzero/data/dev.cardputerzero.isolation
        check "$dir" PASS storage-entries:/var/lib/cardputerzero/data/dev.cardputerzero.isolation
        printf '%s\n' 'audio-play=denied;audio-capture=denied;gpio=denied;storage=persist-ok' \
            >"$dir/result-acceptance-deny.txt"
        printf '%s\n' 'audio-play=ok;audio-capture=ok-silent;gpio=ok;storage=persist-ok' \
            >"$dir/result-acceptance-allow.txt"
        printf '%s\n' storage-isolation=ok >"$dir/result-isolation-allow.txt"
    else
        check "$dir" PASS storage-reboot-persistence
        printf '%s\n' 'audio-play=ok;audio-capture=ok-silent;gpio=ok;storage=persist-ok' \
            >"$dir/result-acceptance-persistence.txt"
    fi
    printf '%s\n' "$dir"
}

cap_full=$(make_capability 20260801T030000Z-300 full \
    11111111-1111-4111-8111-111111111111 1000100 2)
cap_persist=$(make_capability 20260801T040000Z-400 persistence \
    22222222-2222-4222-8222-222222222222 1000200 0)
"$verifier" capability "$cap_full" "$cap_persist" >/dev/null

make_store() {
    local run_id=$1 action=$2 version=$3 sequence=$4 finished=$5 dir
    dir=$(new_run "$run_id")
    cat >"$dir/summary.env" <<EOF
schema=cardputerzero-store-acceptance-v1
run_id=$run_id
action=$action
expected_version=$version
expected_sequence=$sequence
finished_epoch=$finished
failure_count=0
warning_count=0
EOF
    for name in stability-interlock device-dependency store-config store-trust \
        foreground-precondition metrics-default-off store-cache-mode; do
        check "$dir" PASS "$name"
    done
    cat >"$dir/app-list.json" <<'EOF'
{"outcome":{"status":"ok","data":{"kind":"applications","apps":[]}}}
EOF
    cat >"$dir/metrics.json" <<'EOF'
{"outcome":{"status":"ok","data":{"kind":"metrics-status","enabled":false,"policy_allowed":true,"configured":false,"pending":false}}}
EOF
    case "$action" in
        refresh-v1 | refresh-v2)
            check "$dir" PASS refresh-accepted
            check "$dir" PASS catalog-visible
            cat >"$dir/catalog.json" <<EOF
{"outcome":{"status":"ok","data":{"kind":"catalog","sequence":$sequence,"stale":false,"apps":[{"app_id":"dev.cardputerzero.store-test","version":"$version","package_bytes":1000}]}}}
EOF
            ;;
        resume-v1)
            check "$dir" PASS install-accepted
            check "$dir" PASS partial-created '100 bytes'
            check "$dir" PASS store-restart 'main PID 10 -> 11'
            check "$dir" PASS partial-survived '200 bytes'
            check "$dir" PASS resume-accepted
            check "$dir" PASS installed-version
            check "$dir" PASS range-resume
            check "$dir" PASS installed-launch
            check "$dir" PASS runtime-observer
            check "$dir" PASS runtime-observer-stopped
            ;;
        upgrade-v2)
            check "$dir" PASS upgrade-accepted
            check "$dir" PASS installed-version
            check "$dir" PASS installed-launch
            check "$dir" PASS runtime-observer
            check "$dir" PASS runtime-observer-stopped
            ;;
        offline-v2)
            check "$dir" PASS offline-cache-before
            check "$dir" PASS offline-refresh
            check "$dir" PASS offline-cache-after
            ;;
        stale-v2)
            check "$dir" PASS stale-catalog
            check "$dir" PASS stale-install-rejected
            ;;
    esac
    printf '%s\n' "$dir"
}

store_refresh1=$(make_store 20260801T050000Z-500 refresh-v1 1.0.0 100 1000300)
store_resume=$(make_store 20260801T050100Z-501 resume-v1 1.0.0 not-applicable 1000310)
store_refresh2=$(make_store 20260801T050200Z-502 refresh-v2 1.1.0 101 1000320)
store_upgrade=$(make_store 20260801T050300Z-503 upgrade-v2 1.1.0 not-applicable 1000330)
store_offline=$(make_store 20260801T050400Z-504 offline-v2 1.1.0 101 1000340)
store_stale=$(make_store 20260801T050500Z-505 stale-v2 1.1.0 101 1000400)
"$verifier" store "$store_refresh1" "$store_resume" "$store_refresh2" \
    "$store_upgrade" "$store_offline" "$store_stale" >/dev/null

printf 'RUNNING\n' >"$factory/status"
expect_fail forged-factory-status "$verifier" factory "$factory"
sed -i.bak 's/core_cpu_millipercent=10000/core_cpu_millipercent=9999/' \
    "$performance/summary.env"
expect_fail forged-performance-cpu "$verifier" performance "$performance"
sed -i.bak 's/22222222-2222-4222-8222-222222222222/11111111-1111-4111-8111-111111111111/' \
    "$cap_persist/summary.env"
expect_fail same-boot-capability "$verifier" capability "$cap_full" "$cap_persist"
sed -i.bak 's/expected_sequence=101/expected_sequence=99/' "$store_refresh2/summary.env"
sed -i.bak 's/"sequence":101/"sequence":99/' "$store_refresh2/catalog.json"
expect_fail Store-sequence-rollback "$verifier" store "$store_refresh1" "$store_resume" \
    "$store_refresh2" "$store_upgrade" "$store_offline" "$store_stale"

echo "PASS independent device acceptance evidence verifier"
