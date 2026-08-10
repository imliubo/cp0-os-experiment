# Phase 4B: reproducible `.capp` packages and signatures

<!-- doc-locale: en -->
> **English** | [简体中文](PHASE4B-CAPP-FORMAT.zh-CN.md)

## Security model

A `.capp` contains only regular files. It cannot encode owners, modes, device
nodes, links, timestamps or platform-native executables. The parser accepts at
most 256 entries, a 240-byte normalized UTF-8 path, 16 MiB per entry and a
32 MiB encoded payload. Absolute paths, empty components, `.`/`..`, backslashes,
duplicates, unsorted entries, trailing bytes and unknown header flags are
rejected before manifest parsing or extraction.

The package has two independent Ed25519 roles:

- the developer signs the canonical payload and embeds their public key;
- the store signs the developer identity, developer signature and canonical
  payload after review. Only the SHA-256 ID of the store key is embedded.

Re-signing as a developer always clears an earlier store signature. A store
cannot replace the developer identity without invalidating its own signature.
Production devices require a store key from the root-owned trust directory.
Developer mode is a separate opt-in policy and never treats an embedded public
key as trusted merely because its signature is mathematically valid.

## Canonical format v1

All integers are little-endian. The fixed header contains:

```text
magic[8] = "CP0CAPP\0"
format_version: u16 = 1
flags: u16
entry_count: u32
payload_length: u64
developer_public_key[32]
developer_signature[64]
store_key_id_sha256[32]
store_signature[64]
```

Unused fixed signature fields must be all zero. The payload is the concatenation
of entries sorted by bytewise path:

```text
path_length: u16
content_length: u32
path[path_length]
content[content_length]
```

No nondeterministic metadata is present, so the same files and keys produce
byte-identical packages. Signatures use distinct, NUL-terminated domain strings
for the developer and store roles to prevent cross-protocol reuse.

## CLI workflow

```sh
cp0ctl key generate developer.key developer.pub
cp0ctl key generate store.key store.pub
cp0ctl package ./my-app my-app-unsigned.capp
cp0ctl sign developer my-app-unsigned.capp my-app-developer.capp developer.key
cp0ctl sign store my-app-developer.capp my-app-store.capp store.key
cp0ctl verify my-app-store.capp store.pub
```

Secret keys are 32 raw bytes and are created with mode `0600`. Public keys are
32 raw bytes. Output files use create-new semantics: the CLI never silently
overwrites a key or package.

The `cp0-package` tests freeze canonical round trips, signature tamper detection,
path traversal rejection and duplicate rejection. The CLI integration path is
also exercised with the SDK-only Hello Card application.
