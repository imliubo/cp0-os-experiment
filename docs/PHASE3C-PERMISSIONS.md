# Phase 3C: permission decisions and trusted prompts

<!-- doc-locale: en -->
> **English** | [简体中文](PHASE3C-PERMISSIONS.zh-CN.md)

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
cp0ctl permission reset <app-id> <capability>
```

`reset` is restricted to the authenticated control socket. It removes both a
session grant and a stored decision through the same atomic persistence path,
so the next capability use prompts again. It refuses to race a matching
pending prompt.

The production System Shell exposes the same operation without granting a
generic control channel. Open **Apps**, select an application, open its detail
view and switch to the **Permissions** page. Each declared capability is shown
as one of:

- `ASK`: no decision is stored; the next capability request opens the trusted
  prompt;
- `ALLOW`: a session or persistent grant currently applies;
- `DENY`: a persistent denial currently applies;
- `POLICY`: device policy denies the capability and the owner cannot override
  it from the application page.

Pressing Enter on `ALLOW` or `DENY` resets that capability to `ASK`. It never
silently grants a denied capability. The application must request the
capability again, after which the trusted prompt offers `ONCE`, `ALWAYS` and
`DENY`. `ASK` and `POLICY` rows are read-only.

This phase does not expose a generic application permission socket. Capability
requests enter through typed brokers. The notification broker is the first
integration target; hardware and document brokers will reuse the same state
machine without gaining lifecycle or permission-administration authority.
