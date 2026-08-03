# Multitasking main integration report

## Decision

The multitasking branch is suitable for integration into `main` as a
simulation-tested architecture, protocol and UI foundation. It is not a
device-release-complete multitasking implementation. Production deployment
remains blocked on the MT3-MT6 gates below.

No device was contacted, no service was restarted and no image or system bundle
was produced during this integration.

## Integration identity

| Role | Branch or commit |
| --- | --- |
| Reviewed main baseline | `d0b8a0098c9d4b41b054842676ad625f043e4e91` |
| Multitasking source | `97438636320f7ed175a0569c482cc0aae96332d4` on `codex/multitasking` |
| Integration branch | `codex/multitasking-main-integration` |
| Cherry-pick integration commit | `e950a4f0596f36ce9034d8f534f9414a6b8b93b1` |

The review covered all 17 main-line commits after the multitasking branch point,
including first boot, guided keyboard diagnostics, Store integration, Owner
Developer Access and constrained system power control.

## Conflict resolution

The cherry-pick had two textual conflicts:

- `system-shell/src/main.c`: retained main's first-boot network state,
  Developer Access and power-control paths, then integrated task polling,
  activation, close and multi-surface tracking;
- `tests/test-system-shell-ui.sh`: retained the main-line test inputs and added
  the multitasking UI source and snapshot coverage.

The appd protocol is now v2 and the private System Shell Wayland protocol is v7.
appd, System Shell, compositor policy and Runtime must be released as one
versioned bundle; mixed endpoints fail strict version negotiation.

## Review findings fixed

1. The merged `struct cp0_ui` reached 66,232 bytes and violated the 64 KiB
   product bound. The fixed ten-row task table is now one 880-byte heap
   allocation, with explicit `cp0_ui_deinit()` teardown.
2. A checkpointed or crashed logical task has no active systemd unit, so the
   legacy `is_running()` gate could permit an upgrade or uninstall while that
   task still referenced the old package version. All non-idempotent install,
   upgrade, rollback and uninstall paths now reject any matching logical task.
   Exact-version Store replay remains intentionally idempotent.
3. A restarted App UID may temporarily have an old and a new surface token.
   Shell now prefers the most recently announced token. This closes the local
   selection regression but is not the production identity solution; MT3 must
   bind `(task_id, runtime_generation)` in the compositor.

The merge also preserves Intent senders as background tasks instead of stopping
them after a successful handoff.

## Implemented foundation

- one foreground application and one keyboard focus;
- at most ten logical tasks and at most one task per App;
- creation-order FIFO eviction for App 11, independent MRU card ordering;
- foreground, background, frozen, checkpointed and crashed task states;
- appd protocol v2 list, activate and close operations;
- multiple Runtime session bookkeeping with generation-safe exit handling;
- F3 fixed-size 160x85 task cards, keyboard navigation and placeholder states;
- versioned atomic TaskJournal, bounded checkpoint envelope, trusted-thumbnail
  cache model and deterministic resource-governor model;
- optional C, Rust and WIT lifecycle ABI for bounded checkpoint/restore.

TaskJournal startup recovery, Runtime control, real checkpoint callbacks,
compositor capture and pressure policy are model-only or unconnected in this
slice. Placeholder task cards are not evidence of live device thumbnails.

## Security review

The integration preserves per-App UID, namespace, cgroup, seccomp and broker
boundaries. Background execution does not grant process, cgroup or compositor
control. Shell continues to authenticate the compositor peer and uses the
kernel-observed App UID rather than client-provided app-id text.

UID is sufficient only for the current one-task-per-App simulation. It cannot
distinguish a stale surface from a restarted Runtime of the same App. Production
activation and thumbnail delivery therefore fail the release gate until the
Runtime and compositor authenticate both task ID and runtime generation.

Checkpoint payloads remain App-private, limited to 8 KiB, versioned and hashed.
The SDK does not expose the reserved checkpoint namespace, and a timeout or
invalid payload cannot prevent FIFO eviction.

