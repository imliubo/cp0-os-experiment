# Store Background Status v1

<!-- doc-locale: en -->
> **English** | [简体中文](STORE-BACKGROUND-STATUS-V1.zh-CN.md)

S7C keeps an accepted Store operation visible after the user leaves the Store
page and produces a trusted completion notification. It does not add background
or automatic update policy: every install still begins with an explicit user or
operator request.

## Polling lifecycle

The System Shell reads the local Store Catalog once at startup after loading the
installed-app registry. It then uses two bounded polling rates:

- every second while the Store page is open or a Store operation is active;
- every five seconds while the Store is idle and another page or application is
  foreground.

The faster path follows `queued`, `downloading`, and `installing` operations.
Pause, Cancel, failure, or completion removes the operation from the active
status on the next authoritative Catalog response. A one-tick delay after a
local control request prevents an immediate stale response from replacing the
optimistic accepted state.

Polling uses only the local Unix-domain Store protocol. Search text, application
activity, and progress are not sent to the network. Catalog refresh remains an
explicit Store mutation and is not performed by the background status path.

## Status bar

The 320x170 status bar reserves a fixed area between the screen title and clock.
It displays one stable-width label:

| Authoritative operation | Label |
| --- | --- |
| downloading | `DL 42%` |
| installing | `INSTALL` |
| queued | `QUEUE N` |

When a serial batch contains multiple entries, the activity count includes all
queued, downloading, and installing entries. The representative state uses the
priority downloading, then installing, then queued, so real transfer progress
is not hidden by later queued entries. Paused, canceled, failed, available,
update, and installed applications do not claim an active status.

The label is part of the trusted Shell surface. It remains visible on Home,
Tasks, and over a non-immersive foreground application whose compositor mode
exposes the status bar.

## Completion identity

The UI retains the daemon's raw operation state separately from its derived
`available`, `update`, or `installed` presentation state. A completion is
recorded only when two consecutive verified local Catalog observations contain
the same application ID and version and its raw operation state changes from a
non-installed state to `installed`.

The first successful Catalog read establishes a baseline and never creates a
notification. This prevents an old daemon operation from producing a false
completion after Shell restart. Retaining the raw state also prevents duplicate
notifications if the appd installed-version registry becomes visible one poll
after the Store reports completion.

Multiple applications that complete before the notification can be displayed
are aggregated. One completion names the Catalog application and version;
multiple completions produce one `N UPDATES INSTALLED` notification. The app
name and version come from the strict Store response, while notification title
and body templates are owned by the Shell.

Permission and document prompts, power and settings confirmations, an existing
notification, and system action overlays take precedence. The completion event
remains pending until the trusted overlay can display it. The notification uses
the same four-second banner and compositor notification mode as application
notifications, without granting an application the `notifications.post`
permission.

## Verification

UI behavior tests cover activity priority and count, local optimistic state,
baseline suppression, exact ID/version transition binding, aggregation, taking
an event once, and repeated-poll deduplication. Pixel tests cover progress on the
Store page and on Home at exactly 320x170 while keeping unrelated page hashes
unchanged. The AArch64 compositor build links the final Shell with warnings
treated as errors.
