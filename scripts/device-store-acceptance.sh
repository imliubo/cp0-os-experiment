#!/usr/bin/env bash
set -euo pipefail

if (($# < 1 || $# > 2)); then
    echo "usage: device-store-acceptance ACTION [EXPECTED_SEQUENCE]" >&2
    exit 2
fi
action=$1
expected_sequence=${2:-}
case "$action" in
    refresh-v1 | refresh-v2 | offline-v2 | stale-v2)
        if [[ ! $expected_sequence =~ ^[1-9][0-9]*$ ]]; then
            echo "error: $action requires the expected catalog sequence" >&2
            exit 2
        fi
        ;;
    resume-v1 | upgrade-v2)
        if [[ -n $expected_sequence ]]; then
            echo "error: $action does not accept a catalog sequence" >&2
            exit 2
        fi
        ;;
    *)
        echo "error: unknown Store acceptance action" >&2
        exit 2
        ;;
esac
if ((EUID != 0)); then
    echo "error: device-store-acceptance must run as root" >&2
    exit 2
fi

umask 077
app_id=dev.cardputerzero.store-test
case "$action" in
    *v1) expected_version=1.0.0 ;;
    *v2) expected_version=1.1.0 ;;
esac
result_root=/run/cardputerzero-store-acceptance
run_id=$(date -u +%Y%m%dT%H%M%SZ)-$$
run_dir="$result_root/$run_id"
checks="$run_dir/checks.tsv"
status_file="$run_dir/status"
failures=0
warnings=0
active_app=0
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
    if ((active_app == 1)); then
        /usr/bin/cp0ctl app stop "$app_id" >/dev/null 2>&1 || :
        active_app=0
    fi
}
trap stop_active EXIT

finish() {
    local finished_epoch
    finished_epoch=$(date +%s)
    {
        printf 'schema=cardputerzero-store-acceptance-v1\n'
        printf 'run_id=%s\n' "$run_id"
        printf 'action=%s\n' "$action"
        printf 'expected_version=%s\n' "$expected_version"
        printf 'expected_sequence=%s\n' "${expected_sequence:-not-applicable}"
        printf 'finished_epoch=%s\n' "$finished_epoch"
        printf 'failure_count=%s\n' "$failures"
        printf 'warning_count=%s\n' "$warnings"
    } >"$run_dir/summary.env"
    if ((failures == 0)); then
        printf 'PASS\n' >"$status_file"
        printf 'PASS Store acceptance %s warnings=%s\n' "$run_dir" "$warnings"
        exit 0
    fi
    printf 'FAILED failures=%s\n' "$failures" >"$status_file"
    printf 'FAILED Store acceptance %s failures=%s warnings=%s\n' \
        "$run_dir" "$failures" "$warnings" >&2
    exit 1
}

if systemctl is-active --quiet cardputerzero-stability-acceptance.service; then
    record FAIL stability-interlock \
        "24-hour stability acceptance is active; Store testing would invalidate it"
    finish
fi
record PASS stability-interlock inactive
if ! command -v jq >/dev/null 2>&1; then
    record FAIL device-dependency "jq is required to validate structured service responses"
    finish
fi
record PASS device-dependency jq

