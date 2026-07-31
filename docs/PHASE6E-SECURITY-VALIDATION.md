# Phase 6E: internal security validation baseline

## Delivered scope

Phase 6E turns the existing component-level security tests into an explicit
system security baseline. `docs/THREAT-MODEL.md` records assets, attackers,
trust boundaries, control mappings and production release blockers. ADR 0006
defines the conditions under which dm-verity, RAUC A/B and U-Boot/FIT would add
real security rather than only new boot complexity.

This phase does not claim verified boot, encrypted data, completed hardware
fault injection or independent review.

## Fuzz targets

The separate `fuzz/` workspace keeps libFuzzer and sanitizer dependencies out
of product binaries. Five targets exercise all high-risk serialized entry
points:

| Target | Inputs exercised |
| --- | --- |
| `manifest` | strict manifest JSON and semantic validation |
| `package` | raw and structured-mutated `.capp` v1, canonical re-encoding and signatures |
| `store_protocol` | signed catalog JSON plus terminated/unterminated Store IPC frames |
| `appd_control` | terminated/unterminated appd request and response frames |
| `recovery_backup` | in-memory `CP0 backup v1` header, entry and payload parsing |

`cp0-recovery` exposes its byte-slice verifier only for tests or the `fuzzing`
feature. Product builds continue to expose only file-based backup, verify and
restore operations.

Install the host tool outside the product workspace dependency graph, type
check every target, then run a bounded local campaign:

```sh
cargo +nightly install cargo-fuzz --locked --root target/fuzz-tools
make fuzz-check
./scripts/fuzz-smoke.sh 30
```

The smoke runner applies a 64 KiB input cap, a five-second per-input timeout,
AddressSanitizer and a 1536 MiB host RSS limit. It is a regression gate, not a
substitute for long-running fuzzing. The scheduled CI workflow runs every
target for 30 seconds and preserves crash artifacts.

## Acceptance boundary

Phase 6E internal acceptance requires all targets to build, a local sanitizer
smoke campaign with no crash/timeout/OOM, `make check`, workspace Clippy and a
clean diff. Any crash becomes a minimized permanent regression input before a
fix is accepted.

The external portion remains open: an independent reviewer must receive the
threat model, pinned build inputs, fuzz corpora, image gates and hardware test
results. Review findings must be triaged by severity and resolved or explicitly
accepted by the product owner; this repository cannot self-certify that step.
