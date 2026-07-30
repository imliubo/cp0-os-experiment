# Phase 3F: Restricted HTTPS client broker

## Scope

`network.client` provides one synchronous, bounded HTTPS GET operation. It is
not a raw socket API and does not expose DNS, TCP, TLS configuration, request
headers, streaming bodies or listener sockets to applications.

The first ABI is intentionally narrow:

- URL: HTTPS only, at most 1024 UTF-8 bytes, no control characters;
- redirect limit: two, with HTTPS validation and DNS policy applied again;
- global timeout: five seconds including DNS, redirects and response reading;
- response body: at most 2048 bytes, returned as opaque bytes;
- response metadata: HTTP status code only;
- broker and network-service frames: newline-delimited strict JSON, 4096 bytes.

The Rust and C/C++ SDKs use a caller-owned response buffer. The Runtime host
call returns the status and body length in one packed 64-bit value, so WAMR can
validate both pointer/length pairs before trusted native code runs.

## Trust boundary

The application Runtime remains restricted to `AF_UNIX`. It sends `http-get`
to the existing appd capability broker. appd obtains the application identity
from `SO_PEERCRED`, verifies that the peer PID is in the active application's
systemd cgroup, loads the root-owned manifest and evaluates
`network.client`. The application never supplies its own identity.

After authorization, appd releases its shared lifecycle/permission mutex and
forwards the request over a root-only Unix socket. Slow network operations
therefore cannot block Launcher list/start/stop, trusted permission UI or
notification retrieval.

Only `cp0-networkd` has `AF_INET` and `AF_INET6`. It runs as the unprivileged
`cp0-network` account with no capabilities, no device access, a 24 MiB cgroup
limit and eight-task limit. Its activated socket accepts UID 0 only, matching
the current root appd service.

## SSRF and rebinding policy

`cp0-networkd` disables all environment proxy discovery and installs a custom
resolver in the HTTPS transport. Every connection target, including every
redirect target, is reduced to the resolver's approved socket-address list.
The connector never receives rejected addresses.

The policy rejects IPv4 and IPv6 loopback, private/unique-local, link-local,
multicast, unspecified, reserved, documentation and benchmarking ranges. It
also rejects IPv4-mapped IPv6, site-local IPv6, Teredo, well-known/local-use
NAT64 prefixes and other IPv4-compatible IPv6 forms. A hostname that returns a
mix of public and blocked addresses can use only its public results. A hostname
that returns no public address fails with `blocked-address`.

TLS uses Rustls with WebPKI roots. Certificate validation cannot be disabled.
HTTP redirects cannot downgrade to plaintext because the agent is configured
with `https_only` for the complete redirect chain.

## Stable SDK behavior

The SDK maps capability denial and blocked destinations to `Denied`, pending
permission or transient DNS/TCP/TLS/timeout failures to `Unavailable`, invalid
URLs to `InvalidArgument`, and oversized responses to `ResourceLimit`.
Successful calls return `{ status_code, body_length }`; the body occupies the
prefix of the caller's buffer and may contain arbitrary bytes.

Hello Card declares both `notifications.post` and `network.client`. Pressing
the Linux `KEY_N` key requests `https://example.com/` only after the user
action. Green, yellow, red and magenta indicators represent a 2xx response,
pending/unavailable, denied/non-2xx and internal failure respectively.

## Verification

Local coverage includes:

- strict request/response parsing, canonical base64 and maximum-frame tests;
- public-address policy tests for IPv4, IPv6, mapped and transition ranges;
- literal loopback and plaintext HTTP rejection before a connection;
- an opt-in live public DNS, Rustls certificate and HTTPS response test;
- network-service success/error dispatch with sanitized messages;
- appd-to-networkd Unix protocol exchange and maximum broker response size;
- C Runtime binary-response decoding and stable error mapping;
- Rust and freestanding C11/C++17 SDK compilation tests;
- native workspace tests and AArch64 appd/networkd/Runtime builds.

The 24-hour Phase 2G stability acceptance was already active when this phase
was implemented. No core service was replaced during that run. Device HTTPS,
permission prompt, private-address rejection and cgroup memory measurements
must be recorded after the monitor completes.
