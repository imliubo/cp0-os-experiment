# Developer Mode Deployment

<!-- doc-locale: en -->
> **English** | [简体中文](developer-mode.zh-CN.md)

Read this before changing a physical device. Developer Mode is an
owner-controlled, constrained deployment channel for signed SDK Apps. It is
not an App permission, root access, sudo, a native package channel or a general
SSH shell.

## Prepare the workstation

Use one matched DevKit and pass its doctor first. Generate two independent key
pairs once:

```sh
cp0ctl key generate /secure/developer.key ./developer.pub
ssh-keygen -t ed25519 -f ~/.ssh/cardputerzero_ed25519
```

The raw 32-byte `developer.pub` verifies `.capp` signatures. The Ed25519 SSH
key authenticates the workstation to the forced `cp0-dev` command. Keep both
private keys off the device, outside the App project and outside source
control.

## Authorize on the device

1. Complete first-boot Setup and obtain the owner-selected username and current
   IP from trusted device UI.
2. Open **Settings > Security > Developer Mode** and confirm **Enable**.
3. Select **Pair New Computer**. The root-owned window lasts ten minutes.
4. From the workstation, run:

```sh
cp0ctl pair ./developer.pub ~/.ssh/cardputerzero_ed25519.pub workstation \
  --device OWNER@DEVICE_IP
```

The first pairing uses the owner's password through standard SSH
authentication. The device records the developer public key, SSH public key
and host label together. Configure that private SSH key in `ssh-agent` or the
workstation's SSH host entry for later commands. Never put the owner password
or a private key in an App, script, Skill or command-line argument.

Before sending the owner password, verify the device SSH host-key fingerprint
through a trusted device/operator channel. The current product UI does not yet
display that fingerprint. On an untrusted network, stop and report this release
gap instead of accepting an unknown key. Do not disable host-key checking.

For subsequent commands, either load the dedicated key with `ssh-add` or use a
narrow SSH configuration entry:

```text
Host cardputerzero-dev
    HostName DEVICE_IP
    User OWNER
    IdentityFile ~/.ssh/cardputerzero_ed25519
    IdentitiesOnly yes
```

With that entry, use `--device cardputerzero-dev`. Do not add wildcard host
rules or relax host-key validation.

The owner can review up to eight entries under **Paired Computers**, revoke one
with Enter, or use **Revoke All**. Pair every new workstation separately; do
not copy an old workstation's SSH private key.

## Build and deploy

Sign with the exact developer key registered during pairing:

```sh
cp0ctl package ./my-app ./my-app.unsigned.capp
cp0ctl sign developer ./my-app.unsigned.capp ./my-app.developer.capp \
  /secure/developer.key
cp0ctl verify ./my-app.developer.capp
cp0ctl install ./my-app.developer.capp --device OWNER@DEVICE_IP
cp0ctl logs dev.example.my-app 100 --device OWNER@DEVICE_IP
cp0ctl app start dev.example.my-app --device OWNER@DEVICE_IP
cp0ctl app stop dev.example.my-app --device OWNER@DEVICE_IP
cp0ctl app uninstall dev.example.my-app --device OWNER@DEVICE_IP
cp0ctl device remote-status --device OWNER@DEVICE_IP
```

`cp0ctl` streams a bounded protocol through `ssh -T ... cp0-dev`. It does not
use `scp`, a general temporary upload, sudo or Bash. The device rechecks policy,
Developer Mode, pairing and the package signature on every mutation.

Before install or launch, confirm no stability, recovery, update or factory
acceptance run is active. Launching an App invalidates active stability
acceptance. Device operations require explicit authorization; local build,
simulation, signing and signature verification do not.

## Close access

Turn Developer Mode Off after testing. Pairing and App mutations fail
immediately, and sshd stops unless the independent **Owner SSH Shell** setting
is On. Existing Apps keep their normal runtime permission isolation; disabling
Developer Mode does not convert or broaden those permissions.

Owner SSH Shell is not needed for App development. Even when independently
enabled, paired keys remain restricted to `cp0-dev`, forwarding remains off and
Developer Mode never becomes a system-component update path.
