#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
verifier="$repo_root/scripts/verify-stability-evidence.sh"
test_parent="$repo_root/target/test-tmp"
mkdir -p "$test_parent"
test_root=$(mktemp -d "$test_parent/cp0-stability-evidence.XXXXXX")
case "$test_root" in
    "$test_parent"/cp0-stability-evidence.*) ;;
    *)
        echo "error: unsafe stability evidence test directory" >&2
        exit 1
        ;;
esac
trap 'rm -rf -- "$test_root"' EXIT

bash -n "$verifier"

write_valid_fixture() {
    fixture=$1
    mkdir -p "$fixture"
    printf 'PASS\n' >"$fixture/status"
    cat >"$fixture/summary.env" <<'SUMMARY'
run_id=20260731T000000Z-123
started_epoch=100
finished_epoch=220
duration_seconds=120
interval_seconds=60
failure_count=0
sd_baseline_sectors_written=1000
sd_final_sectors_written=1020
sd_write_bytes=10240
maximum_sd_write_bytes=67108864
cardputerzero-compositor_baseline_memory=7000000
cardputerzero-compositor_final_memory=7100000
cardputerzero-compositor_restarts=0
cardputerzero-system-shell_baseline_memory=1000000
cardputerzero-system-shell_final_memory=1100000
cardputerzero-system-shell_restarts=0
cardputerzero-appd_baseline_memory=2000000
cardputerzero-appd_final_memory=1900000
cardputerzero-appd_restarts=0
SUMMARY
    cat >"$fixture/samples.tsv" <<'SAMPLES'
epoch	uptime	unit	active	sub	pid	restarts	memory_bytes
100	1000.00	cardputerzero-compositor.service	active	running	10	0	7000000
100	1000.00	cardputerzero-system-shell.service	active	running	11	0	1000000
100	1000.00	cardputerzero-appd.service	active	running	12	0	2000000
160	1060.00	cardputerzero-compositor.service	active	running	10	0	7050000
160	1060.00	cardputerzero-system-shell.service	active	running	11	0	1050000
160	1060.00	cardputerzero-appd.service	active	running	12	0	1950000
220	1120.00	cardputerzero-compositor.service	active	running	10	0	7100000
220	1120.00	cardputerzero-system-shell.service	active	running	11	0	1100000
220	1120.00	cardputerzero-appd.service	active	running	12	0	1900000
SAMPLES
    cat >"$fixture/block-io.tsv" <<'BLOCKS'
epoch	uptime	sectors_written	bytes_written
100	1000.00	1000	512000
160	1060.00	1010	517120
220	1120.00	1020	522240
BLOCKS
    cat >"$fixture/foreground.tsv" <<'FOREGROUND'
epoch	uptime	running_apps
100	1000.00	0
160	1060.00	0
220	1120.00	0
FOREGROUND
}

expect_rejection() {
    fixture=$1
    label=$2
    if "$verifier" "$fixture" >/dev/null 2>&1; then
        echo "error: invalid stability evidence passed: $label" >&2
        exit 1
    fi
}

valid="$test_root/valid"
write_valid_fixture "$valid"
"$verifier" "$valid" >/dev/null

with_stored="$test_root/with-stored"
cp -R "$valid" "$with_stored"
awk -F '\t' 'BEGIN { OFS = "\t" }
    { print }
    NR > 1 && $3 == "cardputerzero-appd.service" {
        print $1, $2, "cardputerzero-stored.service", "active", "running", \
            "13", "0", "3000000"
    }
' "$with_stored/samples.tsv" >"$with_stored/samples.tsv.new"
mv "$with_stored/samples.tsv.new" "$with_stored/samples.tsv"
"$verifier" "$with_stored" >/dev/null

stored_gap="$test_root/stored-gap"
cp -R "$with_stored" "$stored_gap"
awk -F '\t' '$1 != 160 || $3 != "cardputerzero-stored.service"' \
    "$stored_gap/samples.tsv" >"$stored_gap/samples.tsv.new"
mv "$stored_gap/samples.tsv.new" "$stored_gap/samples.tsv"
expect_rejection "$stored_gap" stored-service-gap

running="$test_root/running"
cp -R "$valid" "$running"
printf 'RUNNING\n' >"$running/status"
expect_rejection "$running" running-status

missing_unit="$test_root/missing-unit"
cp -R "$valid" "$missing_unit"
awk 'NR != 6' "$missing_unit/samples.tsv" >"$missing_unit/samples.tsv.new"
mv "$missing_unit/samples.tsv.new" "$missing_unit/samples.tsv"
expect_rejection "$missing_unit" missing-unit-row