## Developer Mode hot update assessment

Owner Developer Access can install signed `.capp` packages and proxy bounded
App lifecycle commands. It cannot replace appd, System Shell, compositor
policy, Runtime, systemd units or the OS image. It therefore cannot hot-update
the system components required for multitasking.

After MT3-MT5 are implemented, device integration requires an owner-authorized
coordinated system bundle and normal reboot, or a newly flashed image. Developer
Mode can then install SDK test Apps, but cannot establish the multitasking
system baseline by itself.

## Verification matrix

| Check | Result |
| --- | --- |
| `git diff --check` | PASS |
| `cargo fmt --all -- --check` | PASS |
| `cargo check --workspace --all-targets` | PASS |
| `cargo test -p cp0-appd` | PASS, 117 tests |
| `cargo test -p cp0-sdk` | PASS, 18 tests |
| `tests/test-system-shell-ui.sh` | PASS |
| `tests/test-appd-profile.sh` | PASS |
| `tests/test-compositor-profile.sh` | PASS |
| `tests/test-developer-access.sh` | PASS |
| `tests/test-power-control.sh` | PASS |
| `tests/test-device-deployment.sh` | PASS |
| `tests/test-malicious-apps.sh` | PASS |
| `tests/test-security-validation.sh` | PASS |
| `tests/test-patch-cm0-dtb.sh` | PASS |
| ARM64 System Shell and compositor policy full link | PASS |
| `make check` excluding local-listener Store origin cases | PASS |

The ARM64 link used the repository-pinned Weston 14 inputs. Artifact hashes:

- System Shell: `4f149fa3d9a036f6c24ebb45bd2bf33e118736d69785d263b97826528fb470b0`;
- compositor policy: `130b98fedf8bce9bc616381b582aad88c7742e46d308b1bfd29269fd1ed9227f`.

Two Store origin checks cannot run in the managed host sandbox because binding
`127.0.0.1` returns `EPERM`:

- `tests/test-store-origin.sh`;
- `cp0-stored::tests::rejects_mismatched_http_range_without_appending_and_then_recovers`.

These are environment limitations, not multitasking failures. Store code was
not changed to bypass the listener restriction.

The Tasks and notification 320x170 simulation snapshots are stable at:

- Tasks: `879c45ff089f2ef29fbbeb019199dfd4797d06c9bd4e01590b47d1c381f95d80`;
- notification: `9339f99f3b7134f1df3089248ecfacc60a7f461a01971eda10c780360ac2f1ec`.

## Release gates

- **MT3 / Phase 6L-C:** connect TaskJournal startup reconciliation, authenticated
  Runtime control and compositor `(task_id, runtime_generation)` binding.
- **MT4 / Phase 6L-D:** capture compositor-owned RGB565 thumbnails into sealed
  read-only objects, enforce 2 Hz updates and pass stale/forged identity tests.
- **MT5 / Phase 6L-E-F:** wire fuel- and deadline-bounded WAMR checkpoint/restore,
  private broker persistence, App 11 FIFO behavior and CM0 resource thresholds
  measured with 1, 3 and 10 Apps.
- **MT6 / Phase 6L-G:** deploy one coherent bundle only after authorization,
  reboot normally, then verify F3, Intent, Developer Access lifecycle, App 11,
  appd restart, persistence and power-loss recovery before image release.

Until all gates pass, the branch must not be described as providing live task
thumbnails, durable device resume or a production CM0 background policy.

## Merge recommendation

Merge the integration branch into `main` as the Phase 6L-A/B foundation while
keeping Phase 6L-C through 6L-G open. A fast-forward or ordinary merge is safe
only after confirming that `main` still points to the reviewed baseline; if it
has advanced, rebase or merge it into the integration worktree and rerun the
matrix. Do not deploy this commit independently to a device because the v2/v7
protocol endpoints are intentionally incompatible with the previous bundle.
