# Phase 3M: Device capability acceptance

The Phase 3 device gate uses real SDK applications rather than privileged test
requests. `dev.cardputerzero.acceptance` and
`dev.cardputerzero.isolation` are developer-signed acceptance packages that
are installed only on a dedicated test device. They are not included in the
product image.

## Trust path

Every tested operation follows the production path:

```text
acceptance WASM
  -> public Rust SDK host call
  -> sandboxed App Runtime
  -> appd SO_PEERCRED, active-cgroup and manifest authentication
  -> permission decision
  -> restricted capability service
  -> V0.6 hardware or app-private storage
```

The device harness never opens the Runtime broker socket. It resets only the
two dedicated applications' permissions, starts one foreground application at
a time and resolves the trusted appd prompt. Each probe stores a bounded result
through its own SDK private-storage identity; the root harness reads that
dedicated `cp0-storage`-owned result file after its inode changes. Notifications
remain visible to the operator but are not used as an automation channel,
because System Shell is also a notification consumer. A raw root broker request
therefore cannot produce a passing capability result.

## Coverage

The primary probe performs the following operations:

- fills its 1 MiB private quota with 128 maximum-size values, verifies that the
  next write is rejected, cleans the temporary values and leaves one marker;
- verifies that the marker survives process stop/start;
- requests playback and capture independently and reports whether capture
  returned a nonzero sample;
- reads, inverts, reads back, restores and verifies only the logical
  `grove-function` GPIO;
- reports denied results after persistent deny decisions, then repeats after
  explicit allow decisions.

The isolation probe requests the primary application's marker name under its
own identity and must receive `not found`. The harness also checks exact sysfs
and private-storage owner/mode values and verifies that the login user cannot
bypass `cp0-gpiod`. While each acceptance application is active, it also reads
the actual transient unit properties and requires the fixed 60 percent CPU
quota, CPU weight 50, 16 MiB manifest memory limit, zero swap and 32-task cap.

## Build and provision

Build reproducible packages with an acceptance-only developer key stored under
the ignored `target/` tree:

```sh
./scripts/build-device-capability-apps.sh
```

The command prints the developer key ID and produces the public key plus two
signed `.capp` files in `target/device-capability-acceptance`. On a dedicated
device, place the public key at the exact printed root-owned trust path, enable
developer mode, install both packages with `sudo cp0ctl install`, then disable
developer mode again. Package installation records stable application UIDs;
turning developer mode off does not broaden their runtime permissions.

## Run and retain evidence

Do not run capability acceptance while a core stability run is active. The
harness enforces this and writes only RAM-backed evidence:

```sh
sudo CP0_AUDIO_OBSERVED=yes \
  /usr/libexec/cardputerzero/device-capability-acceptance --full
```

Set `CP0_AUDIO_OBSERVED=yes` only when an operator actually hears the bounded
test tone. A successful PCM write without that observation remains a warning.
An all-zero capture also remains a warning even though the broker and ALSA
capture completed.

Retrieve `status`, `checks.tsv` and `summary.env` from the reported directory
before rebooting. To prove SD-backed persistence, reboot normally and run:

```sh
sudo /usr/libexec/cardputerzero/device-capability-acceptance \
  --persistence-only
```

The second run must report `storage=persist-ok`, retain the permission
decisions without a new prompt and record a different kernel `boot_id`. Retrieve
that RAM-backed result as well before any further reboot.

Verify the two retrieved directories together:

```sh
./scripts/verify-device-acceptance-evidence.sh capability \
  PATH_TO_FULL_RUN PATH_TO_PERSISTENCE_RUN
```

The host verifier requires the full capability/resource/sysfs/storage check
set, validates the bounded private-storage result files and proves that the
persistence run has a later finish time and a different kernel boot ID.

The full run intentionally leaves the dedicated marker, result and permission
decisions in place for the reboot check. It never restarts a platform service,
reboots, mounts, formats or reads another application's result or marker file.
