# Phase 3M: Isolated Intent Broker

Applications do not receive arbitrary inter-process sockets or target paths.
Inter-application handoff is routed by appd using bounded, manifest-declared
actions and the already authenticated Runtime broker connection.

## Contract

An application manifest may export up to eight intent actions. Each action is a
lowercase reverse-domain name containing at most 96 ASCII bytes. The sender
provides only an action and up to 1024 payload bytes; it cannot select an
application ID, socket or process.

appd scans root-owned manifests for the requested action. No matching receiver
returns `not-found`, while more than one receiver returns `ambiguous`; appd
never silently selects one. The global in-memory queue contains at most eight
messages. A queued message is bound to the resolved application ID and can be
removed only once through that application's authenticated `take` call.

Intent receipt is an exported application entry point rather than a user
permission. Both send and take still require the caller's installed UID,
running PID, exact systemd cgroup and current root-owned manifest to match.

## Single-foreground transition

The ordering is deliberately fixed:

1. appd validates the sender, action, unique receiver and queue capacity;
2. appd queues the target-bound message;
3. appd writes `intent-accepted` to the sender's connected broker socket;
4. if that write fails, appd cancels the exact queued message;
5. only after a successful write does appd stop the sender and start the
   receiver;
6. the new receiver instance calls `take`, which consumes the message once.

This preserves the single-running-application invariant. If receiver startup
fails after acknowledgement, the bounded message remains queued so a later
trusted Launcher start can recover it; appd records the transition failure
without starting another application.

## SDK and verification

Rust exposes `intents::send` and `intents::take`. The freestanding C/C++ header
and WIT contract expose the same action/payload model. Runtime host calls repeat
all linear-memory, action and payload bounds before using the sole broker Unix
socket.

Hello Card exports `dev.cardputerzero.hello.open`. Pressing `I` sends that
action to itself; appd acknowledges and restarts it, and the new instance takes
the `restart` payload. Host tests cover manifest validation, canonical broker
frames, ambiguous routing, queue exhaustion, cancellation and one-time
target-bound delivery. A physical key-driven transition remains deferred until
the active 24-hour stability run finishes.
