# CardputerZero OS first-boot provisioning plan

> Status: approved and implemented for host/image acceptance. Fresh-media V0.6
> burn and hardware acceptance are still required before release or deployment.

## Goals

The distributable product image must contain no fixed human username, password,
authorized key or reusable login secret. On first boot, the 320x170 LCD must
guide the device owner through every required choice without requiring HDMI,
SSH, a serial console or a pre-existing network connection.

The flow must:

- create the owner identity and password on the device, never at image-build
  time;
- collect locale, regulatory country, time zone and device name with visible
  explanations and validation;
- establish Ethernet, configure Wi-Fi, or record an explicit offline choice;
- keep remote access off unless the owner explicitly enables it;
- survive power loss at every page and during final application;
- block Home and third-party applications until provisioning is complete;
- fit the single-foreground, keyboard-driven 320x170 System Shell;
- preserve application UID isolation and the immutable-root/data-partition
  design;
- provide a recoverable error path when persistent state is inconsistent.

This plan applies to distributable product images. Development and recovery
artifacts remain explicitly labelled operator tools and must never be presented
as product images. They may accept build-time operator credentials, but no
released product artifact may inherit them.

## Definitions and invariants

- **Human account**: an interactive owner, administrator or maintenance account.
  Fixed `cp0-*` service identities are non-interactive system accounts and are
  not human credentials.
- **Owner**: the locally created human identity. The desktop still starts the
  trusted System Shell directly; the owner does not log in before seeing Home.
  The password authenticates remote access and future privileged user actions.
- **Provisioned**: owner identity, regional settings and one network decision
  have been committed consistently. A live Internet connection is not required.
- **Network decision**: one of Ethernet successfully configured, a Wi-Fi profile
  saved, or the owner explicitly selected `Use offline`.
- **Complete is last**: the `COMPLETE` state is never written until every
  durable object has been written, synced and read back successfully.

Temporary loss of link, an unavailable access point or an unplugged Ethernet
cable after provisioning must not reopen the wizard. They are normal Network
Settings states. The wizard resumes only when no network decision was ever
committed, setup is incomplete, or its durable state is inconsistent.

## Product flow

### Entry and ownership

The System Shell queries the root provisioning service before exposing Home.
When state is absent or incomplete, it enters a trusted Setup mode. In this
mode the compositor:

- permits only the System Shell provisioning surface;
- rejects ordinary application activation and task switching;
- retains global power handling and a controlled emergency shutdown path;
- never draws setup content in an SDK application surface.

The flow resumes at the last committed non-secret step after restart. Back is
allowed before final commit. During commit, navigation and ordinary power
actions are held until the current atomic operation completes or fails into a
recoverable page.

### Pages for 320x170

Each page uses a fixed header, at most four visible rows, one primary action and
one concise help/error area. Long lists scroll without resizing the surface.

1. **Welcome and regional locale**: the first release renders Setup in English
   and selects either the generated `en_US` or `zh_CN` system locale. Full Shell
   localization remains part of the long-translation pixel acceptance gate.
2. **Country and time zone**: regulatory country first, then a bounded time-zone
   list. Country controls legal Wi-Fi channels. Network time is used when
   available; manual clock correction is deferred to post-setup Settings.
3. **Device name**: editable hostname with an example and inline validation.
4. **Owner name**: display name used by trusted system UI.
5. **Username**: interactive account name, availability and reserved-name
   checks.
6. **Password**: masked input, strength/length feedback and a Show toggle.
7. **Confirm password**: exact re-entry; neither value is persisted as setup
   state.
8. **Network**: live Ethernet state and IPv4 address, Wi-Fi adapter state, or
   `Use offline`. A connected link without DHCP is shown as `Waiting for IP`
   and cannot be accepted as configured Ethernet.
9. **Wi-Fi network**: scan results, signal/security indicators and Refresh.
   Hidden SSID entry is deferred until its keyboard and error states have pixel
   coverage; it is not silently treated as an empty scan result.
10. **Wi-Fi credentials**: masked passphrase and connection progress. DHCP is
    the initial supported IP mode; advanced static addressing belongs in
    Settings after initial release.
