# Store Download Control v1

S7A adds bounded, user-visible control of one Store installation without
weakening Catalog, package, or appd verification. The device still runs only
one Store mutation job at a time.

## Protocol

Protocol version 1 adds the strict command:

```json
{
  "name": "control",
  "app_id": "dev.cardputerzero.example",
  "action": "pause"
}
```

`action` is exactly `pause`, `resume`, or `cancel`. Success returns
`operation-accepted` bound to the request's application ID and action, plus the
operation version. Acceptance means the request was recorded; clients poll the
Catalog summary for the final state.

Catalog summaries add `paused` and `canceled` states. Only `failed` summaries
carry one `failure_reason`, chosen from this closed vocabulary:

- `network`
- `storage`
- `verification`
- `installer`
- `catalog-changed`
- `internal`

Unknown fields, states, actions, reasons, inconsistent progress, a missing
failure reason, or a reason on a non-failed summary are rejected by Rust and C
clients. `invalid-state` is distinct from `busy`: the former means the action
cannot apply to the operation, while the latter means a prior accepted action
has not reached its terminal state or another Store mutation owns the job.

## State and file lifecycle

The daemon records the application version and signed package SHA-256 before it
accepts an installation. A worker checks cooperative control before transfer,
at every bounded download chunk, and immediately before appd handoff.

```text
available/update -> queued -> downloading -> installing -> installed
                         |          |
                         +----------+-> paused -> queued (resume)
                         |          |
                         +----------+-> canceled
                         |
                         +------------> failed

paused/failed -> canceled
canceled/failed -> queued (new install/retry)
```

Pause keeps the digest-named private `0600` `.part` file and reports bounded
progress. Resume is accepted only while paused and only if the current verified
Catalog still has the same application version and package digest. Network
retry can reuse the same digest-bound bytes; package verification truncates a
bad full download before reporting failure.

Cancel during queued/downloading is cooperative. Cancel from paused/failed
reserves the global Store job while synchronously removing the `.part`, so a
concurrent resume, retry, refresh, or media job cannot race cleanup. Cleanup
failure becomes `failed/storage`; success becomes `canceled`. Once state is
`installing`, pause and cancel are rejected because appd owns the atomic
installation handoff. An accepted Cancel is monotonic: a later Pause cannot
replace it before the worker reaches its next control boundary, while repeated
Cancel requests remain idempotent.

A verified Catalog refresh reconciles resumable operations. If a paused or
failed operation's version or digest no longer matches, it remains visible as
`failed/catalog-changed`: resume is rejected, cancel can remove the old digest
file, and a new install starts against the new signed identity. Completed
installed operations are not misreported as Catalog failures.

## System Shell

The 320x170 detail overview exposes one primary action and, where valid, a
secondary Cancel action:

| State | Primary | Secondary |
| --- | --- | --- |
| available/update | Install | - |
| queued/downloading | Pause | Cancel |
| paused | Resume | Cancel |
| failed/canceled | Retry | Cancel for failed only |
| installing/installed | - | - |

Up/Down selects the action and Enter executes it. A stale Catalog still allows
Pause and Cancel because they reduce activity, but blocks Install, Resume, and
Retry. Update membership is derived independently from the installed version,
so an update remains on the Updates page while queued, downloading, paused,
failed, or canceled. Catalog polling is authoritative; daemon restart cannot
leave a stale local paused state stuck in the Shell.

The Shell renders the closed failure reason (`FAIL NETWORK`, `FAIL STORAGE`,
and so on) without displaying daemon error strings. CLI operators have the same
bounded path through:

```sh
sudo cp0ctl store pause dev.cardputerzero.example
sudo cp0ctl store resume dev.cardputerzero.example
sudo cp0ctl store cancel dev.cardputerzero.example
```

## Verification

Protocol tests cover strict control commands, accepted-response binding, new
states, progress, and failure reason consistency. Daemon tests use barriers to
prove pause acknowledgement, resume Busy behavior, exact partial reuse,
cooperative cancel deletion, appd handoff rejection, Catalog digest changes,
and stable network/storage/verification/installer failure classification.

C tests cover all new states, malformed failure reasons, and action-bound
accepted responses. UI behavior tests cover normal and stale Catalog actions,
authoritative daemon reconciliation, update membership, selection reset, and
failure rendering. Pixel snapshots verify the compact downloading and failed
detail layouts at exactly 320x170.
