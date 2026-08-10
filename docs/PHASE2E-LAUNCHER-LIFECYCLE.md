# Phase 2E: Installed application Launcher

<!-- doc-locale: en -->
> **English** | [简体中文](PHASE2E-LAUNCHER-LIFECYCLE.zh-CN.md)

## Lifecycle model

The System Shell no longer treats a mapped Wayland surface as an installed
application. It polls appd's authenticated control socket for canonical
manifest metadata and keeps that catalog independently from ephemeral
compositor tokens.

The private application summary now includes:

- application ID, display name and version from the installed manifest;
- standard or immersive display policy;
- current appd/systemd running state.

Responses stay below the 8 KiB protocol frame by requesting eight entries per
page. The Shell accepts at most 32 entries, shows a `32+` marker if more exist,
and scrolls four visible rows over the 320x170 display. This is a Launcher UI
limit, not an authority boundary; appd remains the source of truth.

Selecting a stopped application sends an appd start command, marks the row as
STARTING and records the canonical application ID as pending activation. The
trusted App Runtime later maps its surface through the compositor. The Shell
matches the compositor event to that ID, applies the manifest display mode and
activates the opaque surface token. A token can never be supplied by WASM.

Home hides the application but does not terminate it. Tasks shows the active
task cards with explicit OPEN and STOP actions; OPEN is the safe default and
Space is a direct STOP shortcut. Apps exposes the same direct shortcut for a
selected RUNNING or STARTING application, while its Actions detail page uses a
state-specific OPEN APP or STOP APP command. STOP is sent to appd, which clears
the application's session permissions and terminates its transient systemd
unit. When the compositor withdraws the surface token, the installed Launcher
row is retained and returns to READY.

## Defensive client

The Shell client validates the protocol version, request ID, response kind,
application ID grammar, display mode, booleans, pagination progress and all
bounded string copies. Catalog calls use a 500 ms socket timeout; lifecycle
calls allow 3 seconds for systemd. Every socket receives `FD_CLOEXEC` before it
connects.

Pure C tests cover pagination, metadata decoding, response identity, invalid
display modes, wrong request IDs, undersized output arrays and hostile
application IDs. UI tests cover installed/stopped state, starting, surface
binding, token removal without catalog removal, failed launch state, Tasks
actions and scrolling beyond four entries. Apps and Tasks have 320x170 pixel
regression snapshots.

## V0.6 verification

The updated AArch64 appd, cp0ctl and System Shell were hot-deployed without a
reboot or image flash. Device checks confirmed that appd returned `Hello Card`,
`standard` and stopped/running lifecycle state from the trusted manifest. An
external start produced a compositor surface token, the Shell displayed the
canonical notification permission prompt, one-time authorization completed
the WASM host call, and stop returned the catalog to stopped while retaining
the installed entry. Camera inspection confirmed the prompt and final Home
screen. Compositor, Shell and appd remained active.

The device intentionally has no remote input injection interface. The final
physical acceptance of Apps -> Hello Card -> Enter and Tasks -> STOP therefore
remains a short operator check; all underlying state transitions and the real
appd/surface path have already been exercised independently.
