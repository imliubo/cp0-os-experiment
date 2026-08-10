# Documentation Localization

<!-- doc-locale: en -->
> **English** | [简体中文](LOCALIZATION.zh-CN.md)

## Language Policy

English is the default and canonical entry language for maintained Markdown
documentation in this repository. Every English `FILE.md` must have a paired
Simplified Chinese translation named `FILE.zh-CN.md` in the same directory.

Both files describe the same contract. Neither translation is a place to keep
extra requirements, acceptance results, warnings, or implementation status that
does not exist in the other language.

## Required Header

Place the locale marker and language switch immediately after the level-one
heading. English documents use:

```md
<!-- doc-locale: en -->
> **English** | [简体中文](FILE.zh-CN.md)
```

Simplified Chinese documents use:

```md
<!-- doc-locale: zh-CN -->
> [English](FILE.md) | **简体中文**
```

YAML front matter, when required by a tool such as the Skill loader, remains at
the beginning of the file. Put the locale marker and switch after the first
level-one heading that follows the front matter.

## Editing Rules

1. Change the English and Simplified Chinese files together in one task.
2. Preserve code blocks, identifiers, command lines, paths, protocol names,
   hashes, versions, and link targets unless the underlying technical fact also
   changes.
3. In Chinese documents, link to another document's `.zh-CN.md` version when
   one exists. The language switch itself is the intentional link back to the
   default English file.
4. Use established project terminology consistently. In particular, translate
   "production image" as "量产镜像", "acceptance gate" as "验收门禁",
   "immutable root" as "不可变根文件系统", "signed integer" as
   "有符号整数", and "capability broker" as "能力代理服务".
5. Do not translate product, component, protocol, or interface identifiers such
   as CardputerZero, System Shell, Runtime, compositor, appd, WAMR, Wayland,
   Store, SDK, ADR, BSP, API, UID, IPC, SSH, DRM, GPIO, LoRa, WASM, and
   WebAssembly.
6. Treat machine translation as a draft. Review security constraints, numbered
   requirements, negation, status labels, units, and acceptance conclusions
   against the source before merging.
7. Preserve fail-closed semantics explicitly. A rejected operation or invalid
   state must not be translated as shutting down or powering off the device.

## Validation

Run the localization gate directly or as part of `make check`:

```sh
./tests/test-document-localization.sh
make check
```

The gate checks that every maintained default Markdown file has its paired
translation, that both files carry the expected marker and reciprocal language
switch, and that default English documents do not contain untranslated Chinese
prose. It ignores generated and other content excluded by Git.

Structural checks cannot prove that a translation preserves meaning. Reviewers
must still compare changes in both languages, with additional scrutiny for
security boundaries and claims based on physical-device evidence.
