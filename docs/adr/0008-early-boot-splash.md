# ADR 0008: V0.6 Early-Boot Splash and Silent Boot

<!-- doc-locale: en -->
> **English** | [简体中文](0008-early-boot-splash.zh-CN.md)

- Status: Accepted
- Date: 2026-08-06

## Decision

The CM0 V0.6 product image sets `start_x=1` and uses the
`start_x.elf`/`fixup_x.dat` pair from `raspi-firmware`. During initramfs
`init-top`, the static, bounded `early-splash-spi` helper runs first. It maps the
BCM2837 peripheral window at `0x3f000000`, initializes ST7789 through SPI0/GPIO
registers at approximately 20 MHz, and writes the fixed 108800-byte
`splash.rgb565` into panel RAM.

This path does not wait for DRM, udev, the root partition, OverlayFS, or
systemd. It then starts a non-blocking framebuffer worker that redraws the same
image after the Linux driver takes ownership and may reset panel RAM. In the
final root, `cardputerzero-early-splash.service` remains as a bounded fallback
before the compositor. A successful framebuffer worker writes a boot-scoped
marker under `/run/cardputerzero-early-splash/`; an atomic directory lock stops
the worker and systemd fallback from writing concurrently, so the image is
redrawn at most once after DRM takeover. None of these three layers exposes the
framebuffer to apps. Failure does not block Home. The recovery initramfs
contains no helper, worker, or splash image.

The peripheral base, SPI0 CS0, GPIO25 DC, and 35-line offset come from commit
`e05b81c80f1f5a8e589956937adba5b5d04f0ca9` on the official CardputerZero
`pi-gen` `ci/early-splash` branch. That experimental prototype filled solid
blue only. Its `MADCTL=0x60` and minimal initialization sequence were never
validated for image orientation, gamma, or display inversion. Hardware
revalidation on 2026-08-07 confirmed that it inverted the product image and
produced incorrect colors. The direct-SPI helper now matches the controller
configuration in `cardputerzero,st7789v_lcd.bin` from pinned BSP commit
`c3b254819307c177a34100b66fe19e52059ce8c4`: `MADCTL=0xa0`, complete power and
gamma values, display inversion, and MSB-first transfer of the RGB565_LE image.

The current official `cardputerzero_v0.6` image displays `splash.bmp` from
VideoCore `start.elf`, before the ARM kernel. Its firmware SHA-256 is
`d1639763fa6714e2cd4544fb45b9d5e5d54e949eaa11d7e7057651b6d4d51efd`.
Hardware proved that this firmware forces a 256/256 MB ARM/VideoCore split; it
must not be reintroduced for a faster splash. This design retains the standard
camera firmware and 64/448 MB split. Its SPI transfer uses a bounded TX/RX FIFO
stream pump rather than draining RX after each byte. It also removes the
experimental 20 ms delay after `DISPON`, because the fixed BSP flushes
immediately after that command.

The official reference for switching to the firmware splash is
`CardputerZero/pi-gen@cc5a7375dfa903757b040e76a1e64e5b0dcf8e7f`. That commit
only replaces U-Boot with a custom `start.elf` and installs `splash.bmp`; it does
not define visual handoff among Linux DRM, Weston, and the desktop. Binaries
kept in this repository for provenance auditing match the unchanged files
introduced by that commit and its later
`554544921c1659f39bf296b7986715fdeac898c8` snapshot.

## Continuous Visual Handoff

Product boot must not intentionally clear the panel between direct SPI, fbdev,
Weston, and Setup/Home. The sequence is:

1. The initramfs direct-SPI helper writes the product image as early as possible.
2. After DRM fbdev appears, the background worker redraws the same RGB565 image
   to cover any panel-RAM reset during driver probe.
3. `cardputerzero-display-retry.service` waits only for the cold-boot LCD
   stabilization point while retaining framebuffer contents. It must not stop
   or restart Weston.
4. Weston starts exactly once. `cp0-compositor` immediately autolaunches the
   trusted `os.cardputerzero.boot-splash` Wayland client.