11. **Remote access**: Off by default; explain the selected mode and local risk.
12. **Review**: show all non-secret choices. Passwords and Wi-Fi secrets are
    represented only as `Configured`.
13. **Applying**: deterministic progress for identity, regional settings,
    network, remote access and verification.
14. **Complete**: hostname, current IPv4 (or Offline), remote-access status and
   a single `Start` action. Current IP remains available from the Network system
   app after Setup.

Keyboard behavior is consistent on every page: Up/Down moves focus, Left/Right
changes a value, Enter activates, Backspace edits, and ESC goes back when safe.
The V0.6 BSP commits an ASMUX-generated Shift press in its own input frame
before the selected character. The Shell then derives held Shift from the
compositor-provided XKB keymap and depressed/latched/locked modifier masks,
with the raw key event retained as a fallback. This keeps unshifted letters
lower-case, held-Shift letters upper-case and the BSP's Sym layer independent.
Password pages disable key-click sound, mask text by default and must never
appear with cleartext in screenshots, crash reports or support bundles.

Low battery may produce a warning before commit, but absent or unsupported
battery telemetry must not deadlock Setup. LCD brightness and keyboard/audio
feedback use conservative defaults until the owner can change them in Settings.

### Validation rules

- Username: `^[a-z_][a-z0-9_-]{0,31}$`; reject `root`, every existing system
  identity, all `cp0-*` names and future reserved prefixes. The selected UID is
  allocated from the controlled owner range `1000-1999`; application UIDs remain
  at `20000` and above.
- Hostname: 1-63 lower-case RFC 1123 characters, no leading/trailing hyphen.
- Password: first-release policy is 10-64 printable ASCII characters, exact
  confirmation and no truncation. This is a local minimum, not a claim of
  password strength.
- Wi-Fi: validate SSID byte length and security-specific credential bounds;
  support open, WPA2 and WPA3 profiles offered by NetworkManager. Unsupported
  enterprise authentication is identified before asking for a password.
- All free text and protocol frames have explicit byte and character bounds.

## Architecture

```text
keyboard -> compositor -> trusted System Shell Setup UI
                              |
                              | bounded Unix SOCK_SEQPACKET protocol
                              v
                      cp0-provisiond (root)
                       |       |       |
                 owner NSS   locale   NetworkManager
                    data      config     keyfiles
                       \       |       /
                        cp0-data persistence
```

### Trusted UI

Provisioning is a mode of the existing System Shell, not an installable SDK
application. It reuses the current RGB565 renderer, keyboard focus rules,
trusted compositor surface and pixel-regression harness. This avoids granting
an application identity authority and avoids another resident UI process on a
512 MB device.

The Shell remains unprivileged. It renders bounded data and sends validated
intent to `cp0-provisiond`; it never edits account, shadow, NetworkManager or
SSH files directly.

### Root provisioning service

`cp0-provisiond` is a root-owned, socket-activated service. Its Unix socket
uses DAC permissions for the `cp0-shell` identity and every connection is also
checked with `SO_PEERCRED`. UID 0, the owner UID and all application UIDs are
rejected as callers; root manages the service through local files and systemd,
not through the Shell protocol.

The service uses bounded `SOCK_SEQPACKET` messages with strict versioned JSON.
The implemented commands are:

- `GetStatus` and `SetRegion` (locale, country, time zone and hostname);
- `SetOwner` and `SetPassword`;
- `ListWifi`, `ConnectWifi`, `UseEthernet` and `UseOffline`;
- `SetSshEnabled` and `Commit`.

Responses never echo password or Wi-Fi secrets. Unknown fields, duplicate
fields, oversized frames, invalid state transitions and calls from the wrong
peer fail closed. After `COMPLETE`, mutating provisioning commands are
permanently unavailable. Normal post-setup changes use the existing Settings
brokers; factory reset remains a separate physical/recovery ceremony.

The daemon owns the setup-only NetworkManager keyfile/connection path;
`cp0-connectivityd` continues to own post-setup radio and airplane toggles.
Both remain separate least-authority services rather than exposing scan secrets
through the ordinary Settings broker.

### Resource bounds

