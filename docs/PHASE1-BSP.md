# Phase 1: CM0 V0.6 BSP and Minimal Image

<!-- doc-locale: en -->
> **English** | [简体中文](PHASE1-BSP.zh-CN.md)

## Hardware Baseline (2026-07-30)

The device runs Debian 13.6 and Linux `6.18.34+rpt-rpi-v8`. Firmware reports
512 MB total RAM but assigns 256 MB each to ARM and VideoCore. Linux
`MemTotal` is only 233008 KiB, and the command line contains
`cgroup_disable=memory`. Together, these constraints reduce app capacity and
disable cgroup memory isolation.

The VC4 overlay also requests 256 MB CMA. With too little ARM memory, the
kernel falls back to 8 MB CMA and repeatedly reports camera/DRM allocation
failures. Phase 1 therefore uses:

- `gpu_mem=64` and `gpu_mem_512=64`; the latter overrides the CM0 firmware's
  512 MB-specific default and leaves about 448 MB for ARM;
- `dtoverlay=vc4-kms-v3d,cma-64` for a reasonable DRM/camera CMA reservation;
- `fdtget`/`fdtput` during image build to remove only the embedded
  `cgroup_disable=memory` token from the base CM0 DTB while retaining all other
  firmware boot arguments;
- `cgroup_memory=1 cgroup_enable=memory` to enable the memory controller;
- `apparmor=1 security=apparmor` to enable the AppArmor primary LSM already
  built into the kernel.

The hardware already has a working SPI DRM device:
`/dev/dri/cardputer-zero-internal` points to `card1`, connector `card1-SPI-1`
is connected at 320x170, and the fbdev compatibility layer is RGB565. This
phase pins that driver rather than migrating the display stack again.

## BSP Source

Driver source is pinned to:

```text
repository: https://github.com/m5stack/m5stack-linux-dtoverlays.git
commit:     c3b254819307c177a34100b66fe19e52059ce8c4
profile:    CONFIG_CARDPUTERO_V0_5=y
```

Upstream's V0.5 build switch produces `cardputerzero-v5-overlay`, which is also
the hardware description used by V0.6 devices. The build verifies the exact
commit and rejects a moving HEAD. At that commit, the image applies display
stability parameters from upstream `origin/test`, reducing LCD SPI from 60 MHz
to 20 MHz while retaining the mainline keyboard fixes.

On V0.6, IMX219 power enable is M5IOE1 P12 (GPIO offset 11), not SoC GPIO16 as
on older hardware. `powerfail_suo` in `cardputerzero-v5-overlay` already owns
P12 with active-low semantics and keeps the physical line high in normal
operation. Adding `camera-py12-high-overlay` would make a GPIO hog contend for
the same line. Boot configuration therefore loads only the board overlay and
then `imx219`, and rejects both standalone P12 and GPIO16 camera-power overlays.

The first IMX219 automatic probe can run before M5IOE1 and `powerfail_suo` are
ready. `cardputerzero-camera-probe.service` performs bounded retries for the
`powerfail` binding, waits for P12 to stabilize, then retries `10-0010` at most
five times. Results and filtered camera kernel messages are stored under
`/run/cardputerzero-camera-probe/`. The log retains at most 100 lines related
to Camera, M5IOE1, powerfail, and Unicam; it contains no app data, network
identifier, or generic kernel log. The Owner can read it after enabling SSH.

The V0.6 profile sets `start_x=1` and uses `raspi-firmware`'s
`start_x.elf`/`fixup_x.dat`. The official M5Stack image used an opaque
`m5stack_bootscreen` variant for a pre-kernel splash, but hardware proved it
ignores the 64 MB setting and forces a 256/256 MB split. It is no longer
packaged. Image gates reject its old firmware hash, and probe state records the
actual firmware mode, variant, and hash. Product splash now uses a continuous
handoff across initramfs direct SPI, Linux framebuffer, and a trusted Wayland
surface; see ADR 0008.

V0.6 cold boot also showed that the panel sometimes ignored early fbdev
initialization, while a later compositor disable/enable reliably transmitted
the complete ST7789 soft-reset, sleep-out, and display-on sequence. An early
implementation restarted the compositor before System Shell, clearing the
splash and causing a visible black frame near eight seconds. The current
display-retry oneshot waits only until that stabilization point while panel RAM
retains the framebuffer splash, then starts Weston once. It calculates the
remaining wait from `/proc/uptime` rather than sleeping another fixed eight
seconds from service start. The trusted Weston splash remains until Setup or
Home's first frame. Final cold-boot acceptance still requires an image
containing this change.

