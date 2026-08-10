# Phase 2F: Trusted notification banners

<!-- doc-locale: en -->
> **English** | [简体中文](PHASE2F-TRUSTED-NOTIFICATIONS.zh-CN.md)

## Presentation contract

Application notifications are queued by `appd`, but only the authenticated
System Shell can present them. The Shell obtains canonical application
identity, display name and bounded notification text over the protected appd
control socket. An application cannot choose the trusted surface, position,
colour or lifetime of a banner.

Private System Shell protocol v4 adds a `notification` overlay mode. It keeps
the trusted Shell surface above the application while exposing only the top
88 pixels. Standard and immersive applications remain visible below the
banner and retain keyboard focus. This differs deliberately from permission
prompts, which switch to the opaque full-screen trusted layer and consume
input.

The banner displays the canonical application name, a required title and up
to two lines of optional body text. It expires after four one-second Shell
timer ticks. Home, Tasks, Power, application withdrawal and permission prompts
cancel the current banner. Permission and power UI also suppress notification
rendering defensively if states ever overlap.

## Bounds and failure behaviour

- The Shell accepts only appd responses with the matching request ID and
  `next-notification` response kind.
- Notification ID, application ID, application name, title and body are parsed
  into fixed-size buffers with no unbounded allocation.
- Empty queues are distinct from malformed responses; malformed data is
  ignored without changing the current UI state.
- The appd list response is capped at eight summaries per frame so its legal
  worst case remains below the 8 KiB control protocol limit.
- Application IDs use the manifest maximum of 128 bytes throughout the
  compositor activation path.

Pure C tests cover valid and empty notification responses, response identity,
empty bodies, state clearing and permission priority. A 320x170 pixel snapshot
locks the trusted banner layout. The compositor profile test locks protocol v4,
the notification enum and the 128-byte application ID limit.

## V0.6 validation

The AArch64 Shell, Weston policy module, appd and `cp0ctl` were hot-deployed to
the V0.6 device without an image flash or reboot. Installed hashes were:

- System Shell: `f4d375c0798818ef3529471dabb80ed0c9d7f0a04da756b075b1a4961bd5cbdc`;
- Weston policy: `37c336e95f6860a4fcfbcb180b892e499dacf529a7c8e5193d8ad96e94ef7420`;
- appd: `2ea26313737eb968cc04871addea63bbe04616d6547a76ab07218b83a8262f4d`;
- cp0ctl: `a7743b43eb2fdab37d1e83d5a592ef79777614c196432475167afbbcb7f080bc`.

Hello Card produced a canonical `notifications.post` prompt. Both `once` and
persisted `always` decisions were exercised. Shell journal records paired each
`notification=<id> visible` event with `expired` exactly four seconds later.
Because the desktop camera caches short-lived frames, a bounded transient test
service repeated the already-authorized application start every five seconds.
4K Camera2 inspection then showed the physical LCD changing from Home to the
trusted `HELLO CARD / HELLO CODEX / NATIVE HOST CALL IS ACTIVE` banner and back
to Home. Pixel-region frame difference rose from camera noise around 2.6 to
11.19 while the banner was present.

The transient test exited on its own, the application catalog returned
`running: false`, and compositor, System Shell and appd all remained active.
Application-focus retention for `notification` mode is compositor-enforced and
covered by the policy tests; the final physical Launcher activation check is
tracked separately in the Phase 2 Roadmap.
