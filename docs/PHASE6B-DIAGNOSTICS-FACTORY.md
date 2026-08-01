# Phase 6B: privacy-preserving diagnostics and factory acceptance

## Policy

CardputerZero diagnostic and support tooling does not upload telemetry.
Diagnostic collection is a local, explicit root operation and all generated
files remain below `/run`, so they are RAM-backed and disappear on reboot.
Support tooling does not contact a network endpoint, create a persistent
identifier or modify application state. The separately consented Store weekly
counter contract in `STORE-METRICS-V1.md` cannot read or include these bundles.

The default support bundle excludes:

- application IDs, packages, private storage and Document Portal contents;
- Wi-Fi profiles and SSIDs, IP and MAC addresses;
- SSH keys, hostname, machine ID, boot ID and hardware serial numbers;
- raw kernel and service journals.

It includes the OS/kernel version, non-identifying V0.6 hardware presence,
allowlisted service state and exit status, memory counters, protected mount
properties and the aggregate SD write counter. This is enough to distinguish a
missing device, failed unit, memory pressure, writable-root regression or SD
write anomaly without collecting user content.

Generate the default bundle from a recovery console or SSH session:

```sh
sudo /usr/libexec/cardputerzero/device-support-bundle
```

The command prints the exact root-only `tar.gz` path below
`/run/cardputerzero-support`. It also retains the unpacked source next to the
archive for local inspection. Nothing is uploaded automatically.

Raw logs can be diagnostically necessary, but service messages may contain
application IDs, URLs, paths or user-entered text. They therefore require a
separate explicit operation:

```sh
sudo /usr/libexec/cardputerzero/device-support-bundle --include-journal
```

That bundle records `journal_included=1`, names the file
`sensitive-journal.txt` and carries an inspection warning. Operators must get
user consent and inspect it before transfer. There is deliberately no upload
command in the image.

## Factory gate

`device-factory-acceptance` is the unprovisioned V0.6 release gate. It is
read-only: it does not capture a camera frame, play or record audio, drive GPIO,
transmit LoRa, mount filesystems, restart services or change a device mode.
Run it only after the first boot of the three-partition product image and before
enabling developer/recovery mode or installing user applications:

```sh
sudo /usr/libexec/cardputerzero/device-factory-acceptance
```

Every invocation creates an isolated directory below
`/run/cardputerzero-factory` containing:

- `hardware-smoke.txt`, the existing CM0/LCD/keyboard/audio/battery baseline;
- `checks.tsv`, one bounded PASS/FAIL row for each factory invariant;
- `summary.env`, schema, failure counts, available memory and the current SD
  write counter;
- `status`, containing `PASS` or the number of failures.

Retrieve the complete reported directory before rebooting, then validate it
from the matching source revision instead of trusting `status` alone:

```sh
./scripts/verify-device-acceptance-evidence.sh factory PATH_TO_RUN_DIR
```

The host verifier requires the complete fixed factory check set, rejects
symbolic or malformed evidence and cross-checks the summary failure counts.
Warnings emitted by the nested hardware smoke remain visible but do not replace
any required factory invariant.

The gate requires the immutable OverlayFS profile, a labelled partition 3 that
has grown to the final 1 MiB of the SD card, an ext4 filesystem expanded to the
partition, the v1 persistent layout, clean default device modes, active core
services with no restarts, all broker sockets, exact socket owner and mode, a
live authenticated appd ping and no failed systemd units. Optional external
camera and LoRa peripherals remain outside this V0.6 factory gate.

Automated repository tests enforce the RAM-only output roots, no-delete and
read-only factory behavior, support-bundle data exclusions, explicit journal
consent, lack of upload commands and image installation of both tools. Final
acceptance still requires running the factory gate on the flashed Phase 6A
candidate and completing the physical keyboard/display checks.
