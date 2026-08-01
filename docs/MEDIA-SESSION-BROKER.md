# Media-session broker

The media-session broker completes the global `Fn+Q`, `Fn+W` and `Fn+E`
actions without giving the System Shell or applications general inter-process
communication. It is coordination metadata, not an audio-device capability.
Playing sound still requires the manifest permission `audio.playback`.

## Trust and identity

An application can send only `update-media-session` and
`take-media-action` over its existing Runtime broker socket. Neither request
contains an application ID. appd derives identity from the peer UID, verifies
that the process belongs to the installed application's systemd cgroup and
requires it to be the current Runtime session.

The System Shell sends a targetless `dispatch-media-action` command over the
trusted appd control socket. appd snapshots the sole foreground Runtime and
routes the action only to a media session registered by that exact application.
The success response includes the authoritative installed application ID and
the accepted action. The Shell strictly checks both before displaying `SENT`.

Applications cannot provide titles, artwork, paths, sockets, process IDs or a
target application. The installed manifest remains authoritative for identity
and display name.

## Bounded state

A registration contains only:

- playback state: `inactive`, `paused` or `playing`;
- a three-bit supported-action mask for Play/Pause, Previous and Next.

An inactive session must use an empty mask. A paused or playing session must
support at least one action. Unknown bits and unknown enum values are rejected
at the SDK, Runtime and appd protocol boundaries.

The broker stores one session and at most four pending actions. Unsupported
actions return `UNAVAILABLE`; a full queue returns `BUSY`. Taking an action
consumes it once. Registration replacement drops queued actions, and reducing
the supported mask removes queued actions that are no longer valid.

## Lifecycle

Media state is cleared on explicit stop, unexpected Runtime exit, application
replacement, successful uninstall and inactive registration. Lifecycle code
clears broker state independently of Runtime locking, so a stale application
cannot receive an action intended for a new foreground process.

The public SDK exposes:

- C/C++: `cp0_media_session_update` and `cp0_media_take_action`;
- Rust: `media::update_session` and `media::take_action`;
- WIT: the imported `media` interface.

There is no polling thread in appd. An application chooses when to call
`take-action`, normally from its bounded event loop. This keeps idle CPU and
memory use suitable for the 512 MB CM0 target.

## Device acceptance

Local tests cover strict targetless frames, caller isolation, supported-action
filtering, the four-entry queue, lifecycle clearing, generated SDK bindings and
the Shell `SENT`, `UNAVAILABLE`, `BUSY` and `FAILED` overlays. Physical
acceptance still requires a media-capable test application and the real
`Fn+Q/W/E` key combinations on V0.6 hardware.
