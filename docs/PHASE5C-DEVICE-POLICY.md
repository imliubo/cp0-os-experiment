# Phase 5C: device policy and user-controlled modes

## Policy boundary

`/etc/cardputerzero/device-policy.json` is the root-owned upper bound for
device behavior. Applications, the System Shell and `cp0-stored` cannot write
this file. appd loads and strictly validates at most 16 KiB at startup, rejects
unknown fields, and requires sorted unique application and permission lists.

The policy authority is `personal`, `parent` or `organization`. The authority
is a user-facing ownership label, not an authentication credential. Parent and
organization provisioning is currently local and root-mediated; remote fleet
management and enrollment are not implemented.

Policy v1 can:

- lock developer mode and recovery mode off;
- disable all Store installation;
- change application launch from `allow-all` to an exact allowlist;
- globally deny any SDK capability before a user permission prompt is shown.

An allowlist also blocks Store installation for an application not present in
the list. A global capability denial overrides a previous per-application
`allow-always` decision. Existing installed bytes and application data are not
deleted when policy changes.

When appd loads a policy that locks either mode off, it removes any stale mode
marker before accepting requests. Disabling a mode remains permitted even when
the mode is locked; enabling it does not.

After provisioning a policy, restart appd to apply launch, Store and capability
rules:

```sh
sudo install -o root -g root -m 0644 device-policy.json \
  /etc/cardputerzero/device-policy.json
sudo systemctl restart cardputerzero-appd.service
```

## User controls

Home's fifth entry opens the 320x170 Settings screen. It shows Developer Mode,
Recovery Boot, the active authority and whether Store, application launch or
capabilities are restricted. A locked mode cannot be enabled. Enabling either
mode requires a second confirmation whose default selection is Cancel;
disabling is immediate.

The same bounded appd protocol is available from the recovery console:

```sh
sudo cp0ctl device status
sudo cp0ctl device developer on
sudo cp0ctl device developer off
sudo cp0ctl device recovery on
sudo cp0ctl device recovery off
```

Developer mode is not an unsigned-code switch. A developer package still needs
a valid developer signature and a matching root-provisioned public key below
`/etc/cardputerzero/trust/developers`. appd checks both the current policy and
the persistent developer-mode marker for every developer installation.

## Recovery boot

Recovery Boot creates the root-owned persistent marker
`/var/lib/cardputerzero/registry/recovery-mode`. On the next and subsequent
boots, the compositor refuses to start and
`cardputerzero-recovery-console.service` explicitly activates `getty@tty1`.
The LCD therefore presents the local Linux login console and the keyboard can
enter commands.

Recovery remains enabled until explicitly disabled. To return to the System
Shell:

```sh
sudo cp0ctl device recovery off
sudo systemctl reboot
```

The mode markers contain one exact value, are created with mode `0600`, synced,
and atomically renamed inside the persistent root-owned registry. appd rejects
symbolic links, writable files and replacement races when reading them.

## Enforcement points

```text
root device-policy.json
        |
        +-- Settings mode locks -> appd atomic markers
        +-- developer install -> policy + marker + developer signature/key
        +-- StoreInstall -> Store switch + application allowlist
        +-- Start -> application allowlist
        +-- capability request -> global deny before user decision
        +-- next boot -> compositor gate + tty1 recovery service
```

The Store UID is authorized only for the catalog-bound `StoreInstall` command.
It cannot read or change settings, start applications, inspect logs or use the
root developer installation path.

Automated coverage includes bounded/strict policy decoding, atomic mode state,
locked modes, allowlist and capability decisions, developer installation
gating, Store UID command isolation, strict Shell response parsing, Settings
navigation and 320x170 screenshot regression. Recovery boot itself still needs
real-device acceptance after the new image or units are deployed.

Phase 5C was hot-deployed to V0.6 on 2026-07-31. Device-side acceptance
confirmed the default personal settings, developer mode on/off, recovery marker
on/off without reboot, exact marker cleanup, Store UID denial, and active
compositor/Shell/appd services with zero post-deployment restarts. Weston also
confirmed the 320x170 output and physical `tca8418c` keyboard. Recovery mode was
left off before starting the replacement 24-hour stability run. The next-boot
tty1 selection remains intentionally untested until that run completes or a
new image is flashed.
