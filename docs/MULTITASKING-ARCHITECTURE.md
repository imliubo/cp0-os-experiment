# Multitasking architecture

<!-- doc-locale: en -->
> **English** | [简体中文](MULTITASKING-ARCHITECTURE.zh-CN.md)

## Product contract

CardputerZero OS remains a single-foreground system. Exactly one application
may own the 320x170 application layer and keyboard focus. F3 opens a trusted
task switcher with up to ten task cards; selecting a card activates that task
instead of launching a second instance.

An application has at most one task. Starting an eleventh distinct application
evicts the task with the smallest creation sequence, even if another task was
used less recently. Before termination, appd asks the application for a
bounded, versioned checkpoint. A timeout, unsupported callback or invalid blob
is recorded but never prevents termination. A capacity-evicted task disappears
from F3; its checkpoint remains in the application's private state and is
offered when the application is next opened from Launcher.

The ten-task limit is a product limit, not a promise that ten WAMR processes
will always remain resident on a 512 MB CM0. appd may freeze background tasks
or checkpoint and destroy their processes under memory pressure while keeping
their cards in F3.

## Task states

| State | Process | App code runs | Surface | Activation |
| --- | --- | --- | --- | --- |
| `foreground` | resident | yes | active or covered by Shell | direct |
| `background` | resident | yes, deprioritized | hidden | direct |
| `frozen` | resident | no | last frame retained | thaw, then direct |
| `checkpointed` | absent | no | trusted last frame retained | launch and restore |
| `crashed` | absent | no | trusted last frame plus crash badge | clean launch or restore |

Entering F3 gives keyboard focus to System Shell but does not change the
logical foreground task. This avoids a false foreground transition every time
the trusted overlay is opened. Switching cards changes the foreground task.

Every task has a non-zero, monotonically allocated `task_id`, an immutable
creation sequence and a monotonically updated activation sequence. Creation
sequence drives FIFO capacity eviction. Activation sequence drives the visual
most-recent-first card order. Neither wall-clock time nor compositor tokens are
used as durable identity.

## Ownership

### appd

appd is the lifecycle authority. Its task registry owns task identity, state,
runtime unit binding, foreground selection and eviction order. All mutating
operations are serialized by one lifecycle coordinator. Slow systemd,
checkpoint and compositor operations run as bounded steps with the expected
task ID and runtime generation revalidated before commit.

The existing single `Option<RuntimeSession>` becomes a task-indexed runtime
map. Each runtime monitor is tied to its task ID and generation, so an old
monitor cannot mark a restarted task as crashed. Permission prompts, document
prompts and media actions also use the foreground task ID rather than a global
single-runtime slot.

### App Runtime

Each resident task keeps its existing dedicated UID, transient systemd unit,
cgroup and bubblewrap/WAMR sandbox. Background execution does not weaken
isolation. appd changes scheduling properties through systemd; applications do
not receive a process or cgroup control API.

The launch contract adds `CP0_TASK_ID` and a pre-opened, authenticated runtime
control channel. The Runtime reports its task ID and generation to appd, and
uses a private compositor protocol to bind the resulting Wayland surface to
that identity. `xdg_toplevel.app_id` remains untrusted display text.

### Compositor

The compositor continues to enforce one normal application layer and one
keyboard focus. It may retain multiple hidden application views. A trusted
surface record is keyed by `(task_id, runtime_generation)` and has a volatile
compositor token used only by System Shell. Tokens are never persisted.

The compositor captures thumbnails from the last committed application buffer;
applications cannot submit their own task-switcher image. Capture output is
RGB565 at 160x85 (27,200 bytes). Ten images consume at most 272,000 bytes plus
small metadata. A double buffer is allocated only for the selected card, not
for every task.

While F3 is visible, resident, non-frozen surfaces may update their cached
thumbnail at no more than 2 Hz. Frozen, checkpointed and crashed tasks display
their last trusted frame. The Shell never maps an application buffer directly;
the compositor copies into a sealed read-only shared-memory object and tags it
with task ID, runtime generation and thumbnail generation.

### System Shell

System Shell obtains task metadata from appd and surface/thumbnail handles from
the private compositor protocol. It joins those sources only on trusted task ID
and runtime generation. Unmatched compositor surfaces are not shown as cards;
tasks without a current thumbnail use a Shell-owned placeholder.

