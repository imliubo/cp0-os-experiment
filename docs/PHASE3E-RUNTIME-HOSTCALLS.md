# Phase 3E: Runtime capability host calls

## WAMR boundary

The first implemented SDK import is:

```c
int32_t cp0_post_notification(
    const uint8_t *title, uint32_t title_length,
    const uint8_t *body, uint32_t body_length);
```

It is registered in WAMR as `(*~*~)i`. WAMR converts both offsets to native
addresses and validates each length against the application's linear memory
before trusted Runtime code receives it. The Runtime additionally caps title
and body byte lengths, rejects ASCII control characters and performs bounded
JSON string encoding. Invalid UTF-8 or character-count overflow is rejected by
the Rust broker protocol parser.

The host call returns SDK error values: success, denied, unavailable (including
a pending user prompt), invalid argument, resource limit or internal failure.
It never exposes the broker file descriptor, application identity or permission
decision API to WASM.

## Runtime syscall envelope

The aarch64 seccomp program now permits `socket` only when argument 0 is
`AF_UNIX`. IPv4, IPv6 and netlink socket creation still returns `EPERM`.
`connect` can reach only paths visible in the empty bubblewrap root, whose sole
socket is `/run/cardputerzero/broker.sock`. Send/receive use the existing I/O
allowlist and one-second socket timeouts prevent a stalled broker from blocking
an application indefinitely.

The outer application unit still restricts address families to Unix and
netlink; netlink is needed only while bubblewrap creates the private network
namespace and is denied by the Runtime's argument-filtered `socket` rule.

## V0.6 validation

The update was hot-deployed without rebooting or flashing.

- Static Runtime SHA-256:
  `24b32b340bad2d49f76ebc9bd07638708245444d036c70777e33dff31008d1bd`.
- Seccomp probe SHA-256:
  `54976a1afb29b61782d39d640753e880c23dc540a77ade7598d2261cda94c9e0`.
- Hello WASM SHA-256:
  `bee4b2daa3232d941f6236e012d8f3e7ff3e1fcec023faa4871472605dc48952`.
- The probe confirmed forbidden syscall checks still passed after allowing
  `AF_UNIX`; an IPv4 socket remained denied while an Unix socket succeeded.
- Hello called `cp0_post_notification` from WASM, remained active and produced
  notification ID 3 in the final run. The Shell control channel received canonical app ID/name,
  title `Hello Card` and body `Runtime host call is active`.
- Hello stopped cleanly and appd, compositor and System Shell remained active.
