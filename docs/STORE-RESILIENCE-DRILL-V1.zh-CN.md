# Store容错演练v1

<!-- doc-locale: zh-CN -->
> [English](STORE-RESILIENCE-DRILL-V1.md) | **简体中文**

S8E 结合了容量、发布、CDN、数据库恢复、队列重放和签名器故障门。这些是工程验收演练，而不是声称生产多区域基础设施或HSM仪式已完成。

## 自动门

- 出版者构建并独立验证一个由1024个应用组成的丰富目录，使用恰好16个有符号碎片。每个碎片包含的应用不超过64个，并且大小不超过48 KiB。
- 循环回路 Store 原始点执行 HTTP 范围、速率限制、生成切换、不可用原始点、路径逃逸拒绝和旧包的移除。设备测试在分片缺失时保留之前的目录，并拒绝截断、替换或校验和不符的资源。
- PostgreSQL Publisher 接收注入一个目录外失败，恢复一个过期租约而不重用序列，重放不变代，仅在完全验证后修复陈旧的`current`指针，并拒绝直接 SQL 变更。
- 文件键引用签名器拒绝缺少文件、符号链接、暴露模式、长度错误和相对路径。生产HSM停机和密钥仪式仍然是单独的部署关卡。

`scripts/run-store-database-restore-drill.sh` 创建一个自定义格式的转储，
拒绝覆盖或丢弃任何数据库，仅恢复到明确命名的`cp0_store_*restore*`目标，比较迁移、发布任务、目录根/碎片、透明度、出箱和审计中的确定性行指纹。它在`target/store-resilience`下写入私有证据，并故意保留恢复后的数据库以供检查。

## 2026-08-01 证据

PostgreSQL 17 源代码 `cp0_store_s8a_publisher_20260801` 被恢复到新的数据库 `cp0_store_s8e_restore_20260801` 中。源代码和恢复匹配：

| 表格 | 行 | 行指纹 |
| --- | ---: | --- |
| `audit_events` | 130 | `c3491b7296cbafd097a9227383576639` |
| `store_transparency_checkpoints` | 65 | `a7614be8c4d2568a035d538a164ca973` |
| `store_transparency_leaves` | 65 | `49cad105d5ba0cfd05d9e8a99992808b` |
| `outbox_events` | 195 | `ecfd20094a86136ff71f85ea4a4ccdec` |
| `store_catalog_shards` | 40 | `22ab8fe508255604ae3c00aa734cf266` |
| `store_catalog_snapshots` | 65 | `4a362f0ed7a47388d69bd4efaf170c81` |

恢复的数据库报告了20次迁移，61个非内部触发器和422个验证约束。直接的目录快照更新引发了SQLSTATE `55000` 并被钻头捕获。行指纹是恢复操作的相等性检查；签名的目录对象和透明性验证仍然是安全完整性机制。