F3 shows a horizontally navigable card stack sized for 320x170. Left and right
change the selected card, Up and Down select its OPEN or STOP action, Enter
executes the selected action, Space stops it directly, and Esc/Home returns
Home. OPEN is selected whenever F3 opens or the selected card changes so an
accidental Enter cannot terminate a task. Only the center card uses the full
160x85 thumbnail; adjacent
cards are partially visible to make navigation discoverable. Card allocation
is fixed so labels, badges and thumbnails cannot shift the layout.

## Lifecycle transactions

### Launch a new application

1. appd rejects a second instance and checks launch policy.
2. If ten tasks exist, appd captures the oldest task's latest trusted frame and
   requests a checkpoint with a strict timeout.
3. appd stops the victim unit, persists the sanitized checkpoint result and
   removes the victim task.
4. appd starts a new sandbox with a fresh runtime generation and task ID.
5. After Runtime and compositor identity are bound, appd backgrounds the old
   foreground and commits the new foreground task.
6. Shell receives the new task generation and activates only the matching
   compositor token.

Any failure before step 3 leaves the old task registry intact. A checkpoint
failure is not a transaction failure. A launch failure after eviction keeps
the eviction record and returns to Home; it does not resurrect a partially
stopped process.

### Switch tasks

1. Shell sends `activate-task(task_id)` to appd.
2. appd thaws a frozen task or launches and restores a checkpointed task.
3. appd revokes foreground-only resources from the previous task, updates
   scheduler weights and commits the foreground transition.
4. appd returns the expected runtime generation.
5. Shell activates the compositor token with the same trusted pair.

The compositor stays on the trusted Shell if any identity or generation check
fails. There is never a period where two applications are visible or focused.

### Close and crash

Closing a card is an explicit stop and removes its task record. It does not
delete private application data or a previously durable checkpoint. An
unexpected unit exit changes only the matching runtime generation to `crashed`,
clears session-scoped permissions and releases exclusive resources.

## Checkpoint SDK contract

Checkpointing is opt-in and application-defined. It is not a WASM memory dump.
The next SDK ABI adds conventional exported callbacks for checkpoint and
restore, wrapped by the Rust and C SDKs:

- schema version is an application-owned non-zero `u32`;
- payload is opaque and limited to 8 KiB;
- checkpoint execution has a 250 ms wall timeout and a fuel budget;
- restore runs before the first visible frame and has the same bounds;
- the Runtime validates pointers and copies bytes before returning control;
- appd stores the blob through the authenticated private storage broker under
  a reserved key unavailable to normal SDK storage calls;
- checkpoint metadata includes app ID, package version, schema version, length
  and SHA-256; a version mismatch is offered only when the App explicitly
  declares compatibility.

Applications that do not implement callbacks still participate in multitasking
but resume from a clean launch after capacity eviction or process reclamation.

## Resource policy

Foreground units use the current CPU and memory manifest limits. Background
units receive low CPU weight and lose camera, microphone, GPIO output and other
exclusive leases unless a capability-specific policy explicitly permits
continuation. Network and audio playback may continue when granted. Permission
and document prompts are canceled or deferred when their task leaves the
foreground; they never appear attributed to a different foreground task.

The first implementation uses these pressure stages:

1. keep background tasks resident while memory headroom is healthy;
2. freeze least-recently activated background tasks;
3. checkpoint and stop frozen tasks while retaining their task cards;
4. allow the kernel/systemd unit limit to terminate a task only as a final
   failure, then mark that task `crashed`.

Exact pressure thresholds require device measurements and are deliberately not
hard-coded during simulation development.

## Protocol changes

appd protocol v2 adds `list-tasks`, `activate-task` and `close-task`. Task rows
contain task ID, trusted app metadata, state, creation and activation sequences,
checkpoint availability, runtime generation and thumbnail generation. Existing
`start` remains a Launcher operation: it activates an existing task or creates
one. Existing `stop` closes the task for compatibility.

Private System Shell Wayland protocol v7 adds the compositor-authenticated App
UID event used by the simulation slice. Runtime-authenticated task/generation
binding and sealed thumbnail events remain MT3/MT4 work. Protocol version
negotiation remains strict; image and device releases must update appd, Shell
and compositor as one bundle.

## Simulation implementation status

The `codex/multitasking-main-integration` branch integrates the simulation-first
slice on main without deploying it to a device:

