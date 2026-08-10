# Phase 5A: trusted installation, upgrades and rollback

<!-- doc-locale: en -->
> **English** | [简体中文](PHASE5A-ATOMIC-INSTALL.zh-CN.md)

## Device trust policy

Production installation requires both a valid developer signature and a valid
store signature. Store public keys are raw 32-byte Ed25519 keys installed as:

```text
/etc/cardputerzero/trust/store/<sha256-of-key>.pub
```

`image/build-image.sh` accepts an optional `CP0_STORE_PUBLIC_KEY` path, validates
its exact length and installs only the public key. Signing secrets never enter
the image build context.

Revoking either the developer identity or the store identity rejects the whole
package. A root-owned marker named by key ID is placed below:

```text
/etc/cardputerzero/trust/revoked/<sha256-of-key>
```

A package without a store signature is rejected unless all of these conditions
hold:

- `/etc/cardputerzero/developer-mode` is a secure regular file containing
  exactly `enabled\n`;
- the embedded developer key has a matching root-owned file below
  `/etc/cardputerzero/trust/developers`;
- neither key ID is revoked.

The trust directories and files must not be links or writable by group/other
users. Production checks also require UID 0 ownership.

## Transaction model

`cp0ctl install` validates the container locally, copies it with `0600` mode to
the root-only `/run/cardputerzero-appd` inbox and sends only the generated base
name over the lifecycle socket. appd rejects separators, `..`, hidden names,
oversized names and non-`.capp` files. Shell is still trusted for launch and UI
operations, but install and rollback commands additionally require peer UID 0.

appd performs installation in this order:

1. validate source type, ownership/mode, package bounds and canonical encoding;
2. verify developer revocation/signature and store or developer-mode trust;
3. validate manifest, device SDK compatibility and packaged entrypoint;
4. write every regular file with create-new semantics into a private staging
   directory and `fsync` it;
5. rename the complete version directory into place on the same filesystem and
   `fsync` its parent;
6. verify the preallocated stable Unix account and atomically replace the
   root-owned application registry.

A power failure before step 5 leaves no visible version. A failure between
steps 5 and 6 leaves an inactive orphan; retrying the identical signed package
recognizes its exact files and completes registry activation. Existing content
for the same ID/version is never overwritten. The registry itself is updated
through a separately synced temporary file and directory rename.

## Stable identities and rollback

The image preallocates 64 locked application accounts, `cp0-app-20000` through
`cp0-app-20063`. appd cannot modify `/etc/passwd`, and a registry identity is
never recycled for a different application ID. This bounds installed
applications while keeping account memory and disk overhead negligible.

An upgrade retains the two most recent versions in registry history and leaves
their immutable package directories in place. `cp0ctl app rollback <app-id>`
requires the application to be stopped, verifies the previous manifest and
atomically switches the active registry version. The former current version
becomes the next rollback target, allowing a controlled roll-forward.

Automated tests cover signature trust, explicit developer mode, revocation,
same-version conflicts, incompatible SDK versions, repeat-after-power-loss
recovery, history bounds and stable-identity rollback.