The setup renderer stays inside the existing 32 MiB Shell cgroup. The daemon
receives a 64 MiB memory limit, zero swap and a small task limit, and is allowed
to exit when idle or after completion. The one-shot ceiling includes the daemon
and its yescrypt child in the same cgroup; the previous 16 MiB ceiling could
kill password hashing on CM0. Protocol frames are at most
16 KiB, Wi-Fi results are capped at 64 entries, and only one scan or connection
attempt runs at a time. No downloaded asset, WebView, desktop toolkit or
additional long-running UI process is introduced. Hardware acceptance must
retain measured peak-memory evidence when the candidate image is accepted.

### Human identity without a mutable `/etc`

pi-gen may require a temporary stage account. The build will use a clearly
named account such as `cp0-build`, then remove it, its home, groups, sudo rules
and owned files before export. The mounted-rootfs release gate will reject any
ordinary interactive UID in the final image. Root remains locked and fixed
service accounts retain `nologin` shells.

Runtime owner records are stored on `cp0-data`, separate from immutable OS
service identities. The implemented backend is `libnss-extrausers`, with:

```text
/var/lib/extrausers/passwd
/var/lib/extrausers/shadow
/var/lib/extrausers/group
/var/lib/extrausers/gshadow
```

NSS resolves immutable service accounts from `/etc` and the owner from this
persistent database. Debian 13's standard `pam_unix` authenticates the shadow
record exposed by `libnss-extrausers`; this was verified with a target-container
password authentication test and avoids the unavailable `libpam-extrausers`
package.
Persisting the complete `/etc/passwd`, `/etc/shadow` or `/etc/group` is rejected
because an OS rollback/update could conflict with service accounts.

`cp0-provisiond` allocates exactly one owner UID, creates all four databases
under an exclusive lock, validates every existing record, and commits through
same-filesystem temporary files, `fsync`, rename and parent-directory `fsync`.
Password hashing uses the platform yescrypt/libxcrypt implementation in the
daemon. Cleartext secrets are never passed in argv or environment variables,
written to a temporary file, logged or stored in `state.json`; their buffers
are explicitly cleared after use.

The owner home is stored on `cp0-data` and mounted at `/home/<username>` with
mode `0700`. The first release relies on the existing `cp0-data` capacity rather
than a separate owner-home quota. SSH authorized keys should instead use
the root-controlled `/etc/cardputerzero/authorized_keys/<username>` path so a
compromised owner shell cannot silently rewrite policy-managed keys. The home
is still required if an interactive SSH shell is enabled and must be included
in the documented backup/factory-reset policy.

### Durable state and power-loss recovery

Non-secret state lives at:

```text
/var/lib/cardputerzero/provisioning/state.json
```

It is root-owned, mode `0600`, schema-versioned, strictly parsed and atomically
replaced using temp-write, file `fsync`, rename and directory `fsync`.
Implemented durable states are:

```text
UNPROVISIONED -> OWNER -> PASSWORD_READY -> NETWORK
              -> REMOTE_ACCESS -> REVIEW -> COMMITTING -> COMPLETE
                                                     \-> REPAIR_REQUIRED
```

`SetRegion` performs the `UNPROVISIONED -> OWNER` transition; the protocol's
`REGION` value is reserved for a future independently persisted locale page.
Each transition is idempotent. `PASSWORD_READY` records only that a password
hash was committed, never the secret. At startup the service cross-checks the
state against owner passwd/shadow/group/home records, the completion marker and
the SSH-consent marker. Regional settings are idempotently reapplied at boot;
the explicit network decision remains valid after later link/profile changes.
Missing identity prerequisites or contradictory completion records enter
`REPAIR_REQUIRED`; they must not bypass Setup, create a second owner or erase
data automatically.

Final commit applies and verifies regional settings, the SSH consent marker,
owner group membership and the live SSH service before persisting `COMMITTING`.
It then atomically writes the completion marker and finally persists
`COMPLETE`. Restart with `COMMITTING` but no completion marker returns safely to
`REVIEW`; restart with both final side effects and the marker completes the
transaction automatically. `REVIEW` accepts either pre-commit or idempotently
applied SSH membership so an SSH start failure remains retryable instead of
being misclassified as identity corruption.

