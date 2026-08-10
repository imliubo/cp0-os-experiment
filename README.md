# CardputerZero OS

<!-- doc-locale: en -->
> **English** | [简体中文](README.zh-CN.md)

CardputerZero OS is a compact application operating system for CardputerZero
V0.6, built around the Raspberry Pi CM0, 512 MB RAM, an SD card, and a 320x170
LCD. It replaces the traditional Linux desktop with a keyboard-first System
Shell, a trusted compositor policy, and a single-foreground application model.

Third-party applications must be built with the CardputerZero SDK and packaged
as WebAssembly applications. They do not receive direct access to Linux device
nodes, system buses, arbitrary IPC, host paths, or another application's data.
Sensitive operations are mediated by typed capability brokers and owner-managed
permissions.

## System Model

- A minimal Debian/systemd base with no X11, browser, or full desktop
  environment in the device image.
- A native 320x170 System Shell for Home, Apps, Tasks, Settings, Store, trusted
  overlays, first-boot provisioning, and recovery entry points.
- A Weston-based compositor policy that enforces one visible foreground app,
  trusted system layers, global keys, focus, screen sleep, and task switching.
- WAMR AOT, one Linux process and stable UID per running app, namespaces,
  seccomp, cgroups, an empty device view, and bounded runtime resources.
- A root-owned app lifecycle and permission service plus isolated brokers for
  network, documents, audio, camera, GPIO, LoRa, storage, intents, screenshots,
  photos, display, power, and media transfer.
- Signed `.capp` packages, deterministic review and publishing records, a
  device Store service, and separate web control-plane applications.
- Product, development, and recovery access profiles with distinct SSH,
  console, writable-root, update, backup, and factory-reset boundaries.

The shared Linux kernel means this is defense in depth, not a mathematical
claim of absolute isolation. The security boundary and residual risks are
defined in the [threat model](docs/THREAT-MODEL.md).

## Project Status

The repository contains a working OS stack, SDK 1.1, example applications,
image build pipeline, Store components, recovery tooling, and host/device
acceptance suites. Core shell, app runtime, capability, package, immutable-root,
diagnostic, recovery, production-access, and verified-update foundations have
been implemented and exercised incrementally on V0.6 hardware.

This remains an engineering project rather than a published production
release. Open hardware, long-duration stability, production infrastructure,
release-signing, licensing, and rollout gates remain authoritative in the
[roadmap](docs/ROADMAP.md) and its linked specialist roadmaps. A passing host
test does not replace physical-device or finished-image acceptance.

## Repository Map

- `image/` and `bsp/`: reproducible image stages and the pinned V0.6 board
  support package.
- `system-shell/`, `compositor/`, and `protocol/`: the trusted user interface,
  window policy, and private Wayland protocol.
- `app-runtime/`, `appd/`, and `crates/`: the WASM runtime, lifecycle service,
  capability brokers, Store services, and command-line tools.
- `sdk/`, `devkit/`, `simulator/`, `examples/`, and `skills/`: public app
  contracts, developer tooling, simulator, sample apps, and build guidance.
- `developer-portal/`, `review-console/`, and `store-operations/`: Store web
  control planes that are not included in the device image.
- `docs/`, `schemas/`, `tests/`, and `scripts/`: architecture, decisions,
  contracts, build tooling, and acceptance automation.

## Quick Start

Validate a manifest and run the repository gate:

```sh
cargo run -p cp0ctl -- manifest validate examples/hello-card/app.json
make check
```

Build the relocatable App DevKit for the current host:

```sh
make devkit
```

Image builds require Docker. A development image must receive an explicit
development-only password:

```sh
CP0_FIRST_USER_PASSWORD='development-password' make image
make verify-image
```

Build a production candidate without a shared login password or SSH key:

```sh
CP0_ACCESS_PROFILE=production make image
make verify-image
```

Do not publish an image or DevKit solely because these commands pass. Follow
the release, licensing, signature, and physical-device gates in the linked
documentation.

## Documentation

- [Architecture](docs/ARCHITECTURE.md)
- [Roadmap](docs/ROADMAP.md)
- [Developer Guide](docs/DEVELOPER-GUIDE.md)
- [App DevKit Distribution](docs/APP-DEVKIT-DISTRIBUTION.md)
- [SDK Versioning](docs/SDK-VERSIONING.md)
- [Store Architecture](docs/STORE-ARCHITECTURE.md)
- [Threat Model](docs/THREAT-MODEL.md)
- [Recovery Image](docs/PHASE6C-RECOVERY-IMAGE.md)
- [Production Access](docs/PHASE6G-PRODUCTION-ACCESS.md)
- [Documentation Localization](docs/LOCALIZATION.md)

English is the default documentation language. Every maintained Markdown
document has a paired Simplified Chinese file named `*.zh-CN.md`; use the
language switch below each title to move between them.
