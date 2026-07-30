# SDK 与 manifest 版本策略

## SDK ABI

WIT package 版本使用语义化版本。应用在 manifest 中声明所需的 `major.minor`：

- major 变化可以删除或改变已有类型、函数和行为；
- minor 变化只能增加可选能力，并保持已有应用可运行；
- 文档和实现修正使用仓库 patch 版本，不改变 manifest 的 SDK 要求。

设备必须拒绝未知 major，允许运行 `app.minor <= device.minor` 的相同 major 应用。
在 SDK 1.0 前，0.x 的每个 minor 都可以包含破坏性变更，但仓库必须同时更新 WIT、
示例、运行时和迁移说明。

## Manifest schema

`schema_version` 是独立整数。解析器必须拒绝未知字段和未知版本，避免拼写错误被静默
忽略。增加必填字段、改变字段含义或收紧已发布取值都需要新的 schema 版本。

## 权限

权限名称一旦发布不可改变含义。新增权限不要求升级 manifest schema；删除或拆分权限
需要保留兼容映射，直到对应 SDK major 停止支持。

## 兼容性验证

每个已发布 SDK minor 都应保留一个最小测试应用。CI 必须使用当前工具验证其 manifest，
并在 App Runtime 可用后启动这些应用执行 ABI smoke test。