## Building the Minimal Image

Docker is the default, supporting macOS and Linux. The daemon must run Linux
arm64 containers. A Linux arm64 host may set `CP0_USE_DOCKER=0` for a native
build.

The GitHub release workflow runs on an x86_64 runner. It installs
`binfmt-support` and `qemu-user-static`, then registers `qemu-aarch64` on the
runner host before starting the privileged pi-gen container. This host-level
registration is required for pi-gen to execute ARM64 stage commands; configuring
the package only inside the container is insufficient.

```sh
export CP0_FIRST_USER_PASSWORD='development-password'
./image/build-image.sh
make verify-image
```

`CP0_SSH_PUBLIC_KEY` may be set for a key-only development image. A production
image must not contain a default password or default-enabled SSH.

The build pins the arm64 `pi-gen` branch at
`ca8aeed0ae300c2a89f55ce9617d5f96a27e99e5`. It runs official `stage0`,
`stage1`, and the CardputerZero custom stage only; `stage2` is excluded. Debian
and Raspberry Pi package sources use HTTPS. `stage0` installs only the `rpi-v8`
kernel required by CM0, excluding Pi 5's 2712 kernel and headers. The custom
stage builds the pinned BSP, produces the `-cp0-os-dev` image, removes
compilers, kernel headers, and desktop-only components, and installs hardware
diagnostics. Proxy settings exist only in the build chroot and must not remain
in the exported system.

After a temporary network or build failure, the container retains `work/` and
the apt cache. Resume after restoring connectivity:

```sh
CP0_FIRST_USER_PASSWORD='development-password' \
CP0_RESUME_BUILD=1 ./image/build-image.sh
```

Set `CP0_KEEP_BUILD_CONTAINER=1` to keep a successful build container for
diagnosis. By default, success removes the container and failure retains it.
`deploy/` receives the compressed image, package inventory, build log, and
`SHA256SUMS`.

## Historical Minimal-Image Candidate (2026-07-30)

This flashable candidate was fully built and accepted through read-only mount
inspection on macOS with Docker Linux/arm64:

- 224 MB compressed and 1.5 GB expanded; rootfs uses 724 MB and bootfs 49 MB;
- only kernel `6.18.34+rpt-rpi-v8` and 16 CardputerZero BSP modules;
- no Launcher, LightDM, Wayfire, PCManFM, PipeWire, PackageKit, GTK input tools,
  compiler, kernel headers, or 2712 kernel;
- a trimmed Weston 14.0.2 DRM/Pixman kiosk baseline, disabled by default, with
  the `tty1` recovery console retained;
- default boot to `multi-user.target` with NetworkManager, SSH, and AppArmor;
- upstream `fb_load.service`, which waited for the vendor Launcher, masked;
  `tty1` framebuffer console shows boot log, hardware summary, IPv4, and login;
- with HDMI attached, V0.6 enumerates HDMI as `fb0` and LCD as `fb1`, so it uses
  `fbcon=map:1`; smoke tests find LCD by `panel-mipi-dbid`, never by number;
- no `cgroup_disable=memory` in CM0 DTB; fixed 64 MB GPU, 64 MB CMA, memory
  cgroup, and AppArmor;
- volatile journald capped at 16 MB and 192 MB zram without writeback;
- `rpi-resize.service` from `raspberrypi-sys-mods` installed and force-enabled
  to expand root to the remaining SD space on first boot; a missing service
  fails the build.

That expansion describes only the 2026-07-30 two-partition baseline. Phase 6A
three-partition product images disable `rpi-resize.service` and the `resize`
kernel argument. Initramfs expands only the final `cp0-data` partition and uses
an immutable root by default. See
[immutable root and persistent data](PHASE6A-IMMUTABLE-ROOT.md).

Hardware acceptance on 2026-07-30 recorded:

- `MemTotal` 424756 KiB; the final Phase 2 candidate used about 151 MiB at idle
  with 192 MiB zram;
- 27.7 seconds to `multi-user.target` on first expansion boot and 18.1 seconds
  on later stable boots;
- LCD, RGB565 framebuffer, TCA8418 keyboard, ES8389 audio, battery, memory
  cgroup, and AppArmor smoke all passed with `failures=0`; an absent camera was
  a non-blocking warning;
- on 2026-08-02, the kernel I2C-1 bus and all six clients passed; the product
  does not expose generic `/dev/i2c-1`, and smoke records disabled raw access as
  a secure state, not a hardware warning;
