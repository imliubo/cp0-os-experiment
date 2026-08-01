# Store Update Queue v1

S7B adds an explicit, bounded Update All operation without introducing parallel
downloads or weakening the signed Catalog identity used for installation. A
single application install and an Update All batch use the same daemon-owned
serial worker.

## Protocol

Protocol version 1 adds the strict command:

```json
{
  "name": "install-batch",
  "app_ids": [
    "dev.cardputerzero.alpha",
    "dev.cardputerzero.beta"
  ]
}
```

`app_ids` contains 1 through 8 valid application IDs in ascending byte order.
Empty, oversized, duplicated, unsorted, invalid, or unknown IDs reject the
whole request. A successful response preserves the exact request order and
binds every accepted ID to the Catalog version snapshot:

```json
{
  "kind": "install-batch-accepted",
  "apps": [
    {"app_id": "dev.cardputerzero.alpha", "version": "2.0.0"},
    {"app_id": "dev.cardputerzero.beta", "version": "3.1.0"}
  ]
}
```

Rust, CLI, and C clients reject a partial, reordered, duplicated, malformed, or
extended response. Acceptance is all-or-nothing: while holding the Store state
lock, `cp0-stored` verifies every ID against one fresh verified Catalog,
snapshots each version and package SHA-256, reserves the global mutation job,
and publishes every initial operation as `queued`. No download starts before
that atomic acceptance step completes.

## Serial queue

The daemon owns one worker and processes accepted applications in request
order. It retains the global mutation reservation until every queue entry is
terminal, so refresh, media-cache mutation, another install, and Resume cannot
interleave a different Catalog or package identity with the queue.

Each entry still has its own S7A control state. Pause retains only that entry's
digest-named partial file; Cancel removes only that entry's partial file; a
network, storage, verification, or installer failure is recorded only on that
entry. The worker then advances to the next queued application. A paused entry
can be resumed after the current batch releases the global job. Canceling an
entry that has not started is cooperative and is observed before package data
is transferred.

Single-item `install` is implemented through the same batch acceptance and
worker path. This keeps Busy behavior, failure classification, control races,
and Catalog binding identical between individual and Update All operations.

## System Shell

The 320x170 Updates page keeps every application with a newer Catalog version
visible through `update`, `queued`, `downloading`, `paused`, `installing`,
`failed`, and `canceled` states. A separate `UPDATE ALL N` command row selects
at most eight currently eligible entries:

- `update`
- `failed` with an update available
- `canceled` with an update available

Active `queued`, `downloading`, `paused`, and `installing` entries are not
submitted again. Down from Update All selects the first application row; Up
from that row returns to Update All. Enter on an application still opens its
individual details and S7A controls. A stale Catalog continues to render the
command and update rows but blocks Update All. When no eligible entries remain,
the command loses selection so an active application row remains navigable.

The C client API is:

```c
int cp0_store_install_batch(
    const char *const app_ids[],
    size_t app_count);
```

Operators can exercise the same daemon path with sorted canonicalization in the
CLI:

```sh
sudo cp0ctl store install-batch \
  dev.cardputerzero.alpha \
  dev.cardputerzero.beta
```

Automatic updates remain disabled. Every batch is an explicit local user or
operator action.

## Verification

Protocol tests cover empty, oversized, duplicated, unsorted, malformed, and
identity-mismatched batches. Daemon tests use three distinct packages and
barriers to prove atomic acceptance, serial ordering, global mutation
ownership, per-entry Pause and Cancel cleanup, continuation after terminal
states, and later Resume.

C tests bind response count, order, ID, version, and exact object shape. UI
behavior tests cover the eight-entry bound, active-entry exclusion,
failed/canceled inclusion, stale rejection, selection reset, and individual
details. A 320x170 pixel snapshot verifies the command row and update list.
