#!/usr/bin/env bash
set -euo pipefail

include_journal=0
case "${1:-}" in
    "") ;;
    --include-journal) include_journal=1 ;;
    *)
        echo "usage: device-support-bundle [--include-journal]" >&2
        exit 2
        ;;
esac
if (($# > 1)); then
    echo "usage: device-support-bundle [--include-journal]" >&2
    exit 2
fi
if ((EUID != 0)); then
    echo "error: device-support-bundle must run as root" >&2
    exit 2
fi

umask 077
result_root=/run/cardputerzero-support
run_id=$(date -u +%Y%m%dT%H%M%SZ)-$$
run_dir="$result_root/$run_id"
bundle="$result_root/$run_id.tar.gz"
install -d -o root -g root -m 0700 "$run_dir"

units=(
    cardputerzero-overlay-root-status.service
    cardputerzero-compositor.service
    cardputerzero-system-shell.service
    cardputerzero-appd.service
    cardputerzero-stored.service
    NetworkManager.service
    ssh.service
)

single_line() {
    local value=${1:-}
    value=${value//$'\t'/ }
    value=${value//$'\r'/ }
    value=${value//$'\n'/ }
    printf '%s' "${value:0:512}"
}

read_property() {
    local unit=$1 property=$2
    systemctl show "$unit" --property="$property" --value 2>/dev/null || true
}

model=$(tr -d '\000' 2>/dev/null </proc/device-tree/model || true)
os_version=$(sed -n 's/^PRETTY_NAME=//p' /etc/os-release 2>/dev/null | head -1)
os_version=${os_version#\"}
os_version=${os_version%\"}
{
    printf 'schema=cardputerzero-support-v1\n'
    printf 'created_utc=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    printf 'journal_included=%s\n' "$include_journal"
    printf 'model=%s\n' "$(single_line "$model")"
    printf 'os=%s\n' "$(single_line "$os_version")"
    printf 'kernel=%s\n' "$(uname -r)"
    printf 'architecture=%s\n' "$(uname -m)"
    printf 'uptime_seconds=%s\n' "$(cut -d. -f1 /proc/uptime 2>/dev/null || printf unknown)"
} >"$run_dir/manifest.env"

cat >"$run_dir/privacy.txt" <<'PRIVACY'
This bundle is local and is never uploaded automatically.
The default bundle excludes application data and identifiers, installed-app
lists, document contents, Wi-Fi profiles and SSIDs, IP and MAC addresses, SSH
keys, hostname, machine-id, boot-id, hardware serials and raw system logs.
The optional sensitive-journal.txt can contain application IDs, URLs, paths or
user-entered data. Include it only with explicit operator consent and inspect it
before transferring the bundle.
PRIVACY

printf 'unit\tload\tactive\tsub\tresult\tpid\trestarts\tmemory_bytes\texit_status\n' \
    >"$run_dir/services.tsv"
for unit in "${units[@]}"; do
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$unit" \
        "$(single_line "$(read_property "$unit" LoadState)")" \
        "$(single_line "$(read_property "$unit" ActiveState)")" \
        "$(single_line "$(read_property "$unit" SubState)")" \
        "$(single_line "$(read_property "$unit" Result)")" \
        "$(single_line "$(read_property "$unit" MainPID)")" \
        "$(single_line "$(read_property "$unit" NRestarts)")" \
        "$(single_line "$(read_property "$unit" MemoryCurrent)")" \
        "$(single_line "$(read_property "$unit" ExecMainStatus)")" \
        >>"$run_dir/services.tsv"
done

printf 'metric\tvalue\n' >"$run_dir/resources.tsv"
for metric in MemTotal MemAvailable SwapTotal SwapFree Dirty Writeback; do
    value=$(awk -v key="$metric:" '$1 == key { print $2 * 1024 }' \
        /proc/meminfo 2>/dev/null || true)
    printf '%s_bytes\t%s\n' "${metric,,}" "${value:-unknown}" \
        >>"$run_dir/resources.tsv"
done
printf 'target\tfilesystem\toptions\tsize_bytes\tused_bytes\tavailable_bytes\n' \
    >"$run_dir/mounts.tsv"
for target in / /run/cardputerzero-root/volatile /run/cardputerzero-data; do
    if mountpoint -q "$target" 2>/dev/null; then
        filesystem_type=unknown
        options=unknown
        size=unknown
        used=unknown
        available=unknown
        read -r filesystem_type options \
            < <(findmnt -n -o FSTYPE,OPTIONS --target "$target" 2>/dev/null)
        read -r size used available \
            < <(df -B1 --output=size,used,avail "$target" 2>/dev/null | tail -1)
        printf '%s\t%s\t%s\t%s\t%s\t%s\n' \
            "$target" "$filesystem_type" "$options" "$size" "$used" \
            "$available" >>"$run_dir/mounts.tsv"
    fi
done
if [[ -r /sys/block/mmcblk0/stat ]]; then
    read -r _ _ _ _ _ _ sectors_written _ \
        </sys/block/mmcblk0/stat
    printf 'mmc_sectors_written\t%s\n' "$sectors_written" \
        >>"$run_dir/resources.tsv"
fi

printf 'component\tstate\tdetail\n' >"$run_dir/hardware.tsv"
drm_connector=$(find /sys/class/drm -maxdepth 1 -type l \
    -name 'card*-SPI-*' 2>/dev/null | head -1)
if [[ -n $drm_connector ]] &&
    grep -qx '320x170' "$drm_connector/modes" 2>/dev/null; then
    printf 'display\tpresent\t320x170\n' >>"$run_dir/hardware.tsv"
else
    printf 'display\tmissing\texpected-320x170\n' >>"$run_dir/hardware.tsv"
fi
if grep -q 'Name="tca8418c"' /proc/bus/input/devices 2>/dev/null; then
    printf 'keyboard\tpresent\ttca8418c\n' >>"$run_dir/hardware.tsv"
else
    printf 'keyboard\tmissing\ttca8418c\n' >>"$run_dir/hardware.tsv"
fi
if grep -qi 'ES8389-Audio' /proc/asound/cards 2>/dev/null; then
    printf 'audio\tpresent\tES8389-Audio\n' >>"$run_dir/hardware.tsv"
else
    printf 'audio\tmissing\tES8389-Audio\n' >>"$run_dir/hardware.tsv"
fi
for component in battery video0; do
    case "$component" in
        battery)
            candidate=$(find /sys/class/power_supply -maxdepth 1 -type l \
                -name 'bq27220-*' 2>/dev/null | head -1)
            ;;
        video0) candidate=/dev/video0 ;;
    esac
    if [[ -e $candidate ]]; then
        printf '%s\tpresent\tavailable\n' "$component" \
            >>"$run_dir/hardware.tsv"
    else
        printf '%s\tmissing\tunavailable\n' "$component" \
            >>"$run_dir/hardware.tsv"
    fi
done

printf 'network_manager_active\t%s\n' \
    "$(systemctl is-active NetworkManager.service 2>/dev/null || true)" \
    >"$run_dir/network.tsv"
if [[ -d /sys/class/net ]]; then
    network_links=0
    for carrier in /sys/class/net/*/carrier; do
        if [[ -r $carrier ]] && [[ $(<"$carrier") == 1 ]]; then
            network_links=$((network_links + 1))
        fi
    done
    printf 'connected_link_count\t%s\n' "$network_links" \
        >>"$run_dir/network.tsv"
fi

if ((include_journal == 1)); then
    journal_args=()
    for unit in "${units[@]}"; do
        journal_args+=(--unit "$unit")
    done
    journalctl "${journal_args[@]}" --boot --no-pager --output=short-monotonic \
        --lines=600 >"$run_dir/sensitive-journal.txt" 2>&1 || true
fi

tar --sort=name --owner=0 --group=0 --numeric-owner \
    -czf "$bundle" -C "$run_dir" .
chmod 0600 "$bundle"
printf 'PASS support bundle %s\n' "$bundle"