- appd protocol v2 and its C client expose task listing, activation and close;
- `TaskRegistry` enforces one foreground task, a ten-task bound, independent
  FIFO/MRU sequences, generation-safe crash handling and versioned snapshots;
- the server keeps multiple Runtime sessions and no longer destroys the sender
  during an intent transition;
- F3 renders a fixed 160x85 card stack, including trusted RGB565 test frames,
  placeholders and all five lifecycle states;
- Wayland protocol v7 reports the kernel-authenticated client UID. Because this
  release permits only one task per App UID, Shell joins the appd account UID to
  that surface. App-id text is never trusted. A later multi-instance release
  must add a Runtime-authenticated `(task_id, runtime_generation)` binding;
- `TaskJournal` atomically stores validated registry snapshots and a bounded
  64-entry capacity-eviction history. Restart reconciliation retains active
  generations and marks missing units crashed;
- `CheckpointBlob` enforces the 8 KiB, schema, package, SHA-256 and 250 ms
  simulation contract. The SDK exposes an optional lifecycle world and stable C
  export signatures;
- `ThumbnailCache` rejects UID/task/runtime/generation mismatches, limits update
  rate to 2 Hz and proves the ten-frame 272,000-byte budget;
- the resource governor produces deterministic CPU, freeze, checkpoint/stop and
  lease-revocation plans from an abstract pressure level. No CM0 threshold is
  guessed before measurement.
- integration with the latest Setup, Developer Access, Store and power UI keeps
  `struct cp0_ui` below its 64 KiB bound by allocating one fixed 880-byte task
  table; teardown explicitly releases it;
- non-idempotent package install, upgrade, rollback and uninstall reject every
  active logical task, including checkpointed/crashed tasks with no resident
  process; an exact-version Store replay remains idempotent;
- when more than one transient surface exists for the same App UID, Shell
  prefers the most recently announced token. Runtime generation binding is
  still required before production release.

The task journal, checkpoint envelope, thumbnail cache and governor are model
components in this phase. Production wiring still needs the authenticated
Runtime control socket, compositor memfd capture, systemd journal hooks and
measured pressure thresholds. Those are intentionally held behind the device
integration gate rather than partially deployed.

## Delivery roadmap

1. **MT0 architecture and invariants - complete.** Protocols, state ownership,
   limits, FIFO semantics and failure behavior are specified and model-tested.
2. **MT1 appd and F3 simulation - complete.** Multi-session appd, protocol v2,
   UID-bound activation and card UI compile for ARM64.
3. **MT2 persistence and SDK checkpoint model - complete.** Atomic task journal,
   restart reconciliation, bounded blobs and optional SDK lifecycle ABI have
   local tests.
4. **MT3 trusted capture integration - pending device gate.** Add compositor
   capture into sealed read-only memfd objects and connect them to Shell without
   mapping App-owned memory.
5. **MT4 Runtime lifecycle integration - pending device gate.** Add the private
   control socket, WAMR callback fuel/deadline enforcement, atomic checkpoint
   broker namespace and restore-before-first-frame behavior.
6. **MT5 measured governor - pending device gate.** Measure CM0 memory/CPU/SD
   behavior, choose thresholds, tune freeze/checkpoint order and run power-loss
   recovery tests.
7. **MT6 release acceptance - pending authorization.** Deploy one bundle,
   validate F3 with 0/1/3/10 Apps, exercise the 11th-App FIFO path, reboot appd,
   verify checkpoint persistence, then package a new image only if requested.

## Simulation and acceptance

No hardware operation is required for this phase. Local acceptance must cover:

- one foreground task under randomized launch, switch, freeze, checkpoint,
  crash and close sequences;
- ten-task hard limit and strict FIFO eviction on launch 11;
- checkpoint timeout still evicts the victim;
- stale task/runtime/thumbnail generations fail closed;
- MRU visual ordering does not alter FIFO creation ordering;
- F3 pixel snapshots at zero, one, three and ten cards;
- keyboard navigation, activation and close events;
- simulator checkpoint/restore across forced process recreation;
- fixed thumbnail memory and frame-rate budgets.

Device deployment, service restart and image generation remain out of scope
until explicitly authorized.

Developer Mode is not a system-component hot-update channel. It can install and
exercise signed SDK Apps after a compatible multitasking system bundle exists,
but it cannot replace appd, System Shell, compositor policy or Runtime. Current
hardware work therefore remains gated on a coordinated bundle/reboot or a new
image assisted by the device owner.
