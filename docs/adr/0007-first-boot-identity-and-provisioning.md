# ADR 0007: first-boot identity and provisioning

- Status: proposed; awaiting product approval
- Date: 2026-08-02

## Context

The current development image requires a build-time username/password, while
the production access profile locks a fixed `pi` account and disables all login
paths. The latter removes a reusable password but does not satisfy the product
requirement that a distributable image contain no fixed human username and let
its owner configure identity and networking entirely on the 320x170 LCD.

The lower root is immutable and only selected paths on `cp0-data` persist. A
first-boot implementation therefore cannot safely mutate only `/etc/passwd` in
the OverlayFS upper layer. Setup also handles passwords and Wi-Fi credentials,
so it cannot be an untrusted SDK application or receive root authority.

## Proposed decision

1. A product image contains no human account. pi-gen may use a temporary
   `cp0-build` staging identity, but export removes the account, home, group and
   all owned residue. Mounted-image gates enforce the absence of interactive
   ordinary UIDs, password hashes and authorized keys.
2. The owner is created locally and stored in a dedicated extrausers NSS/PAM
   database on `cp0-data`. Immutable `/etc` continues to own only root and
   non-interactive system/service identities. The owner range is `1000-1999`;
   application UIDs remain `20000+`.
3. The existing trusted System Shell owns Setup UI. While setup is incomplete,
   the compositor prevents Home, Tasks and ordinary applications from becoming
   active.
4. A new root-only `cp0-provisiond` owns account, regional, NetworkManager and
   SSH mutations. It accepts a bounded versioned Unix protocol only from the
   exact `cp0-shell` peer UID and never returns a secret.
5. Durable, non-secret provisioning state is atomically stored on `cp0-data`.
   Every step is idempotent and `COMPLETE` is written last. Cross-object
   disagreement enters `REPAIR_REQUIRED`; it never silently bypasses setup,
   erases data or creates another owner.
6. Network completion means successful Ethernet configuration, a saved Wi-Fi
   profile or an explicit offline choice. Current link state does not control
   future entry to Setup.
7. SSH is off by default and is marker-controlled rather than permanently
   masked. Enabling it is an explicit Setup/Settings decision, root login stays
   disabled, and only the dynamic owner group may authenticate.
8. The owner receives no sudo by default. A later Developer Mode may grant a
   separately audited constrained administration role.

The detailed UI, storage, secret-handling and test contract is in
`docs/FIRST-BOOT-PROVISIONING.md`.

## Consequences

- Product images no longer have a universal build-time login path. Recovery
  remains a separately labelled physical-maintenance artifact.
- The image gains extrausers/PAM integration, a small privileged daemon and a
  persistent owner home/identity schema that OS rollback must preserve.
- System Shell becomes responsible for an additional trusted state, but does
  not gain root privileges or direct access to credential files.
- Network loss after setup is a normal Settings condition and cannot trap the
  owner in a boot loop.
- Factory reset must erase identity, home, network profiles, SSH material and
  completion state as one ceremony.
- Public-key onboarding needs a separately designed import and fingerprint UI;
  it is not approximated with invisible boot-partition magic.

## Rejected alternatives

- **Ship a locked fixed user**: removes password access but still embeds a human
  identity and cannot support owner-selected credentials.
- **Persist all of `/etc` or account files**: couples mutable data to service
  accounts and makes A/B updates and rollback conflict-prone.
- **Run `useradd` into OverlayFS only**: loses or splits identity when the
  overlay/data state changes and cannot provide an update-stable contract.
- **Make Setup an SDK application**: gives untrusted application machinery
  identity/network authority and permits UI/focus ambiguity.
- **Run the whole Shell as root**: unnecessarily expands the trusted attack
  surface of rendering, input and metadata parsing.
- **Reopen Setup whenever offline**: makes temporary network failure a device
  availability failure and confuses configuration with connectivity.
- **Enable SSH or sudo automatically**: creates an undocumented remote or local
  privilege path contrary to the product security model.

## Approval and enablement gates

This ADR is not accepted and changes no implementation until the product owner
approves the decisions recorded in the detailed plan. Enablement then requires
mounted-image credential gates, protocol/secret tests, state fault injection,
all Setup pixel tests and fresh-media V0.6 acceptance before any production
claim.