5. `unblank-display.sh` waits for that surface's first frame callback and then
   restores a backlight that may be at zero.
6. Compositor policy hides the splash only after the complete trusted System
   Shell surface is mapped, so the first Setup or Home frame directly replaces
   the same image.

The Wayland splash uses a dedicated `WESTON_LAYER_POSITION_UI` layer. It is not
registered as an app and never appears in Apps or Tasks. App ID alone is not
authority: policy also requires the client to have the `cp0-compositor` UID, so
an ordinary app cannot impersonate the boot splash. If System Shell exits
during handoff, policy reveals the still-running splash instead of a black
background or another app's content.

The helper is a bounded program in the normal initramfs. It reads a hash-pinned
product image and preserves the standard root-discovery, data-expansion, and
OverlayFS chain. `alarm(2)` terminates abnormal register access, while the
calling script adds BusyBox `timeout -s KILL 2`. SPI-idle and RX-FIFO polling
also have fixed attempt limits. LCD failure can therefore skip the splash but
cannot block boot.

Do not package the M5Stack `m5stack_bootscreen` firmware. Build and final-rootfs
gates explicitly reject its historical SHA-256
`d1639763fa6714e2cd4544fb45b9d5e5d54e949eaa11d7e7057651b6d4d51efd`.
Each image records its standard firmware version in the package inventory.

The product command line uses `quiet loglevel=3 logo.nologo`,
`vt.global_cursor_default=0`, `fbcon=map:off`, `systemd.show_status=false`, and
`rd.systemd.show_status=false`; it does not enable the LCD console banner. The
recovery image retains `loglevel=6 fbcon=map:1` and the boot summary. Runtime
Recovery Mode maps tty1 to the udev-managed `/dev/fb_lcd` through a fixed helper
and does not depend on HDMI/LCD framebuffer enumeration order.

## Rationale

Supplier firmware can initialize ST7789 before the ARM kernel, but hardware on
2026-08-06 showed that it ignores `gpu_mem_512=64`, assigns 256 MB to VideoCore,
and leaves Linux about 227 MiB. That cost breaks app capacity, performance
gates, and ADR 0003 on a 512 MB CM0. It is unacceptable.

Product boot hides kernel and console output with
`quiet loglevel=3 logo.nologo fbcon=map:off`. The panel therefore remains black
only until the Linux direct-SPI helper runs, after which the splash appears
without exposing boot logs or waiting for framebuffer, writable data, or
userspace services.

The Circle bare-metal splash can be built from source, but its current stable
implementation requires renaming the kernel, writing FAT32, and rebooting a
second time. This increases cold-boot latency and the power-loss window and is
not part of the default product boot chain. The initramfs direct-SPI helper runs
after the ARM kernel begins, later than a VideoCore firmware splash but before
any Linux display driver. It retains maintainable standard firmware, the
correct memory split, and one boot. Later workers poll in the background and
must not delay root discovery or first-boot data-partition expansion.

## Consequences

Every `raspi-firmware` update still requires V0.6 acceptance for cold boot with
and without HDMI, IMX219, LCD orientation and color, the 64/448 MB split,
normal restart, unexpected power loss, and recovery console. Image gates verify
the standard `start_x` selection, firmware-file presence, absence of the old
vendor hash, and the splash's exact size and hash. They also require a static
ARM64 `early-splash-spi` in product initramfs and prohibit it from recovery
initramfs.

Normal product boot no longer displays the IP address, login prompt, or kernel
errors on the LCD. Diagnostics rely on Home, the network lease, mDNS,
Owner-authorized SSH, or explicit recovery media/Recovery Mode. The recovery
image retains a fully visible console so the splash design does not eliminate
local repair.

Static and build gates prove the absence of compositor restart/stop boot paths,
fixed Wayland splash dimensions and resource paths, trusted-UID checks,
first-frame markers, and System Shell ordering. Final acceptance still requires
V0.6 cold-boot video or continuous observation: host tests cannot prove handoff
among panel RAM, DRM atomic modesetting, and backlight control.
