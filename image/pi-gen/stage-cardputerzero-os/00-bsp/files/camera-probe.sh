#!/usr/bin/env bash
set -euo pipefail

if ((EUID != 0)); then
    echo "error: camera-probe must run as root" >&2
    exit 2
fi

sysfs_root=${CP0_CAMERA_PROBE_SYSFS_ROOT:-/sys}
run_root=${CP0_CAMERA_PROBE_RUN_ROOT:-/run/cardputerzero-camera-probe}
modprobe_command=${CP0_CAMERA_PROBE_MODPROBE:-/usr/sbin/modprobe}
journalctl_command=${CP0_CAMERA_PROBE_JOURNALCTL:-/usr/bin/journalctl}
sleep_command=${CP0_CAMERA_PROBE_SLEEP:-/usr/bin/sleep}
boot_config=${CP0_CAMERA_PROBE_BOOT_CONFIG:-/boot/firmware/config.txt}
firmware_file=${CP0_CAMERA_PROBE_FIRMWARE_FILE:-/boot/firmware/start.elf}
bootscreen_firmware_sha256=d1639763fa6714e2cd4544fb45b9d5e5d54e949eaa11d7e7057651b6d4d51efd

powerfail_device="$sysfs_root/bus/platform/devices/powerfail"
powerfail_driver="$sysfs_root/bus/platform/drivers/powerfail-suo"
camera_device="$sysfs_root/bus/i2c/devices/10-0010"
camera_driver="$sysfs_root/bus/i2c/drivers/imx219"

install -d -o root -g root -m 0755 "$run_root"

for module in m5ioe1 powerfail_suo imx219; do
    "$modprobe_command" "$module" >/dev/null 2>&1 || true
done

driver_bound() {
    [[ -L $1/driver ]]
}

bind_device() {
    local driver=$1 device=$2
    [[ -e $driver/bind ]] || return 1
    printf '%s' "$device" >"$driver/bind" 2>/dev/null
}

powerfail_attempts=0
for powerfail_attempts in 1 2 3 4 5; do
    if driver_bound "$powerfail_device"; then
        break
    fi
    if [[ -d $powerfail_device && -d $powerfail_driver ]]; then
        bind_device "$powerfail_driver" powerfail || true
    fi
    "$sleep_command" 0.20
done

# P12 is active-low in the powerfail binding. GPIOD_OUT_LOW therefore leaves
# the physical line high. Give the IMX219 rail time to settle before reprobe.
if driver_bound "$powerfail_device"; then
    "$sleep_command" 0.05
fi

camera_attempts=0
for camera_attempts in 1 2 3 4 5; do
    if driver_bound "$camera_device"; then
        break
    fi
    if [[ -d $camera_device && -d $camera_driver ]]; then
        bind_device "$camera_driver" 10-0010 || true
    fi
    "$sleep_command" 0.25
done

powerfail_state=unbound
camera_state=unbound
driver_bound "$powerfail_device" && powerfail_state=bound
driver_bound "$camera_device" && camera_state=bound

firmware_mode=start
if [[ -r $boot_config ]] && grep -Eq '^[[:space:]]*start_x=1([[:space:]]*(#.*)?)?$' "$boot_config"; then
    firmware_mode=start_x
fi
firmware_variant=raspi-firmware
firmware_sha256=unavailable
if [[ -r $firmware_file ]]; then
    firmware_sha256=$(sha256sum "$firmware_file" | awk '{print $1}')
    if [[ $firmware_sha256 == "$bootscreen_firmware_sha256" ]]; then
        firmware_variant=m5stack_bootscreen
    fi
fi

status_tmp="$run_root/status.env.tmp"
{
    printf 'schema=cardputerzero-camera-probe-v2\n'
    printf 'firmware_mode=%s\n' "$firmware_mode"
    printf 'firmware_variant=%s\n' "$firmware_variant"
    printf 'firmware_sha256=%s\n' "$firmware_sha256"
    printf 'powerfail=%s\n' "$powerfail_state"
    printf 'powerfail_attempts=%u\n' "$powerfail_attempts"
    printf 'imx219=%s\n' "$camera_state"
    printf 'imx219_attempts=%u\n' "$camera_attempts"
    if [[ $camera_state == bound ]]; then
        printf 'result=PASS\n'
    else
        printf 'result=FAIL\n'
    fi
} >"$status_tmp"
chown root:root "$status_tmp"
chmod 0644 "$status_tmp"
mv -f "$status_tmp" "$run_root/status.env"

kernel_tmp="$run_root/kernel-camera.log.tmp"
"$journalctl_command" -k -b --no-pager -o cat 2>/dev/null |
    awk '
        BEGIN { count = 0 }
        tolower($0) ~ /(imx219|m5ioe1|powerfail|unicam)/ {
            gsub(/[[:cntrl:]]/, " ")
            print substr($0, 1, 512)
            count++
            if (count >= 100) exit
        }
    ' >"$kernel_tmp" || true
chown root:root "$kernel_tmp"
chmod 0644 "$kernel_tmp"
mv -f "$kernel_tmp" "$run_root/kernel-camera.log"

if [[ $camera_state == bound ]]; then
    echo "camera-probe: IMX219 bound after $camera_attempts attempt(s)"
else
    echo "camera-probe: IMX219 remains unbound after $camera_attempts attempt(s)" >&2
fi

# Camera absence must not prevent the rest of the OS from reaching Home.
exit 0
