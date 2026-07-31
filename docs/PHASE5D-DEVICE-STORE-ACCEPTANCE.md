# Phase 5D: Device Store acceptance

This gate validates the complete production Store path on the CM0 device. It
uses two SDK-only versions of the same application and never gives the device a
package path, hash, version or signature override.

## Security boundary

The test keeps the production network policy intact. The origin must be a real
public HTTPS endpoint with a publicly routable address and a certificate that
passes normal validation. A private LAN server, literal IP address, HTTP
endpoint, disabled TLS verification or environment proxy is not an acceptable
substitute.

The device harness refuses to run while
`cardputerzero-stability-acceptance.service` is active. It writes evidence only
below `/run/cardputerzero-store-acceptance`, does not reboot or reconfigure the
device, and kills only the main process of `cardputerzero-stored.service` during
the resume test. The socket and `Restart=on-failure` policy must start a new
service process.

The acceptance packages, developer key and Store signing key remain below the
ignored host `target/test-store` directory. They are not embedded in a product
image. Only the 32-byte Store public key is provisioned on the test device.

## Controlled public origin

The recommended acceptance origin is the repository's loopback-only server
behind a Cloudflare Quick Tunnel. The runner prefers the repository-local
source build at
`target/tools/cloudflared-2026.7.3/source/cloudflared`, then the absolute path
in `CP0_CLOUDFLARED`, and finally `cloudflared` from `PATH`. Confirm the selected
executable without starting a tunnel:

```sh
./scripts/run-test-store-origin.sh --print-cloudflared
```

The repository-local acceptance binary was built from the vendored dependencies
at Cloudflare tag `2026.7.3`, commit
`3a2b45c2a511fcdd81b68c190938e4ffadbea5dc`, with the verified Go 1.26.5
darwin/arm64 toolchain. Its SHA-256 is
`0a59c7b61dedf9096d3df3ee52c7cef81ab31614e8fc8457e864506eae7aa672`.
The generated binary and source checkout remain below ignored `target/` and are
not image inputs. The runner verifies that exact digest and stops if a file at
the repository-local path does not match; `CP0_CLOUDFLARED` and `PATH` are
explicit operator trust choices. Build or select a trusted executable before
continuing; the runner does not download or update `cloudflared`.

Run this in a dedicated host terminal shortly before device acceptance:

```sh
./scripts/run-test-store-origin.sh 18080 1800 524288
```

The runner performs the order-sensitive setup automatically:

1. creates a root-only control file with throttled v1 selected;
2. binds the origin only to `127.0.0.1`;
3. obtains a random public `https://*.trycloudflare.com` URL;
4. builds and signs both catalogs against that exact URL and current time;
5. remains in the foreground so stopping the runner also stops both processes.

The public endpoint serves only the active signed catalog, its public key and
the matching test package. It never serves the developer key, Store signing
key, review files, control file or arbitrary paths, and sends `Cache-Control:
no-store`. The generated catalog and package are non-sensitive test artifacts,
but they transit Cloudflare and the random URL is publicly reachable for the
life of the runner.

The runner prints the exact control commands. Initially v1 is limited to 512
KiB/s, which leaves a reliable interruption window for an 8 MiB package while
remaining inside `cp0-stored`'s 45-second download timeout. After the v1 resume
test, switch to unthrottled v2 from another host terminal:

```sh
node scripts/test-store-origin.mjs set \
  target/test-store/origin-control.json v2
```

Before the offline-cache action, switch the same HTTPS endpoint to a controlled
HTTP 503 fault:

```sh
node scripts/test-store-origin.mjs set \
  target/test-store/origin-control.json offline-v2
```

This keeps DNS and TLS stable while ensuring the refresh reaches a real public
origin failure. Host-side request logs, tunnel logs, URL, PIDs and validity
times are retained below the printed `target/test-store-origin/<run-id>`
directory. Keep the runner alive through the stale-catalog action.

## Manual static origin

Choose a public HTTPS base URL whose document root can be switched atomically
between two static directories. Use a short lifetime only when the complete
online sequence can finish before expiry:

