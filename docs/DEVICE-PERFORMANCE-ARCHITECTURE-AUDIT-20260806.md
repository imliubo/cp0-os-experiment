# V0.6 device performance and architecture audit

<!-- doc-locale: en -->
> **English** | [简体中文](DEVICE-PERFORMANCE-ARCHITECTURE-AUDIT-20260806.zh-CN.md)

## Test identity

- Date: 2026-08-06, Asia/Shanghai
- Device: CardputerZero V0.6, Raspberry Pi CM0, 512 MiB RAM
- Display: 320x170 ST7789 LCD
- Camera: Raspberry Pi IMX219
- Source baseline: `5f4ea63` (Camera scheduling fix `d603bdc`)
- Image profile: product layout with the isolated hardware-debug access profile
- Root layout: read-only `mmcblk0p2` lower root plus a 64 MiB volatile overlay
- Persistent layout: `mmcblk0p3`, independently mounted at
  `/var/lib/cardputerzero` and the other approved state paths

The core binaries and eight built-in applications were hot-deployed into the
volatile overlay before this audit. A normal reboot therefore restores the
older lower root. The results below validate the current implementation on the
real CM0 but are not a clean-boot production release acceptance.

## Outcome

The current camera, Gallery, application lifecycle and idle-resource design is
viable on the single-core CM0. The visible Camera UI sustains 30.14 public
preview frames per second, performs a warm 1280x720 capture in 58 ms, and
releases the camera pipeline after leaving the foreground. All eight built-in
applications start,
publish a surface and stop within the measured bounds. A 120-second idle
monitor completed with no foreground application, no service restart, no SD
write and bounded memory growth, and its complete evidence was independently
verified on the host.

Camera remains the dominant workload. While foreground, the App Runtime used
34.512% CPU, camerad used 52.565%, appd used 4.051%, and the compositor used
3.293%. This is close to one full CM0 core in aggregate, but the measured UI
still met the 30 FPS public preview target. The architecture should retain the
fixed-resolution, single-producer pipeline and must not add a concurrent
high-resolution preview, a second camera process or background preview work.

A follow-up audit found two ways a hidden Camera could retain work. The Camera
App kept its previous `Live` state after appd rejected a background request, so
it continued the 33 ms preview retry and 1 ms input-poll loops. Separately, a
restarted System Shell displayed Home without clearing appd's previous
foreground identity. The App now enters an explicit unavailable state, retries
at 750 ms, waits up to 250 ms while inactive and keeps a two-millisecond
foreground scheduling margin. Shell startup now fails closed unless it can
clear any stale foreground identity before dispatching UI events.

## Camera pipeline

The deployed pipeline uses one continuous `rpicam-vid` process with the fixed
product-owned configuration:

- sensor output: 1280x720 YUV420;
- selected sensor mode: 1920x1080, 10-bit packed;
- mounting correction: 180-degree rotation;
- internal target: 40 FPS;
- public preview: 320x170 RGB565 at a 30 FPS target;
- photo: 1280x720 JPEG plus a 320x170 Gallery thumbnail;
- background behavior: stop the camera process after two seconds without a
  foreground request.

Measured device results:

| Operation | Result |
| --- | ---: |
| Visible Camera UI preview throughput | 30.14 FPS |
| Cold pipeline to first complete frame | 1.345 s |
| Warm 1280x720 capture | 58 ms |
| Camera capture through Gallery import | 312 ms |
| Pipeline after returning Home | stopped within about 2 s |

The visible preview result counts the exact 320x170 RGB565 payload bytes read
by the Camera Runtime while its compositor surface was active: 453 complete
frames over 15.029 seconds. The trusted screenshot from the same activation
shows the immersive Camera surface in `LIVE` state.

The camera and JPEG evidence from this run is almost black because the lens was
pointed at a dark test environment. Protocol sizes, frame cadence, capture
latency, JPEG validity and UI rendering were still verified; exposure and
subject quality were not used as pass criteria.

## Gallery pipeline

Gallery remained inside the WASM application boundary. It received only sealed
RGB565 view descriptors and never received a JPEG path, storage key or raw
filesystem access. On photo ID 46, the first Fit decode took 115.0 ms. Cached
Half, Actual center, Actual left and Actual right views took 4.7-4.8 ms, and all
five frame hashes differed.

Physical-key injection through the compositor seat verified unmodified `Z`,
`X`, `C` and `F` input. `Z` and `F` select the previous image; `X` and `C`
select the next image. Both ends wrap. Enter opens the original-resolution view,
zoom and pan change the rendered viewport, and returning preserves the selected
photo.

## Built-in application performance

Every row below completed with `PASS`. Surface-ready time is measured from the
control request until the compositor observes the App surface. CPU values are
percent of the single CM0 core during the active sample.