pid_change="$test_root/pid-change"
cp -R "$valid" "$pid_change"
awk -F '\t' 'BEGIN { OFS = "\t" } NR == 8 { $6 = 99 } { print }' \
    "$pid_change/samples.tsv" >"$pid_change/samples.tsv.new"
mv "$pid_change/samples.tsv.new" "$pid_change/samples.tsv"
expect_rejection "$pid_change" pid-change

memory_growth="$test_root/memory-growth"
cp -R "$valid" "$memory_growth"
awk -F '\t' 'BEGIN { OFS = "\t" } NR == 8 { $8 = 12000000 } { print }' \
    "$memory_growth/samples.tsv" >"$memory_growth/samples.tsv.new"
mv "$memory_growth/samples.tsv.new" "$memory_growth/samples.tsv"
awk -F = 'BEGIN { OFS = "=" } \
    $1 == "cardputerzero-compositor_final_memory" { $2 = 12000000 } \
    { print }' "$memory_growth/summary.env" >"$memory_growth/summary.env.new"
mv "$memory_growth/summary.env.new" "$memory_growth/summary.env"
expect_rejection "$memory_growth" memory-growth

sd_mismatch="$test_root/sd-mismatch"
cp -R "$valid" "$sd_mismatch"
awk -F = 'BEGIN { OFS = "=" } $1 == "sd_write_bytes" { $2 = 512 } { print }' \
    "$sd_mismatch/summary.env" >"$sd_mismatch/summary.env.new"
mv "$sd_mismatch/summary.env.new" "$sd_mismatch/summary.env"
expect_rejection "$sd_mismatch" sd-summary-mismatch

timeline_mismatch="$test_root/timeline-mismatch"
cp -R "$valid" "$timeline_mismatch"
awk -F '\t' 'BEGIN { OFS = "\t" } NR == 3 { $1 = 161 } { print }' \
    "$timeline_mismatch/block-io.tsv" >"$timeline_mismatch/block-io.tsv.new"
mv "$timeline_mismatch/block-io.tsv.new" "$timeline_mismatch/block-io.tsv"
expect_rejection "$timeline_mismatch" sampling-timeline-mismatch

foreground_running="$test_root/foreground-running"
cp -R "$valid" "$foreground_running"
awk -F '\t' 'BEGIN { OFS = "\t" } NR == 3 { $3 = 1 } { print }' \
    "$foreground_running/foreground.tsv" >"$foreground_running/foreground.tsv.new"
mv "$foreground_running/foreground.tsv.new" "$foreground_running/foreground.tsv"
expect_rejection "$foreground_running" foreground-running-app

foreground_missing_sample="$test_root/foreground-missing-sample"
cp -R "$valid" "$foreground_missing_sample"
awk 'NR != 3' "$foreground_missing_sample/foreground.tsv" \
    >"$foreground_missing_sample/foreground.tsv.new"
mv "$foreground_missing_sample/foreground.tsv.new" \
    "$foreground_missing_sample/foreground.tsv"
expect_rejection "$foreground_missing_sample" foreground-missing-sample

foreground_bad_time="$test_root/foreground-bad-time"
cp -R "$valid" "$foreground_bad_time"
awk -F '\t' 'BEGIN { OFS = "\t" } NR == 3 { $1 = 161 } { print }' \
    "$foreground_bad_time/foreground.tsv" >"$foreground_bad_time/foreground.tsv.new"
mv "$foreground_bad_time/foreground.tsv.new" \
    "$foreground_bad_time/foreground.tsv"
expect_rejection "$foreground_bad_time" foreground-timeline-mismatch

foreground_malformed="$test_root/foreground-malformed"
cp -R "$valid" "$foreground_malformed"
awk -F '\t' 'BEGIN { OFS = "\t" } NR == 3 { $3 = -1 } { print }' \
    "$foreground_malformed/foreground.tsv" >"$foreground_malformed/foreground.tsv.new"
mv "$foreground_malformed/foreground.tsv.new" \
    "$foreground_malformed/foreground.tsv"
expect_rejection "$foreground_malformed" foreground-malformed-count

linked_foreground="$test_root/linked-foreground"
cp -R "$valid" "$linked_foreground"
rm "$linked_foreground/foreground.tsv"
ln -s "$valid/foreground.tsv" "$linked_foreground/foreground.tsv"
expect_rejection "$linked_foreground" linked-foreground-samples

duplicate="$test_root/duplicate"
cp -R "$valid" "$duplicate"
printf 'failure_count=0\n' >>"$duplicate/summary.env"
expect_rejection "$duplicate" duplicate-summary-key

linked_failures="$test_root/linked-failures"
cp -R "$valid" "$linked_failures"
ln -s /dev/null "$linked_failures/failures.log"
expect_rejection "$linked_failures" linked-failure-log

echo "PASS stability evidence verifier"