If `cp0-data` cannot be mounted safely, the Shell displays a persistent-storage
error with shutdown/retry guidance. It must not create credentials in the
volatile overlay.

### Networking

The country selection is committed before enabling a Wi-Fi connection. Wi-Fi
profiles remain in the existing persistent NetworkManager keyfile path with
mode `0600`, root ownership and no secret exposure to the Shell. Setup supports
scan, refresh, connection retry and an explicit offline choice. The live setup
status distinguishes NetworkManager unavailable, no Wi-Fi adapter, Ethernet
carrier without DHCP, and a usable IPv4 address. Ethernet counts as configured
only after NetworkManager has obtained a usable IPv4 configuration during
Setup. Open, WPA2 and WPA3 networks are selectable; WEP, 802.1X/EAP and other
unsupported security modes remain visible but are rejected before asking for
an incompatible password.

Shell request timeouts are operation-specific rather than a single three-second
deadline: system mutations and status operations use 20 seconds because their
responses include live network probes, scans use 45 seconds, and CM0 password
hashing, Wi-Fi activation and final commit use 75 seconds. NetworkManager itself
is bounded to 30 seconds for scan and 45 seconds for activation. The UI renders
explicit `Securing password`, `Scanning Wi-Fi` and `Connecting Wi-Fi` wait states
before blocking on these bounded calls.

`state.json` records only the network decision type and stable profile ID, not
the SSID password. Network availability after setup is informational. Forgetting
the last Wi-Fi profile later does not destroy the owner identity or reopen the
wizard; Settings can offer a deliberate `Run network setup` action instead.

When network time is unavailable, Setup may accept a manually entered clock and
stores a last-known sane timestamp on `cp0-data`. Later NTP synchronization may
advance or correct wall time, but Store/update security decisions must not treat
an unverified pre-NTP clock as authoritative. V0.6 hardware acceptance must
determine whether a usable RTC exists and document the cold-power behavior.

### Remote access

Remote access is visibly Off by default. The first release should offer:

- **Off**: no SSH listener and no generated host keys;
- **SSH with owner password**: owner only, root login disabled;
- **SSH with public key**: proposed follow-up once a safe on-device import and
  fingerprint-confirmation UI is complete.

An owner selected for SSH is added to a dedicated dynamic `cp0-ssh` group and
sshd uses `AllowGroups cp0-ssh`, `PermitRootLogin no` and mode-specific
authentication. Host keys are generated only after SSH is enabled and are
written to the already persistent `/etc/ssh` bind. Setup completion never opens
a port unless the owner selected it.

The production profile disables SSH initially without permanently masking it.
The implemented owner choice uses this root-only marker:

```text
/var/lib/cardputerzero/provisioning/ssh-enabled
```

sshd and host-key preparation use a systemd condition or generator tied to the
marker. The Shell cannot create it directly. The Complete page shows hostname,
IP and SSH status without displaying a password.

## Security and privacy requirements

- Setup performs no analytics, Store login or Internet call other than the
  network operations selected by the owner.
- Secrets are excluded from journald, kernel command line, process listings,
  environment, support bundles, screenshots and panic output.
- The compositor continues to own all input and trusted surfaces; apps cannot
  display, observe or race Setup.
- All setup files reject links, non-regular files, unexpected owners/modes and
  path substitution.
- Timeouts and retries are bounded. A malicious access point cannot hold the UI
  permanently or cause unbounded result lists and allocations.
- Factory reset must erase owner NSS data, home, Wi-Fi profiles, SSH keys and
  provisioning state together, then return to `UNPROVISIONED`.
- Recovery and OS rollback must understand the provisioning schema and must not
  restore a `COMPLETE` marker without its referenced identity/configuration.

The owner is **not** granted sudo by default. The recommended product model is
that Developer Mode separately and visibly grants a constrained `cp0-admin`
role when it is implemented. Making every owner an unrestricted Linux
administrator would weaken the Android-like application and broker boundary.
This product decision requires explicit approval.

## Verification plan and release gates

Production acceptance is not complete until all of the following pass:

### V0.6 finding: missing persistent service directory

