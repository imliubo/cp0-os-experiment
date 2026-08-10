# ADR 0001: Platform, Graphics Stack, and Application Runtime

<!-- doc-locale: en -->
> **English** | [简体中文](0001-platform-and-runtime.zh-CN.md)

- Status: Accepted
- Date: 2026-07-30

## Decision

1. The base system starts from Debian arm64 minimal and does not switch to Yocto in the first version.
2. The graphics stack uses DRM/KMS + Wayland, with the first compositor being Weston kiosk shell.
3. The system only supports a single foreground app and does not provide overlapping desktop windows.
4. Third-party applications must use the new SDK and run in WASM, and are not compatible with traditional Linux GUI applications.
5. The device uses WAMR in interpreter or AOT mode. WIT describes the public
   type interface, while a separate machine-readable flat ABI contract is the
   sole generation source for Runtime hostcalls and language imports.
6. Native processes are used only for trusted system components and do not support the installation of third-party native applications.

## Rationale

CM0 has only 512 MB of RAM. Debian maximizes reuse of existing drivers and image
work, while Weston lets the project validate the Wayland path before taking on
the maintenance cost of a custom compositor. WAMR's embedded footprint is a
better fit for this device than a full JIT runtime. Combining WASM with process
sandboxing provides a clear, auditable capability boundary for third-party apps.

## Consequences

The existing LVGL framebuffer application must be migrated or rewritten. The
SDK must provide sufficiently complete UI, media, and hardware capabilities;
otherwise, developers will demand a native escape hatch. If Weston cannot meet
the product's interaction requirements, physical-device test results will guide
the decision on whether to maintain a wlroots-based compositor.
