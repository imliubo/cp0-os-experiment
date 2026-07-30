# Phase 3C: permission decisions and trusted prompts

## Security model

Applications declare capabilities and a user-facing reason in the validated
manifest. A runtime or broker request never supplies an application ID as an
authority claim. The broker will identify its caller from a trusted runtime
channel and resolve the canonical manifest through `appd`'s root-owned install
registry.

The permission database is
`/var/lib/cardputerzero/registry/permissions.json`. It is strict JSON with a
schema version, stable application IDs and per-capability decisions. `appd`
rejects symbolic links, non-regular files, non-root ownership and files writable
by group or other. Persistent changes use a mode `0600` temporary file, `fsync`,
atomic rename and parent-directory `fsync`.

An undeclared capability is always rejected. A declared capability without a
stored decision creates a trusted prompt. Decisions have these semantics:

- `allow-once` lives only in `appd` memory and is cleared when the app stops;
- `allow-always` is persisted;
- `deny` is persisted and removes any session grant.

## Prompt state machine

Only one prompt can be pending because the OS has one foreground application.
Repeated requests for the same application and capability return the same
prompt. A different request is rejected as busy until the current prompt is
resolved or the application stops. Prompt IDs are non-zero and stale IDs cannot
modify a decision.

Prompt display metadata comes exclusively from the canonical installed
manifest: prompt ID, application ID, application name, capability and declared
reason. This prevents untrusted request payloads from impersonating another app
or changing the text displayed by the System Shell.

## Shell control contract

The authenticated appd protocol adds:

- `get-permission-prompt` to retrieve the pending prompt, if any;
- `resolve-permission` with a prompt ID and `allow-once`, `allow-always` or
  `deny`.

The existing socket DAC and `SO_PEERCRED` checks restrict both commands to root
and `cp0-shell`. Diagnostic equivalents are:

```sh
cp0ctl permission pending
cp0ctl permission resolve <prompt-id> once|always|deny
```

This phase does not expose a generic application permission socket. Capability
requests enter through typed brokers. The notification broker is the first
integration target; hardware and document brokers will reuse the same state
machine without gaining lifecycle or permission-administration authority.