```sh
published=$(date +%s)
CP0_TEST_STORE_PAD_BYTES=8388608 \
  ./scripts/build-test-store.sh \
  https://store.example.com/cardputerzero-acceptance \
  "$published" 1800
```

The command creates:

- `catalog-v1`: version 1.0.0, rendered green after launch;
- `catalog-v2`: version 1.1.0, rendered blue after launch;
- `store.pub`: the test catalog trust key;
- `acceptance.json`: exact URL, key ID, sequences and validity window.

It also verifies the developer and Store signatures. The review records bind no
permissions and exactly these SDK imports:

```text
cp0_display_dimensions
cp0_present_rgb565
cp0_wait_event
```

When using a manually managed origin, serve `catalog-v1` as the base URL root
first. Configure the package response for low, steady throughput before the
resume action; the default 8 MiB padding must remain partially downloaded long
enough for the harness to observe and interrupt it, but the resumed transfer
must still finish inside 45 seconds. The origin must support a correct `Range`
request and return HTTP 206 with a matching `Content-Range`.

## Provision the test device

Start from a device with no prior `dev.cardputerzero.store-test` installation
and no Store partial files. Install the current app-platform deployment after
the 24-hour stability result has been retrieved. Provision
`/etc/cardputerzero/store.conf` as root-owned mode 0644 with the generated
catalog URL, and install exactly one test key as:

```text
/etc/cardputerzero/trust/store/<store-key-id>.pub
```

The key must be the generated `store.pub`, root-owned mode 0644. Restart
`cardputerzero-stored.service` once after provisioning so it reloads the
configuration and trust root. This is a test-only trust configuration, not a
production Store enrollment.

## Ordered run

Read `sequence_v1` and `sequence_v2` from `acceptance.json`. Run every command
as root and retrieve its reported directory before any reboot.

1. With `catalog-v1` online, refresh and verify its exact identity:

   ```sh
   /usr/libexec/cardputerzero/device-store-acceptance \
     refresh-v1 <sequence_v1>
   ```

2. Keep the v1 package throttled, then prove partial-file survival, a new
   `cp0-stored` PID, validated HTTP 206 resume, install and launch:

   ```sh
   /usr/libexec/cardputerzero/device-store-acceptance resume-v1
   ```

3. Atomically switch the same public origin root to `catalog-v2`, remove the
   package throttle, refresh, upgrade and launch:

   ```sh
   /usr/libexec/cardputerzero/device-store-acceptance \
     refresh-v2 <sequence_v2>
   /usr/libexec/cardputerzero/device-store-acceptance upgrade-v2
   ```

4. Take the public origin offline while the signed catalog is still valid. The
   refresh must fail inside `cp0-stored`, while the cached v2 catalog remains
   browsable:

   ```sh
   /usr/libexec/cardputerzero/device-store-acceptance \
     offline-v2 <sequence_v2>
   ```

5. Keep the origin offline, wait until `expires_unix_seconds`, then prove the
   cached catalog is marked stale and cannot authorize another install:

   ```sh
   /usr/libexec/cardputerzero/device-store-acceptance \
     stale-v2 <sequence_v2>
   ```

The harness starts each installed version for two seconds. The operator may
observe green for v1 and blue for v2 on the LCD, but visual color is supporting
evidence; the signed installed version reported by appd is authoritative.

## Evidence and pass criteria

Each action produces a root-only RAM directory containing `status`,
`checks.tsv`, `summary.env` and the bounded command responses used by that
action. Retrieve the complete directory before reboot because `/run` is
volatile.

All six actions must report `PASS`. The final evidence set must show:

- exact v1 and v2 catalog sequences with `stale=false` during online tests;
- a nonempty partial smaller than the signed package byte count;
- a different `cp0-stored` main PID after the targeted kill;
- the validated resume journal marker and successful v1 install;
- strict v1 to v2 upgrade and successful launch of each version;
- a real origin failure without loss of the cached catalog;
- `stale=true` after expiry and an `Untrusted` install rejection;
- root-owned Store configuration/trust and private `cp0-store` cache modes.

No flashing is required for this gate if the current app-platform deployment is
installed after the stability run. Reflash only to obtain the required fresh
Store/app state when a previous attempt has left test data behind.
