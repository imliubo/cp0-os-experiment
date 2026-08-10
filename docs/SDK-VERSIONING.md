# SDK and Manifest Versioning

<!-- doc-locale: en -->
> **English** | [简体中文](SDK-VERSIONING.zh-CN.md)

## SDK ABI

WIT package versions follow semantic versioning. An application declares its
required `major.minor` version in the manifest:

- a major release may remove or change existing types, functions, or behavior;
- a minor release may add only optional capabilities and must keep existing
  applications runnable;
- documentation and implementation fixes use the repository patch version and
  do not change the manifest's SDK requirement.

The device must reject unknown major versions. Within a supported major version,
an application may run only when `app.minor <= device.minor`. Before SDK 1.0,
every 0.x minor may contain breaking changes, but the repository must update the
WIT, examples, Runtime, and migration documentation together.

The current device SDK is `1.1`, with WIT package
`cardputerzero:sdk@1.1.0`. Applications built for `1.0` remain compatible.
Outside the current major version's compatibility rules, the device accepts
only the exact legacy `0.1` version through an explicit allowlist. It does not
accept `0.0`, `0.2`, or any other `0.x` version merely because the major number
matches. The manifest and installer require canonical decimal `major.minor`
notation and reject `01.0`, `1.00`, and three-component versions.

## Manifest Schema

`schema_version` is an independent integer. The parser must reject unknown
fields and unknown versions so spelling mistakes are not silently ignored.
Adding a required field, changing a field's meaning, or narrowing a published
value requires a new schema version.

## Permissions

A published permission name must never change meaning. Adding a permission does
not require a manifest schema upgrade. Removing or splitting one requires a
compatibility mapping until the corresponding SDK major version is no longer
supported.

## Compatibility Validation

Every released SDK minor should retain a minimal test application. CI must
validate its manifest with the current tools and, once App Runtime is available,
launch these applications for ABI smoke testing.

The flat ABI uses `sdk/abi/cardputerzero-hostcalls-v1.json` as its sole
generation source. Every released minor must preserve an immutable name and
signature snapshot under `sdk/abi/compat`. The current contract may add
hostcalls, but it must not remove or change a module, name, or WAMR signature in
a historical snapshot. The Runtime registry, C imports, and private Rust imports
are generated from this contract, and CI rejects manual drift.

The repository currently preserves immutable `0.1` and `1.0` snapshots. Both
contain the original 22 hostcalls. Version 1.1 adds restricted HTTPS Range and
48 kHz stereo PCM hostcalls while the Runtime registry continues to include all
historical names and signatures. SDK 1.0 and legacy 0.1 applications use the
same restricted Runtime registry; they do not require a second host interface.
