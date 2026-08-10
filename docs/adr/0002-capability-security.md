# ADR 0002: Capability Permissions and Dual Sandboxes

<!-- doc-locale: en -->
> **English** | [简体中文](0002-capability-security.zh-CN.md)

- Status: Accepted
- Date: 2026-07-30

## Decision

Applications have no network, shared-file, media-input, or hardware permissions
by default. They may request only capabilities declared in their manifest, and
only through SDK hostcalls. Outside the WASM runtime, Linux UID, namespaces,
seccomp, cgroups, and a minimal mount view constrain each host process.

The system identifies callers from a verified package ID and process
credentials; it does not trust an identity string supplied by an application.
Only hardware brokers can access device nodes. Shared files are passed only as
file descriptors through the Document Portal.

## Rationale

Containers alone retain too large a Linux syscall surface. WASM alone exposes
runtime vulnerabilities directly to the system. Two independent boundaries
reduce the chance that one implementation defect causes a complete breach and
centralize privilege enforcement in a small set of trusted services.

## Consequences

Every hardware feature must begin with a broker API design. Permission API
changes are SDK compatibility changes and require versioning, auditing, and
malicious-call tests.
