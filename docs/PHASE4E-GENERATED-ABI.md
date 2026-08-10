# Phase 4E: generated host ABI

<!-- doc-locale: en -->
> **English** | [简体中文](PHASE4E-GENERATED-ABI.zh-CN.md)

## One flat contract

`sdk/abi/cardputerzero-hostcalls-v1.json` is the canonical WAMR import contract.
Each entry fixes the module/name pair, implementation function, C symbol,
parameter ownership types, result type, WAMR pointer-bound signature, native
test fallback and corresponding public WIT operation.

`scripts/generate-sdk-bindings.mjs` validates the contract and produces:

- `app-runtime/src/hostcall_symbols.inc`, included by the Runtime registration
  table;
- `sdk/c/include/cardputerzero_imports.h`, included behind the public C types;
- `sdk/rust/src/host_imports.rs`, the only Rust module allowed to declare raw
  WebAssembly imports.

Generated outputs are committed so image builds do not require Node. CI runs
the generator in `--check` mode and rejects stale output, duplicate mappings,
invalid pointer bounds or WAMR signature/type mismatches.

## WIT relationship

WIT remains the typed, language-neutral public SDK contract. It now matches the
implemented API: bounded event waiting and focused keyboard input are present;
the unimplemented logging and lifecycle callback drafts were removed. Every
public WIT host function has exactly one flat ABI mapping. Pure SDK UI helpers
need no WIT operation.

CM0 continues to execute core WebAssembly through WAMR rather than the
Component Model to keep memory and startup cost bounded. WIT values such as
strings, lists, results and options are lowered by the generated SDK facade to
caller-owned flat buffers and packed scalar results. Applications never call
the flat imports directly.

The generator's offline structural check validates the WIT package version,
interface/function mapping and balanced interface blocks. In addition, the SDK
test suite parses and resolves the complete contract with the standard
Bytecode Alliance `wit-parser` used by `wasm-tools`; it verifies the 1.1.0
package identity, interface count and exported application world. This is a
build-time test dependency only, not a Runtime or product-image dependency.

## Compatibility

`sdk/abi/compat/cardputerzero-hostcalls-0.1.json` and
`sdk/abi/compat/cardputerzero-hostcalls-1.0.json` record every published name
and WAMR signature. Tests require all entries from both snapshots to remain
present and unchanged. A compatible minor may add imports; removing an import
or changing a signature requires a new SDK major.

The frozen 1.0 compatibility baseline contains 22 bounded hostcalls and is
byte-for-byte ABI compatible with legacy 0.1. The current SDK 1.1 additive
contract contains 35 hostcalls, including HTTPS Range and 48 kHz stereo PCM;
all 22 baseline names and signatures remain unchanged. C11, C++17 and Rust
WASM builds consume the generated declarations, while Runtime builds consume
the same signature source. App installation accepts SDK 1.1, compatible 1.0,
and exact legacy 0.1, but not other pre-1.0 versions.

The pinned WAMR 2.4.5 AArch64 static Runtime rebuilt successfully with the
generated registration table. Its Phase 4E SHA-256 is
`1fc27bf80953f16a0840ea82a2fcfc17590b58c09e9ff6aa879a154d9e05130a`.
