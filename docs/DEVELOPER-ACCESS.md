# Developer access

## Product contract

A personally owned production device may run developer applications without
becoming a general-purpose Linux development machine. The owner controls two
independent settings:

| Setting | Default | Authority granted |
| --- | --- | --- |
| Developer Mode | Off | Pair, install, inspect logs, start, stop and uninstall signed SDK applications |
| Owner SSH Shell | Off | Interactive shell as the owner account; still no root or sudo |

Developer Mode does not enable root login, sudo, arbitrary remote commands,
native applications or unsigned packages. Owner SSH Shell is not required for
the normal SDK workflow and is never enabled as a side effect of Developer
Mode.

Recovery and unrestricted maintenance remain a separate removable recovery
image ceremony. A managed parent or organization policy may lock Developer
Mode off. The personal production policy permits the owner to change it.

## On-device workflow

1. Complete first-boot Setup and keep **Owner SSH Shell** Off unless a full
   shell is deliberately required.
2. Open **Settings > Security > Developer Mode** and confirm **Enable**.
3. Select **Pair New Computer**. The device accepts new pairing registrations
   for ten minutes.
4. Use **Paired Computers** to review up to eight host labels. Press Enter on a
   host to revoke it, or select **Revoke All**.
5. Turn Developer Mode Off after testing. This closes the constrained remote
   channel and causes sshd to stop unless Owner SSH Shell is independently On.

The pairing window is volatile, root-owned and bounded to ten minutes. Its
expiry uses Linux `CLOCK_BOOTTIME`, so suspend time counts and wall-clock or NTP
adjustments cannot extend it. Clients receive only the daemon-computed remaining
duration and do not decide whether the window is open. An open window does not
override Developer Mode. Disabling Developer Mode immediately blocks pairing
and all application mutations even if the window has not yet expired.

## Developer workstation workflow

Generate a developer signing key and an Ed25519 SSH key once. After opening the
device pairing window, register both keys using the owner name and device IP:

```sh
cp0ctl key generate developer.key developer.pub
ssh-keygen -t ed25519 -f ~/.ssh/cardputerzero_ed25519

cp0ctl pair developer.pub ~/.ssh/cardputerzero_ed25519.pub workstation \
  --device OWNER@DEVICE_IP
```

The first pairing uses the owner's password through standard SSH
authentication. Add the paired SSH key to the workstation's SSH agent or host
configuration, then use the constrained commands:

```sh
cp0ctl install app.developer.capp --device OWNER@DEVICE_IP
cp0ctl logs dev.example.app 100 --device OWNER@DEVICE_IP
cp0ctl app start dev.example.app --device OWNER@DEVICE_IP
cp0ctl app stop dev.example.app --device OWNER@DEVICE_IP
cp0ctl app uninstall dev.example.app --device OWNER@DEVICE_IP
cp0ctl device remote-status --device OWNER@DEVICE_IP
```

There is no ADB service. `cp0ctl` streams a bounded protocol through
`ssh -T ... cp0-dev`; it does not upload to a general temporary directory and
does not invoke `scp`, sudo or a remote shell.

## Enforcement boundary

`sshd` accepts only the provisioned owner and never root. The owner login shell
is `/usr/libexec/cardputerzero/owner-shell`:

- a password-authenticated `cp0-dev` command is routed to `cp0ctl dev-session`
  and `cp0-devd` checks Developer Mode before every privileged request;
- every paired SSH key is written by root in an owner-readable file with
  `restrict,command="/usr/bin/cp0ctl dev-session"`;
- an interactive or arbitrary command reaches Bash only when the independent
  `cp0-ssh` login group is present;
- a paired forced-command key remains constrained even when Owner SSH Shell is
  On. SSH forwarding, including Unix socket forwarding, is disabled globally.

`cp0-devd` authenticates every Unix connection using `SO_PEERCRED`. UID 1000
may use only the remote developer operations. The trusted `cp0-shell` UID may
use only local pairing-window and revocation operations. Each request reloads
the root-owned device policy and Developer Mode marker.

Installation requires all of the following:

- Developer Mode is currently On;
- the `.capp` is structurally valid and has a valid developer signature;
- the signing key is referenced by the root-owned paired-host registry;
- the matching trust file contains the exact 32-byte key;
- appd independently accepts the package through its existing root control
  path.

Revocation rewrites the owner authorized-key file from the paired-host registry.
The developer trust key is removed when no remaining paired host references it.
At daemon startup the authorized-key file is reconciled from the strict,
root-owned registry.

## Remaining device acceptance

Before release, V0.6 hardware acceptance must verify password pairing, paired
key reuse, ten-minute expiry, individual and bulk revocation, Developer Mode
Off denial, independent Owner SSH Shell behavior, normal reboot persistence and
recovery-image masking. The login-shell `$2`/`SSH_ORIGINAL_COMMAND` behavior
must be observed under the image's real OpenSSH build, not inferred only from a
shell unit test.
