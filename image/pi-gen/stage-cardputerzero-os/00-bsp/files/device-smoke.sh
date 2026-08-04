#!/usr/bin/env bash
set -uo pipefail

failures=0
warnings=0

pass() { printf 'PASS %-22s %s\n' "$1" "$2"; }
fail() { printf 'FAIL %-22s %s\n' "$1" "$2"; failures=$((failures + 1)); }
warn() { printf 'WARN %-22s %s\n' "$1" "$2"; warnings=$((warnings + 1)); }

model=$(tr -d '\000' </proc/device-tree/model 2>/dev/null || true)
case "$model" in
    *"Compute Module 0"*) pass model "$model" ;;
    *) fail model "unexpected device: ${model:-unknown}" ;;
esac

mem_kb=$(awk '/^MemTotal:/ { print $2 }' /proc/meminfo)
mem_kb=${mem_kb:-0}
if ((mem_kb >= 400000)); then
    pass memory "${mem_kb} KiB"
else
    fail memory "${mem_kb:-0} KiB; expected at least 400000 KiB"
fi

gpu_mem=$(vcgencmd get_mem gpu 2>/dev/null || true)
if [[ "$gpu_mem" == "gpu=64M" ]]; then
    pass gpu-memory "$gpu_mem"
else
    fail gpu-memory "${gpu_mem:-unavailable}; expected gpu=64M"
fi

cmdline=" $(cat /proc/cmdline) "
if [[ "$cmdline" == *" cgroup_disable=memory "* ]]; then
    fail memory-cgroup "kernel command line disables memory controller"
elif [[ -r /sys/fs/cgroup/cgroup.controllers ]] &&
    grep -qw memory /sys/fs/cgroup/cgroup.controllers; then
    pass memory-cgroup "memory controller available"
else
    fail memory-cgroup "memory controller missing"
fi

if [[ "$cmdline" == *" cp0.overlay_root=volatile "* ]]; then
    root_type=$(findmnt -n -o FSTYPE / 2>/dev/null || true)
    lower_options=$(findmnt -n -o OPTIONS \
        /run/cardputerzero-root/lower 2>/dev/null || true)
    upper_type=$(findmnt -n -o FSTYPE \
        /run/cardputerzero-root/volatile 2>/dev/null || true)
    data_type=$(findmnt -n -o FSTYPE \
        /run/cardputerzero-data 2>/dev/null || true)
    data_options=$(findmnt -n -o OPTIONS \
        /run/cardputerzero-data 2>/dev/null || true)
    if [[ "$root_type" == overlay && ",$lower_options," == *,ro,* &&
        "$upper_type" == tmpfs && "$data_type" == ext4 &&
        ",$data_options," == *,rw,* && ",$data_options," == *,nodev,* &&
        ",$data_options," == *,nosuid,* && ",$data_options," == *,noexec,* ]]; then
        pass root-overlay "read-only lower, volatile upper, persistent data"
    else
        fail root-overlay \
            "root=$root_type lower=$lower_options upper=$upper_type"
    fi
else
    pass root-overlay "disabled by kernel command line"
fi

lsms=$(cat /sys/kernel/security/lsm 2>/dev/null || true)
if [[ ",$lsms," == *",apparmor,"* ]]; then
    pass apparmor "$lsms"
else
    fail apparmor "active LSMs: ${lsms:-unknown}"
fi

drm_connector=$(find /sys/class/drm -maxdepth 1 -type l -name 'card*-SPI-*' | head -1)
if [[ -n "$drm_connector" ]] && [[ $(cat "$drm_connector/status" 2>/dev/null) == connected ]] &&
    grep -qx '320x170' "$drm_connector/modes" 2>/dev/null; then
    pass display "$(basename "$drm_connector") connected at 320x170"
else
    fail display "SPI DRM connector or 320x170 mode missing"
fi

lcd_fb=
for candidate in /sys/class/graphics/fb*; do
    if [[ $(cat "$candidate/name" 2>/dev/null || true) == panel-mipi-dbid ]]; then
        lcd_fb=$candidate
        break
    fi
done
fb_name=$(cat "$lcd_fb/name" 2>/dev/null || true)
fb_mode=$(cat "$lcd_fb/virtual_size" 2>/dev/null || true)
fb_bpp=$(cat "$lcd_fb/bits_per_pixel" 2>/dev/null || true)
if [[ "$fb_name" == panel-mipi-dbid && "$fb_mode" == 320,170 && "$fb_bpp" == 16 ]]; then
    pass framebuffer "$(basename "$lcd_fb") $fb_name ${fb_mode} RGB565"
else
    fail framebuffer "device=${lcd_fb:-missing} name=$fb_name mode=$fb_mode bpp=$fb_bpp"
fi

if grep -q 'Name="tca8418c"' /proc/bus/input/devices; then
    pass keyboard "tca8418c input registered"
else
    fail keyboard "tca8418c input missing"
fi

if grep -qi 'ES8389-Audio' /proc/asound/cards 2>/dev/null; then
    pass audio "ES8389-Audio ALSA card registered"
else
    fail audio "ES8389-Audio ALSA card missing"
fi

battery=$(find /sys/class/power_supply -maxdepth 1 -type l -name 'bq27220-*' | head -1)
if [[ -n "$battery" ]] && [[ -r "$battery/capacity" ]]; then
    pass battery "$(cat "$battery/capacity")% $(cat "$battery/status" 2>/dev/null)"
else
    fail battery "bq27220 power supply missing"
fi

i2c_device_count=$(find /sys/bus/i2c/devices -maxdepth 1 -type l -name '1-*' |
    wc -l)
if [[ -d /sys/bus/i2c/devices/i2c-1 ]]; then
    if [[ -e /dev/i2c-1 ]]; then
        i2c_access="raw node present"
    else
        i2c_access="raw access disabled"
    fi
    pass i2c-bus "i2c-1 registered, devices=$i2c_device_count, $i2c_access"
else
    fail i2c-bus "kernel i2c-1 bus missing"
fi

if [[ -e /dev/spidev0.1 ]]; then
    pass device-spidev0.1 /dev/spidev0.1
else
    warn device-spidev0.1 "/dev/spidev0.1 missing"
fi

camera_sensor=$(find /sys/bus/i2c/drivers/imx219 -mindepth 1 -maxdepth 1 \
    -type l -name '*-0010' -print -quit 2>/dev/null || true)
if [[ -n "$camera_sensor" ]]; then
    pass camera-sensor "IMX219 bound at $(basename "$camera_sensor")"
else
    fail camera-sensor "IMX219 is not bound to an I2C device"
fi

if command -v systemd-analyze >/dev/null; then
    boot_time=$(systemd-analyze 2>/dev/null | tr '\n' ' ')
    [[ -n "$boot_time" ]] && pass boot-time "$boot_time"
fi

printf '\nSUMMARY failures=%d warnings=%d\n' "$failures" "$warnings"
((failures == 0))
