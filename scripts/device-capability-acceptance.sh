#!/usr/bin/env bash
set -euo pipefail

mode=full
case "${1:-}" in
    "") ;;
    --full) mode=full ;;
    --persistence-only) mode=persistence ;;
    *)
        echo "usage: device-capability-acceptance [--full|--persistence-only]" >&2
        exit 2
        ;;
esac
if (($# > 1)); then
    echo "usage: device-capability-acceptance [--full|--persistence-only]" >&2
    exit 2
fi
if ((EUID != 0)); then
    echo "error: device-capability-acceptance must run as root" >&2
    exit 2
fi

umask 077
primary_app=dev.cardputerzero.acceptance
isolation_app=dev.cardputerzero.isolation
result_root=/run/cardputerzero-capability
run_id=$(date -u +%Y%m%dT%H%M%SZ)-$$
run_dir="$result_root/$run_id"
checks="$run_dir/checks.tsv"
status_file="$run_dir/status"
failures=0
warnings=0
active_app=
final_body=
install -d -o root -g root -m 0700 "$run_dir"
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

stop_active() {
    if [[ -n $active_app ]]; then
        if ! /usr/bin/cp0ctl app stop "$active_app" \
            >"$run_dir/stop-${active_app##*.}.json" 2>&1; then
            record FAIL "app-stop:$active_app" "cp0ctl app stop failed"
        fi
        active_app=
    fi
}
trap stop_active EXIT

json_string() {
    local field=$1
    sed -n 's/^[[:space:]]*"'"$field"'": "\([^"]*\)"[,]*$/\1/p' | head -1
}

json_number() {
    local field=$1
    sed -n 's/^[[:space:]]*"'"$field"'": \([0-9][0-9]*\)[,]*$/\1/p' | head -1
}

finish() {
    local boot_id finished_epoch
    stop_active
    boot_id=$(cat /proc/sys/kernel/random/boot_id 2>/dev/null || true)
    finished_epoch=$(date +%s)
    {
        printf 'schema=cardputerzero-capability-v1\n'
        printf 'run_id=%s\n' "$run_id"
        printf 'mode=%s\n' "$mode"
        printf 'boot_id=%s\n' "${boot_id:-unknown}"
        printf 'finished_epoch=%s\n' "$finished_epoch"
        printf 'failure_count=%s\n' "$failures"
        printf 'warning_count=%s\n' "$warnings"
    } >"$run_dir/summary.env"
    if ((failures == 0)); then
        printf 'PASS\n' >"$status_file"
        printf 'PASS capability acceptance %s warnings=%s\n' "$run_dir" "$warnings"
        exit 0
    fi
    printf 'FAILED failures=%s\n' "$failures" >"$status_file"
    printf 'FAILED capability acceptance %s failures=%s warnings=%s\n' \
        "$run_dir" "$failures" "$warnings" >&2
    exit 1
}

if systemctl is-active --quiet cardputerzero-stability-acceptance.service; then
    record FAIL stability-interlock \
        "24-hour stability acceptance is active; capability testing would invalidate it"
    finish
fi
record PASS stability-interlock inactive

for app_id in "$primary_app" "$isolation_app"; do
    manifest="/var/lib/cardputerzero/apps/$app_id/0.1.0/app.json"
    if [[ -f $manifest && ! -L $manifest ]]; then
        record PASS "installed:$app_id" "$manifest"
    elif [[ $mode == persistence && $app_id == "$isolation_app" ]]; then
        :
    else
        record FAIL "installed:$app_id" "acceptance application is not installed"
    fi
done
if ((failures != 0)); then
    finish
fi

app_list=$(/usr/bin/cp0ctl app list 2>"$run_dir/app-list.err") || {
    record FAIL foreground-precondition "cannot query application list"
    finish
}
printf '%s\n' "$app_list" >"$run_dir/app-list.json"
if grep -q '"running": true' "$run_dir/app-list.json"; then
    record FAIL foreground-precondition "stop the foreground application before acceptance"
    finish
fi
record PASS foreground-precondition "no application is running"

reset_permission() {
    local app_id=$1 permission=$2 file_permission
    file_permission=${permission//./-}
    if /usr/bin/cp0ctl permission reset "$app_id" "$permission" \
        >"$run_dir/reset-${app_id##*.}-$file_permission.json" 2>&1; then
        record PASS "permission-reset:$app_id:$permission" reset
    else
        record FAIL "permission-reset:$app_id:$permission" "cp0ctl reset failed"
    fi
}

permission_choice() {
    local policy=$1 permission=$2
    if [[ $permission == notifications.post || $policy == allow ]]; then
        printf 'always\n'
    else
        printf 'deny\n'
    fi
}

drive_probe() {
    local app_id=$1 policy=$2 allow_prompts=${3:-1}
    local attempt second prompt prompt_id prompt_app permission choice result_file
    local previous_inode current_inode body
    final_body=
    case "$app_id" in
        "$primary_app")
            result_file="/var/lib/cardputerzero/data/$app_id/acceptance.result"
            ;;
        "$isolation_app")
            result_file="/var/lib/cardputerzero/data/$app_id/isolation.result"
            ;;
        *)
            record FAIL "result-path:$app_id" "unknown acceptance application"
            return 1
            ;;
    esac
    for attempt in 1 2 3 4 5 6 7 8; do
        previous_inode=$(stat -c %i "$result_file" 2>/dev/null || true)
        if ! /usr/bin/cp0ctl app start "$app_id" \
            >"$run_dir/start-${app_id##*.}-$policy-$attempt.json" 2>&1; then
            record FAIL "app-start:$app_id:$policy" "cp0ctl app start failed"
            return 1
        fi
        active_app=$app_id
        for second in $(seq 1 90); do
            prompt=$(/usr/bin/cp0ctl permission pending 2>/dev/null || true)
            prompt_id=$(printf '%s\n' "$prompt" | json_number prompt_id)
            if [[ -n $prompt_id ]]; then
                prompt_app=$(printf '%s\n' "$prompt" | json_string app_id)
                permission=$(printf '%s\n' "$prompt" | json_string permission)
                if [[ $allow_prompts != 1 ]]; then
                    record FAIL "permission-persistence:$app_id" \
                        "unexpected prompt for $permission after reboot"
                    return 1
                fi
                if [[ $prompt_app != "$app_id" || -z $permission ]]; then
                    record FAIL "permission-prompt:$app_id" "unexpected pending prompt"
                    return 1
                fi
                choice=$(permission_choice "$policy" "$permission")
                if ! /usr/bin/cp0ctl permission resolve "$prompt_id" "$choice" \
                    >"$run_dir/resolve-$prompt_id.json" 2>&1; then
                    record FAIL "permission-resolve:$app_id:$permission" \
                        "cp0ctl resolve failed"
                    return 1
                fi
                record PASS "permission-resolve:$app_id:$permission" "$choice"
                stop_active
                break
            fi

            current_inode=$(stat -c %i "$result_file" 2>/dev/null || true)
            if [[ -n $current_inode && $current_inode != "$previous_inode" ]]; then
                body=$(cat "$result_file" 2>/dev/null || true)
                printf '%s\n' "$body" \
                    >"$run_dir/result-${app_id##*.}-$policy.txt"
                if [[ -z $body || ${#body} -gt 160 ||
                    $body == *[!a-z0-9=\;.-]* ]]; then
                    record FAIL "result:$app_id:$policy" \
                        "invalid private-storage result"
                    return 1
                fi
                final_body=$body
                stop_active
                return 0
            fi
            sleep 1
        done
        if [[ -n $active_app ]]; then
            record FAIL "probe-timeout:$app_id:$policy" \
                "no permission prompt or fresh private-storage result within 90 seconds"
            return 1
        fi
    done
    record FAIL "probe-attempts:$app_id:$policy" "permission sequence did not converge"
    return 1
}

check_gpio_sysfs() {
    local path actual
    for path in \
        /sys/class/leds/grove_fun/brightness \
        /sys/class/leds/ext_usb_gpio_fun/brightness \
        /sys/class/leds/grove_5v_out/brightness \
        /sys/class/leds/ext_5v_out/brightness; do
        actual=$(stat -c '%a:%U:%G' "$path" 2>/dev/null || true)
        if [[ $actual == 660:root:cp0-gpio ]]; then
            record PASS "gpio-sysfs:$path" "$actual"
        else
            record FAIL "gpio-sysfs:$path" "${actual:-missing}; expected 660:root:cp0-gpio"
        fi
        if runuser -u pi -- test -r "$path" 2>/dev/null ||
            runuser -u pi -- test -w "$path" 2>/dev/null; then
            record FAIL "gpio-bypass:$path" "pi can bypass cp0-gpiod"
        else
            record PASS "gpio-bypass:$path" denied
        fi
    done
}

check_storage_modes() {
    local path actual invalid entry
    actual=$(stat -c '%a:%U:%G' /var/lib/cardputerzero/data 2>/dev/null || true)
    if [[ $actual == 700:cp0-storage:cp0-storage ]]; then
        record PASS storage-root-mode "$actual"
    else
        record FAIL storage-root-mode "${actual:-missing}"
    fi
    for path in \
        "/var/lib/cardputerzero/data/$primary_app" \
        "/var/lib/cardputerzero/data/$isolation_app"; do
        if [[ ! -d $path && $path == *"/$isolation_app" ]]; then
            record PASS "storage-directory:$path" \
                "absent because an isolated missing-key read creates no host directory"
            continue
        elif [[ ! -d $path ]]; then
            record FAIL "storage-directory:$path" missing
            continue
        fi
        actual=$(stat -c '%a:%U:%G' "$path" 2>/dev/null || true)
        if [[ $actual == 700:cp0-storage:cp0-storage ]]; then
            record PASS "storage-directory:$path" "$actual"
        else
            record FAIL "storage-directory:$path" "$actual"
        fi
        invalid=$(find "$path" -mindepth 1 -maxdepth 1 ! -type f \
            -print -quit 2>/dev/null || true)
        if [[ -z $invalid ]]; then
            while IFS= read -r -d '' entry; do
                actual=$(stat -c '%a:%U:%G' "$entry" 2>/dev/null || true)
                if [[ $actual != 600:cp0-storage:cp0-storage ]]; then
                    invalid="$entry ($actual)"
                    break
                fi
            done < <(find "$path" -mindepth 1 -maxdepth 1 -type f -print0)
        fi
        if [[ -z $invalid ]]; then
            record PASS "storage-entries:$path" "regular files with restricted modes"
        else
            record FAIL "storage-entries:$path" "invalid entry $invalid"
        fi
    done
}

if [[ $mode == persistence ]]; then
    if drive_probe "$primary_app" persistence 0; then
        if [[ $final_body == *';storage=persist-ok' ]]; then
            record PASS storage-reboot-persistence "$final_body"
        else
            record FAIL storage-reboot-persistence "$final_body"
        fi
    fi
    check_gpio_sysfs
    check_storage_modes
    finish
fi

for permission in audio.playback audio.capture hardware.gpio notifications.post; do
    reset_permission "$primary_app" "$permission"
done
if ((failures == 0)) && drive_probe "$primary_app" deny; then
    if [[ $final_body == \
        'audio-play=denied;audio-capture=denied;gpio=denied;storage=persist-ok' ]]; then
        record PASS capability-denial "$final_body"
    else
        record FAIL capability-denial "$final_body"
    fi
fi

for permission in audio.playback audio.capture hardware.gpio notifications.post; do
    reset_permission "$primary_app" "$permission"
done
if ((failures == 0)) && drive_probe "$primary_app" allow; then
    if [[ $final_body == *'audio-play=ok;'* ]]; then
        record PASS audio-playback-broker "$final_body"
    else
        record FAIL audio-playback-broker "$final_body"
    fi
    if [[ $final_body == *'audio-capture=ok-signal;'* ]]; then
        record PASS audio-capture-broker signal
    elif [[ $final_body == *'audio-capture=ok-silent;'* ]]; then
        record WARN audio-capture-signal \
            "capture completed but all samples were zero; verify the microphone path"
    else
        record FAIL audio-capture-broker "$final_body"
    fi
    if [[ $final_body == *';gpio=ok;'* ]]; then
        record PASS gpio-read-write-restore "$final_body"
    else
        record FAIL gpio-read-write-restore "$final_body"
    fi
    if [[ $final_body == *';storage=persist-ok' ]]; then
        record PASS storage-quota-and-restart "$final_body"
    else
        record FAIL storage-quota-and-restart "$final_body"
    fi
fi
if [[ ${CP0_AUDIO_OBSERVED:-no} == yes ]]; then
    record PASS audio-observed "operator confirmed the acceptance tone"
else
    record WARN audio-observed \
        "set CP0_AUDIO_OBSERVED=yes only after an operator hears the acceptance tone"
fi

reset_permission "$isolation_app" notifications.post
if ((failures == 0)) && drive_probe "$isolation_app" allow; then
    if [[ $final_body == storage-isolation=ok ]]; then
        record PASS storage-cross-app-isolation "$final_body"
    else
        record FAIL storage-cross-app-isolation "$final_body"
    fi
fi

check_gpio_sysfs
check_storage_modes
finish