| App | Surface ready | Stop | App CPU | Peak App memory |
| --- | ---: | ---: | ---: | ---: |
| Hello Card | 480 ms | 200 ms | 0.172% | 10.05 MiB |
| Calculator | 483 ms | 201 ms | 0.028% | 9.88 MiB |
| Neon Snake | 480 ms | 223 ms | 5.615% | 10.49 MiB |
| Camera | 477 ms | 212 ms | 34.512% | 10.80 MiB |
| Gallery | 505 ms | 202 ms | 0.068% | 10.24 MiB |
| Media Controls | 454 ms | 203 ms | 0.655% | 9.87 MiB |
| Notes | 515 ms | 199 ms | 0.071% | 9.74 MiB |
| Stopwatch | 457 ms | 209 ms | 0.122% | 9.73 MiB |

No application wrote to the SD card during its measured start/active/stop
window. The camera sample briefly used 12 KiB of swap; this remained allocated
after the App stopped and did not grow during the following applications.

## Idle stability

The host independently verified
`target/device-evidence/stability/20260806T031828Z-21323/20260806T031828Z-21323`
with `scripts/verify-stability-evidence.sh`.

- requested duration: 120 seconds;
- sampling interval: 5 seconds;
- complete timeline: 25 block-I/O rows, 25 foreground rows and 24 sample epochs;
- foreground count: zero at every epoch;
- service restarts: zero;
- SD writes: zero bytes;
- compositor memory: 7,622,656 to 7,884,800 bytes;
- System Shell memory: 2,572,288 to 2,625,536 bytes;
- appd memory: 1,024,000 to 1,269,760 bytes.

An earlier 30-second Home sample measured appd at 0.059%, System Shell at
0.676% and camerad at 0.004% CPU. Removing idle App-catalog polling reduced
appd from the former 4.787% observation by about 80 times.

The Camera follow-up reproduced a separate resident-background busy loop.
Before the fix, the hidden Camera used about 3.4-4.1% CPU and drove appd to
about 2.9-3.6%; stopping Camera reduced appd to about 0.4%. An intermediate
bounded-retry build measured 1.03% Camera and 0.72% appd. The final 30.41-second
Home sample, with Camera, Gallery and Calculator still resident, measured:

| Resident service | CPU |
| --- | ---: |
| Camera App Runtime | 0.791% |
| appd | 0.635% |
| System Shell | 0.669% |
| compositor | 0.648% |
| Gallery App Runtime | 0.070% |
| Calculator App Runtime | 0.029% |
| camerad | 0.003% |

Both the normal F1 Home transition and a deliberate System Shell restart
revoked Camera foreground access. In each case the continuous `rpicam-vid`
process exited within about two seconds while the Camera task remained
resident. Reopening Camera through Apps restored a visible `LIVE` surface and
the 30.14 FPS cadence.

## Isolation, access and persistent data

The post-run read-only audit found no failed systemd unit and no running App.
The compositor, System Shell, appd, camerad, storaged, stored and provisiond
were active with zero restarts. App Runtime remains transient and devd remains
socket activated when idle.

Foreground camera authority is now coupled to visible Shell state across Shell
process restarts. A Shell that cannot clear the previous appd foreground token
exits instead of drawing Home while a hidden App retains foreground-only
capabilities.

The App package tree is root-owned. Private App data is `0700 cp0-storage`, the
registry is `0700 root:root`, and `permissions.json` is `0600 root:root`.
Camera had persistent `camera.capture` and `photos.write` grants; Gallery had a
persistent `photos.read` grant. No denied decision was present. The shared
photo library occupied about 9.0 MiB, while the persistent partition had about
25.4 GiB available. Photos and trusted screenshots are not subject to an
automatic item-count eviction policy.

This media is intentionally a hardware-debug artifact. It contains
`/etc/cardputerzero/hardware-debug-access` and a password-sudo policy for the
temporary operator account. Root itself remained locked; sshd reported
`PermitRootLogin no` and effective forwarding disabled. Developer Mode was Off
at the final audit, while the independently controlled Owner SSH Shell was On.
None of the hardware-debug marker, sudo policy, operator account or credential
may appear in the production image.

## Visual evidence

Trusted screenshots and camera/Gallery evidence are retained below
`target/device-evidence/ui/20260806T025529Z`,
`target/device-evidence/camera-gallery-20260806`, and
`target/device-evidence/ui/20260806T033051Z`. The follow-up visible Camera and
post-Shell-restart Home captures are under
`target/device-evidence/ui/20260806-camera-background-fix`. The trusted frames
are exact 320x170 captures with no leaked App or Tasks content.

## Release gates still open

The following results require a newly built and freshly burned production
image. They cannot be closed by the volatile hot deployment:

1. Verify the mounted production root contains the current Shell, appd,
   Runtime, camerad, brokers, eight built-in Apps and QA-independent udev rules.
2. Prove the image contains no hardware-debug marker, sudo policy, shared
   credential, root access or default SSH listener.
3. Perform one normal restart, verify a new boot ID and Home on the LCD, and
   confirm all current binaries and persistent media survive the restart.
4. Run and independently verify factory and official Phase 6F performance
   acceptance from the clean boot.
5. Run capability `--full`, then restart and run `--persistence-only`.
6. Complete the six Store refresh/resume/upgrade/offline/stale acceptance
   sequence and independently verify all evidence.

Power-off, first-boot interruption, production Developer Mode/Owner Shell
separation, recovery media, A/B/verity and external power measurements remain
separate hardware release gates.