The first production candidate booted the trusted Setup surface on V0.6 but
reported `Provisioning service is unavailable`. Inspection of the exact image
showed that the factory `cp0-data` payload omitted
`/var/lib/cardputerzero/provisioning`, while the daemon's systemd sandbox named
that path as a required `ReadWritePaths` entry. systemd therefore rejected the
service before executing it.

The corrected image creates the root-owned mode `0700` directory both in the
factory payload and through tmpfiles, verifies it in the mounted-rootfs gate,
orders the System Shell after the provisioning socket, and retries transient
socket unavailability without leaving Setup stuck. The daemon also retains the
minimal `CAP_CHOWN` needed to assign the persistent Owner home to UID 1000. A
fresh-media burn is still required to close the hardware finding.

The next V0.6 fresh-media run reached Owner creation but returned
`provisioning state could not be updated`. The provisioning unit combined
`ProtectHome=read-only` with a `ReadWritePaths=/home` exception. systemd keeps
the protected home hierarchy read-only in this combination, so creating the
Owner home failed even though the persistent data bind itself was writable.
The unit now relies on `ProtectSystem=strict` plus the explicit `/home` writable
allowlist and sets `ProtectHome=no`; no other home path is exposed to the
daemon. Source and mounted-image gates reject a regression to the conflicting
policy.

The following fresh-media run failed when Device Name was submitted. Region
setup also applies the hostname, locale, time zone, and wireless regulatory
country. The service had `ProtectHostname=yes`, which conflicts with its
hostname-management responsibility, and an absent wireless PHY made `iw reg
set` fail the entire operation. The unit now uses `ProtectHostname=no`, while
retaining its other sandbox controls. Regulatory configuration is skipped only
when `/sys/class/ieee80211` has no PHY. Failed system commands record the tool,
exit status, fixed operation label, and bounded stderr in the service journal;
the Setup UI receives only the fixed operation label.

The next candidate accepted lower-case and Sym input but held Shift did not
produce upper-case letters, and password confirmation returned
`Provisioning service is unavailable`. The fixed V0.6 BSP implements Shift in
its ASMUX state machine and the Shell previously discarded the compositor's XKB
modifier masks. The Shell now consumes the XKB Shift modifier explicitly.
Password hashing and later network operations also shared a three-second Shell
socket deadline, while yescrypt ran inside an undersized 16 MiB cgroup. The
candidate now uses the bounded operation-specific deadlines and 64 MiB
setup-only daemon ceiling described above. The same audit added live Ethernet
IPv4 reporting, bounded NetworkManager waits, unsupported Wi-Fi security
classification, system-identity collision rejection and SSH-On commit recovery.

The next fresh-media run reached Welcome and accepted lower-case input plus the
Sym `-` character, but held Shift was still lower-case and submitting Device
Name failed with `system locale could not be configured`. The pinned V0.6
keyboard driver emitted its synthetic `KEY_LEFTSHIFT` press and the following
letter in one input synchronization frame; the compositor could therefore
deliver the letter before the client-visible modifier state changed. The image
now carries a narrow patch on top of the pinned BSP that flushes every synthetic
Shift press before a character can follow. Shell-side XKB and raw-event tracking
remain as independent safeguards.

The locale files were present and both supported locales had been generated;
the failing boundary was the restricted broker's `localectl` D-Bus mutation.
The same dependency existed for the later time-zone step. The broker now writes
the validated `/etc/default/locale`, `/etc/timezone`, and `/etc/localtime`
content itself using durable temporary-file, `fsync`, and rename semantics.
Replacing the conventional `/etc/localtime` symlink changes the link itself and
never writes through it into `/usr/share/zoneinfo`. Both the interactive broker
and the completed-state boot applicator receive the matching `/etc` writable
allowlist inside their otherwise read-only system view. Host tests assert the
exact locale/time-zone content, replacement behavior, and preservation of the
former symlink target.