config=/etc/cardputerzero/store.conf
catalog_url=$(sed -n 's/^catalog_url=//p' "$config" 2>/dev/null || true)
config_mode=$(stat -c '%a:%U:%G' "$config" 2>/dev/null || true)
if [[ $catalog_url == https://* && $config_mode == 644:root:root ]]; then
    record PASS store-config "$catalog_url"
else
    record FAIL store-config \
        "catalog URL must be provisioned HTTPS and config must be 644:root:root"
fi
trust_count=0
for key in /etc/cardputerzero/trust/store/*.pub; do
    [[ -e $key ]] || continue
    actual=$(stat -c '%s:%a:%U:%G' "$key" 2>/dev/null || true)
    if [[ $actual != 32:644:root:root ]]; then
        record FAIL "store-key:$key" "${actual:-unreadable}"
    else
        trust_count=$((trust_count + 1))
    fi
done
if ((trust_count == 1)); then
    record PASS store-trust "one 32-byte root-owned catalog key"
else
    record FAIL store-trust "expected exactly one test Store key, found $trust_count"
fi

app_list=$(/usr/bin/cp0ctl app list 2>"$run_dir/app-list.err") || {
    record FAIL foreground-precondition "cannot query application list"
    finish
}
printf '%s\n' "$app_list" >"$run_dir/app-list.json"
if ! jq -e '
    .outcome.status == "ok" and
    .outcome.data.kind == "applications" and
    (.outcome.data.apps | type == "array")
' "$run_dir/app-list.json" >/dev/null; then
    record FAIL foreground-precondition "application list response is malformed"
elif jq -e '.outcome.data.apps | any(.running == true)' \
    "$run_dir/app-list.json" >/dev/null; then
    record FAIL foreground-precondition "stop the foreground application before Store acceptance"
else
    record PASS foreground-precondition "no application is running"
fi
if ((failures != 0)); then
    finish
fi

store_list() {
    /usr/bin/cp0ctl store list 2>/dev/null
}

catalog_matches() {
    local encoded=$1 sequence=$2 version=$3 stale=$4
    jq -e \
        --argjson sequence "$sequence" \
        --arg version "$version" \
        --arg app_id "$app_id" \
        --argjson stale "$stale" '
        .outcome.status == "ok" and
        .outcome.data.kind == "catalog" and
        .outcome.data.sequence == $sequence and
        .outcome.data.stale == $stale and
        (.outcome.data.apps | length) == 1 and
        .outcome.data.apps[0].app_id == $app_id and
        .outcome.data.apps[0].version == $version
    ' <<<"$encoded" >/dev/null 2>&1
}

catalog_version_matches() {
    local encoded=$1 version=$2 stale=$3
    jq -e \
        --arg version "$version" \
        --arg app_id "$app_id" \
        --argjson stale "$stale" '
        .outcome.status == "ok" and
        .outcome.data.kind == "catalog" and
        .outcome.data.stale == $stale and
        (.outcome.data.apps | length) == 1 and
        .outcome.data.apps[0].app_id == $app_id and
        .outcome.data.apps[0].version == $version
    ' <<<"$encoded" >/dev/null 2>&1
}

wait_catalog() {
    local second encoded
    for second in $(seq 1 60); do
        encoded=$(store_list || true)
        if catalog_matches "$encoded" "$expected_sequence" \
            "$expected_version" false; then
            printf '%s\n' "$encoded" >"$run_dir/catalog.json"
            record PASS catalog-visible \
                "sequence=$expected_sequence version=$expected_version stale=false"
            return 0
        fi
        sleep 1
    done
    record FAIL catalog-visible "catalog did not converge within 60 seconds"
    return 1
}

installed_version() {
    /usr/bin/cp0ctl app list 2>/dev/null | jq -er \
        --arg app_id "$app_id" \
        '.outcome.data.apps[]? | select(.app_id == $app_id) | .version' | head -1
}

wait_installed() {
    local second version
    for second in $(seq 1 240); do
        version=$(installed_version || true)
        if [[ $version == "$expected_version" ]]; then
            record PASS installed-version "$app_id $version"
            return 0
        fi
        sleep 1
    done
    record FAIL installed-version \
        "$app_id did not reach $expected_version within 240 seconds"
    return 1
}

launch_installed() {
    if ! /usr/bin/cp0ctl app start "$app_id" >"$run_dir/start.json" 2>&1; then
        record FAIL installed-launch "app start failed"
        return
    fi
    active_app=1
    sleep 2
    if /usr/bin/cp0ctl app stop "$app_id" >"$run_dir/stop.json" 2>&1; then
        active_app=0
        record PASS installed-launch "$app_id started and stopped"
    else
        record FAIL installed-launch "app stop failed"
    fi
}

case "$action" in
    refresh-v1 | refresh-v2)
        if /usr/bin/cp0ctl store refresh >"$run_dir/refresh.json" 2>&1; then
            record PASS refresh-accepted "$catalog_url"
            wait_catalog || true
        else
            record FAIL refresh-accepted "cp0ctl store refresh failed"
        fi
        ;;
    resume-v1)
        partial_root=/var/lib/cardputerzero/store/packages
        encoded=$(store_list || true)
        if ! catalog_version_matches "$encoded" "$expected_version" false; then
            record FAIL resume-precondition \
                "a fresh, unexpired v1 catalog must be cached before resume-v1"
            finish
        fi
        package_bytes=$(jq -er \
            --arg app_id "$app_id" \
            '.outcome.data.apps[] | select(.app_id == $app_id) | .package_bytes' \
            <<<"$encoded" 2>/dev/null || true)
        if [[ ! $package_bytes =~ ^[1-9][0-9]*$ ]]; then
            record FAIL resume-precondition "catalog package size is missing or invalid"
            finish
        fi
        current=$(installed_version || true)
        if [[ -n $current ]]; then
            record FAIL resume-precondition \
                "test application is already installed at version $current"
            finish
        fi
        if find "$partial_root" -maxdepth 1 -type f -name '*.part' -print -quit |
            grep -q .; then
            record FAIL resume-precondition \
                "remove prior test partials using a freshly provisioned test data partition"
            finish
        fi
        if ! /usr/bin/cp0ctl store install "$app_id" >"$run_dir/install-first.json" 2>&1; then
            record FAIL install-accepted "initial Store install request failed"
            finish
        fi
        record PASS install-accepted "$app_id $expected_version"
        partial=
        partial_size=0
        for attempt in $(seq 1 200); do
            partial=$(find "$partial_root" -maxdepth 1 -type f -name '*.part' \
                -print -quit 2>/dev/null || true)
            partial_size=$(stat -c %s "$partial" 2>/dev/null || true)
            if [[ $partial_size =~ ^[1-9][0-9]*$ ]] &&
                ((partial_size < package_bytes)); then
                break
            fi
            sleep 0.05
        done
        if [[ ! $partial_size =~ ^[1-9][0-9]*$ ]] ||
            ((partial_size >= package_bytes)); then
            record FAIL partial-created \
                "download completed too quickly or no partial appeared; throttle the test origin"
            finish
        fi
        record PASS partial-created "$partial_size bytes"
        journal_epoch=$(date +%s)
        previous_pid=$(systemctl show cardputerzero-stored.service \
            --property=MainPID --value)
        if ! systemctl kill --kill-whom=main --signal=KILL \
            cardputerzero-stored.service; then
            record FAIL store-restart "targeted cp0-stored kill failed"
            finish
        fi
        current_pid=
        for attempt in $(seq 1 50); do
            current_pid=$(systemctl show cardputerzero-stored.service \
                --property=MainPID --value)
            if systemctl is-active --quiet cardputerzero-stored.service &&
                [[ $current_pid =~ ^[1-9][0-9]*$ ]] &&
                [[ $current_pid != "$previous_pid" ]]; then
                break
            fi
            sleep 0.2
        done
        if ! systemctl is-active --quiet cardputerzero-stored.service ||
            [[ ! $current_pid =~ ^[1-9][0-9]*$ ]] ||
            [[ $current_pid == "$previous_pid" ]]; then
            record FAIL store-restart "cp0-stored did not restart with a new main PID"
            finish
        fi
        record PASS store-restart "main PID $previous_pid -> $current_pid"
        resumed_size=$(stat -c %s "$partial" 2>/dev/null || true)
        if [[ $resumed_size =~ ^[1-9][0-9]*$ ]] &&
            ((resumed_size < package_bytes)); then
            record PASS partial-survived "$resumed_size bytes"
        else
            record FAIL partial-survived \
                "partial is missing or completed before the restart proof"
            finish
        fi
        if ! /usr/bin/cp0ctl store install "$app_id" >"$run_dir/install-resume.json" 2>&1; then
            record FAIL resume-accepted "second Store install request failed"
            finish
        fi
        if wait_installed; then
            if journalctl -u cardputerzero-stored.service --since "@$journal_epoch" \
                --no-pager 2>/dev/null |
                grep -q 'resuming package download from byte'; then
                record PASS range-resume "cp0-stored accepted a matching HTTP range response"
            else
                record FAIL range-resume "no validated 206 resume evidence in the Store journal"
            fi
            launch_installed
        fi
        ;;
    upgrade-v2)
        encoded=$(store_list || true)
        if ! catalog_version_matches "$encoded" "$expected_version" false; then
            record FAIL upgrade-precondition \
                "a fresh, unexpired v2 catalog must be cached before upgrade-v2"
            finish
        fi
        current=$(installed_version || true)
        if [[ $current != 1.0.0 ]]; then
            record FAIL upgrade-precondition "installed version is ${current:-missing}, expected 1.0.0"
            finish
        fi
        if /usr/bin/cp0ctl store install "$app_id" >"$run_dir/upgrade.json" 2>&1; then
            record PASS upgrade-accepted "$app_id $expected_version"
            if wait_installed; then
                launch_installed
            fi
        else
            record FAIL upgrade-accepted "Store upgrade request failed"
        fi
        ;;
    offline-v2)
        encoded=$(store_list || true)
        if catalog_matches "$encoded" "$expected_sequence" \
            "$expected_version" false; then
            record PASS offline-cache-before "verified cached catalog is available"
        else
            record FAIL offline-cache-before "expected v2 catalog is not cached"
            finish
        fi
        journal_epoch=$(date +%s)
        if /usr/bin/cp0ctl store refresh >"$run_dir/offline-refresh.json" 2>&1; then
            :
        else
            record FAIL offline-refresh "refresh request was not accepted"
            finish
        fi
        offline_failure=0
        for attempt in $(seq 1 60); do
            if journalctl -u cardputerzero-stored.service --since "@$journal_epoch" \
                --no-pager 2>/dev/null | grep -q 'catalog refresh failed:'; then
                offline_failure=1
                break
            fi
            sleep 1
        done
        if ((offline_failure == 1)); then
            record PASS offline-refresh "origin failure reached cp0-stored"
        else
            record FAIL offline-refresh \
                "no failure was observed; take the public test origin offline before this action"
        fi
        encoded=$(store_list || true)
        if catalog_matches "$encoded" "$expected_sequence" \
            "$expected_version" false; then
            record PASS offline-cache-after "verified cached catalog remained browsable"
        else
            record FAIL offline-cache-after "cached catalog was lost after refresh failure"
        fi
        ;;
    stale-v2)
        encoded=$(store_list || true)
        if catalog_matches "$encoded" "$expected_sequence" \
            "$expected_version" true; then
            record PASS stale-catalog "expired cached v2 catalog is visible as stale"
        else
            record FAIL stale-catalog \
                "wait until the signed catalog expiry before running stale-v2"
            finish
        fi
        if /usr/bin/cp0ctl store install "$app_id" \
            >"$run_dir/stale-install.out" 2>"$run_dir/stale-install.err"; then
            record FAIL stale-install-rejected "expired catalog unexpectedly authorized install"
        elif grep -q 'Untrusted' "$run_dir/stale-install.err"; then
            record PASS stale-install-rejected "expired catalog cannot authorize installation"
        else
            record FAIL stale-install-rejected "install failed for an unexpected reason"
        fi
        ;;
esac

cache_mode=$(stat -c '%a:%U:%G' /var/lib/cardputerzero/store 2>/dev/null || true)
if [[ $cache_mode == 700:cp0-store:cp0-store ]]; then
    record PASS store-cache-mode "$cache_mode"
else
    record FAIL store-cache-mode "${cache_mode:-missing}"
fi
finish
