# SDK 与 manifest 版本策略

<!-- doc-locale: zh-CN -->
> [English](SDK-VERSIONING.md) | **简体中文**

## SDK ABI

WIT package 版本使用语义化版本。应用在 manifest 中声明所需的 `major.minor`：

- major 变化可以删除或改变已有类型、函数和行为；
- minor 变化只能增加可选能力，并保持已有应用可运行；
- 文档和实现修正使用仓库 patch 版本，不改变 manifest 的 SDK 要求。

设备必须拒绝未知 major，允许运行 `app.minor <= device.minor` 的相同 major 应用。
在 SDK 1.0 前，0.x 的每个 minor 都可以包含破坏性变更，但仓库必须同时更新 WIT、
示例、运行时和迁移说明。

当前设备 SDK 为 `1.1`，WIT package 为 `cardputerzero:sdk@1.1.0`。同一 major 下的
`1.0` 应用保持兼容；除当前 major
的向后兼容规则外，设备仅通过显式 allowlist 接受精确的 legacy `0.1`；`0.0`、
`0.2` 和其他任意 `0.x` 都不会因为 major 相同而被接受。manifest 和安装器都要求
规范的十进制 `major.minor`，拒绝 `01.0`、`1.00` 和三段版本。

## Manifest schema

`schema_version` 是独立整数。解析器必须拒绝未知字段和未知版本，避免拼写错误被静默
忽略。增加必填字段、改变字段含义或收紧已发布取值都需要新的 schema 版本。

## 权限

权限名称一旦发布不可改变含义。新增权限不要求升级 manifest schema；删除或拆分权限
需要保留兼容映射，直到对应 SDK major 停止支持。

## 兼容性验证

每个已发布 SDK minor 都应保留一个最小测试应用。CI 必须使用当前工具验证其 manifest，
并在 App Runtime 可用后启动这些应用执行 ABI smoke test。

扁平 ABI 以 `sdk/abi/cardputerzero-hostcalls-v1.json` 为唯一生成源。每个已发布 minor
必须在 `sdk/abi/compat` 保存不可自动更新的名称/签名快照；当前契约可以增加 hostcall，
但不能删除或改变历史快照中的 module、name 或 WAMR signature。Runtime 注册表、C
导入和 Rust 私有导入均由该契约生成，CI 拒绝任何手工漂移。

当前保留 `0.1` 和 `1.0` 两份不可变快照。两者都包含首版的 22 个 hostcall；1.1
新增受限 HTTPS Range 和 48 kHz 双声道 PCM hostcall，Runtime 注册表仍完整包含历史
名称和签名。1.0 应用和 legacy 0.1 应用继续使用
同一套受限 Runtime 注册表，不需要第二套宿主接口。