The following candidate reported `provisioning command is not valid in the
current state` after a Device/Owner field was submitted. The Shell advanced
pages from the event it had just sent instead of from the daemon's returned
durable phase. A completed request followed by a lost/late response, service
restart, repeated Enter or prior persisted step could therefore leave the two
state machines on different pages. The Shell now applies the authoritative
status after every successful mutation and performs an immediate `GetStatus`
reconciliation after `InvalidState` or `RepairRequired`. Region, owner and
password transitions are idempotent after completion and cannot rewind a later
phase; plaintext buffers are cleared when the returned phase crosses their
durable boundary. Host tests now restart the daemon at every step across all
six Ethernet/Wi-Fi/offline and SSH On/Off combinations.

To stop using full-media burns as the diagnostic loop, the next image also
includes the physically armed, one-boot ED25519 maintenance path documented in
`MAINTENANCE-HOT-UPDATE.md`. It permits volatile user-space binary replacement
with automatic rollback, but never persists a key, password or update and
cannot update BSP or boot components.

### Host and image tests

- mounted product root contains no human UID, fixed username, password hash,
  authorized key or build-user residue;
- development/recovery artifacts cannot be mislabeled as product artifacts;
- state-machine tests cover every legal and illegal transition, password/SSH
  backend failure, and both sides of the final completion-marker transaction;
- fault injection interrupts every durable write before/after write, `fsync`
  and rename, then proves resume or `REPAIR_REQUIRED` behavior;
- protocol tests and fuzzing cover caller identity, frame bounds, duplicate
  fields, invalid Unicode/bytes and secret redaction;
- account tests prove fixed UID ranges, reserved-name rejection, PAM/NSS login,
  group membership, owner-home isolation and OS rollback compatibility;
- NetworkManager tests cover Ethernet IPv4 readiness, open/WPA2/WPA3 Wi-Fi,
  unsupported enterprise/WEP classification, hidden SSID,
  incorrect password, DHCP failure, offline mode and later link loss;
- SSH tests prove Off has no listener/host keys, enabled modes admit only the
  owner, root and applications are rejected, and disabling removes the listener;
- headless 320x170 pixel tests cover every page, long translations, scrolling,
  focus, errors, password masking and restart-resume rendering;
- `make check`, image gates and `git diff --check` remain clean.

### V0.6 hardware acceptance

- cold boot without HDMI, network or prior credentials reaches Welcome on LCD;
- every key needed by Setup works, including editing, show/hide and Back;
- Ethernet, Wi-Fi and offline paths each complete independently;
- power is removed at every state and repeatedly during commit without creating
  a partial second identity or an unusable device;
- completed setup boots Home directly after 10 cold boots and ordinary network
  loss never reopens Setup;
- selected password authenticates only when SSH password mode is enabled;
- memory, process count, boot time and SD writes remain within existing budgets;
- factory reset and an OS rollback return to consistent states with no secret
  leakage in diagnostics.

No implementation should be deployed to the only V0.6 device until host/image
tests pass and the owner approves the burn/test window.

## Phase boundaries

1. **6I-A, contracts**: schemas, state machine, threat-model tests and UI mock
   renderer; no privileged writes.
2. **6I-B, image identity**: temporary build user removal, extrausers/PAM
   integration, persistent home and mounted-image gates.
3. **6I-C, daemon**: authenticated protocol, atomic state/identity writes and
   secret-handling tests.
4. **6I-D, network and SSH**: shared NetworkManager backend, explicit network
   decision and marker-controlled remote access.
5. **6I-E, System Shell**: trusted Setup mode, all pages, errors, resume and
   compositor activation lockout.
6. **6I-F, resilience**: fuzzing, fault injection, image build and host pixel
   acceptance.
7. **6I-G, hardware**: fresh-media burn and full V0.6 acceptance report.

## Approved decisions

The product owner approved these points on 2026-08-02:

1. Owner has no sudo by default; Developer Mode is a separate constrained
   escalation path.
2. `Use offline` is allowed and counts as a completed network decision.
3. SSH is Off by default; password login is opt-in and public-key setup may ship
   after the initial password/offline flow.
4. Username policy, password minimum and fixed owner UID range above.
5. Persistent owner home is included, while managed authorized keys live under
   `/etc/cardputerzero`.
6. Factory reset is the only supported way to replace the original owner in the
   first release; multi-user accounts are out of scope.
