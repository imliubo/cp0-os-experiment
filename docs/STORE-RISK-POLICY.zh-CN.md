# Store 审核风险政策

<!-- doc-locale: zh-CN -->
> [English](STORE-RISK-POLICY.md) | **简体中文**

仓库风险是由不可变的Scanner结果衍生的审阅者指导，不是由开发者提供或由审阅控制台计算的。

## 政策版本 1

该策略在包签名、manifest 和 WASM 导入验证之后对声明的 SDK 权限进行分类：

| 层级 | 条件 |
| --- | --- |
| `standard` | 没有敏感权限，或者只有播放/通知输出 |
| `elevated` | 网络、用户文档、麦克风或摄像头中的一个且仅一个|
| `high` | GPIO, LoRa, 或两个或更多敏感权限 |

稳定的错误码标识了每种贡献的敏感权限。`multiple-sensitive-capabilities` 错误码在至少存在两个错误码时添加。错误码是唯一的并且排序，以便评估具有一种标准形式。

## 信任和坚持

`cp0-store-risk` 是隔离 Scanner 和控制 API 共用的唯一 Rust 策略实现。成功进入
`ready-for-review` 的扫描必须携带恰好一份 policy-v1 assessment。Scan worker 在同一个
可序列化事务中写入该 assessment、append-only 扫描结果和 submission 状态转换。

PostgreSQL 在 `submission_risk_assessments` 中存储评估结果，并将每个评估结果绑定到精确的扫描和报告 SHA-256，然后在触发器中重新评估策略 v1。直接 SQL 不能伪造一个层级、重新排列原因代码、绑定另一个报告、更新或删除一个评估结果。迁移 `0013` 确定性地为现有可审查扫描填充策略 v1。

审查队列返回最新存储的策略版本为`risk`；缺失或无效的评估将关闭而不成为可索赔队列项。
未来的策略更改将附加一个新版本而不是重写审查历史。

## 验证

```sh
cargo test -p cp0-store-risk -p cp0-store-scan -p cp0-store-scan-worker
cargo clippy -p cp0-store-risk -p cp0-store-scan -p cp0-store-scan-worker \
  --all-targets -- -D warnings

CP0_STORE_TEST_DATABASE_URL=postgres://... make store-control-db-check
```

PostgreSQL 门禁覆盖原子创建、迁移回填、规范化等级和原因、append-only 强制、伪造
策略或报告拒绝，以及与外围扫描事务共同执行的 Review Queue 串行化和回滚。