- after first expansion, one `ext4lazyinit` initialized added inode tables;
  after it exited, the stable 20-second sample wrote only about 16 KiB and
  journald remained volatile;
- on a 32 GB card, partition and ext4 grew from 976 MiB to 28.2 GiB on first
  boot; the service then disabled itself and the second boot retained
  `rw,noatime`.

## Product and Recovery Boot Display

Product images use the fixed early splash and set `quiet loglevel=3 logo.nologo`,
`vt.global_cursor_default=0`, `fbcon=map:off`, and systemd status suppression.
The LCD shows no kernel, initramfs, systemd log, or boot summary, and
`cardputerzero-console-banner.service` is disabled. The static initramfs
`init-top` helper follows the official `ci/early-splash` direct-SPI path but uses
the pinned BSP's DRM-validated `MADCTL=0xa0`, power/gamma values, and display
inversion. A bounded TX/RX FIFO pump writes the pinned user image. After ST7789
fbdev appears, a non-blocking initramfs-root worker redraws the same 320x170
RGB565 frame without waiting for data expansion, OverlayFS switch-root, or
systemd. A final-root oneshot provides bounded retry. Worker and oneshot share
a `/run` completion marker and atomic lock, allowing only one framebuffer write
after DRM takeover. The image remains until compositor and System Shell take
over; boot logs never appear in between.

Recovery images retain `loglevel=6 consoleblank=0 fbcon=map:1`, boot summary,
and `getty@tty1`. When a development product image enters runtime Recovery
Mode, a bounded helper uses `/usr/bin/con2fbmap` to map tty1 to the actual
framebuffer behind `/dev/fb_lcd`, so HDMI does not change the local terminal.

During product boot, the splash proves that Linux reached initramfs and the SPI
LCD and splash resource are usable. Home proves that rootfs, systemd,
compositor, and System Shell have started. A device stuck on splash must be
diagnosed through the router DHCP lease, mDNS, or authorized SSH. After login:

```sh
ip -br -4 address
nmcli device status
```

For a recovery image without preconfigured Wi-Fi, use the device keyboard and
local console:

```sh
sudo nmcli device wifi list
sudo nmcli device wifi connect 'SSID' password 'PASSWORD'
```

The keyboard uses BSP `tca8418_keypad_m5stack.ko` and
`tca8418_m5stack.dtbo`. The driver and LCD module are explicitly included in
initramfs; a dedicated hook also copies panel firmware
`cardputerzero,st7789v_lcd.bin`. Keys reach `tty1` through the Linux input
subsystem, allowing shell commands after login. The V0.6 Fn layer and all key
combinations still require acceptance on the new image.

## Validating Boot Arguments on an Existing Device

Read the matching `bcm2710-rpi-cm0.dtb` from the device, generate a patched
version with `patch-cm0-dtb.sh`, upload it and both scripts, then run the
installer as root:

```sh
./scripts/patch-cm0-dtb.sh bcm2710-rpi-cm0.dtb
sudo ./apply-dev-boot-profile.sh
sudo reboot
./device-smoke.sh
```

The installer saves originals under
`/boot/firmware/cardputerzero-os-backup/<UTC timestamp>` and does not reboot
automatically. On failure, restore `config.txt` and `cmdline.txt` from SD or
SSH. DTB must come from the same device and firmware; the script rejects a file
without the target token.

If the development profile prevents boot, mount bootfs on a workstation and
restore the last known-good backup:

```sh
cp cardputerzero-os-backup/20260730T075924Z/config.txt ./config.txt
cp cardputerzero-os-backup/20260730T075924Z/cmdline.txt ./cmdline.txt
```

This backup retains accepted AppArmor and `cma-64` settings but does not load
the failed bootargs overlay.

## Minimization Policy

Development images retain NetworkManager and SSH. Launcher, LightDM, Wayfire,
PCManFM, PipeWire, PackageKit, Cloud Init, Avahi, RPC/NFS, UDisks,
ModemManager, Raspberry Pi Connect, and automatic apt timers are not in the
base system. Bluetooth services and tools are temporarily omitted and will be
added only with a capability broker.

Hardware on 2026-08-02 confirmed that `brcmfmac` detects SDIO BCM43439, but the
old image omitted firmware and created no `wlan0`. Product images now require
`firmware-brcm80211` from the Raspberry Pi repository, and finished-image gates
verify it. Wi-Fi control must still go through the Shell-only network-settings
broker and must not expose NetworkManager privileges directly.

The device uses 192 MB zram and disables both zram writeback and disk swap to
avoid sustained SD-card writes.
