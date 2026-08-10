# Store 提交 撤回

<!-- doc-locale: zh-CN -->
> [English](STORE-SUBMISSION-WITHDRAWAL.md) | **简体中文**

S5J 实现了冻结的 `POST /v1/submissions/{submission_id}:withdraw` 控制面操作。退出会关闭一个不可变版本，但不会删除上传的内容或重写其历史记录。

## HTTP 合约

- 授权令牌：live所有者/开发者或内部`store.submit`，或`store.control`；
- 活跃团队成员身份和启用的2FA在事务中重新读取；
- `Idempotency-Key`: 必需，16-128 字节；
- `If-Match`: 当前资源版本需要强ETag；
- 请求体：恰好为空;
- 成功: `200 application/json`, 更新的提交和其新的强ETag。

幂等性请求摘要绑定操作、提交ID和预期资源版本。精确重试将返回存储的主体和ETag，而不发出另一个事件。使用该密钥进行另一个请求将被拒绝。跨团队查找返回`not-found`，而不是披露提交信息。

## 状态和原子效果

撤销从`draft`, `uploading`, `processing`, `ready-for-review`, `in-review`, 和 `pending-secondary-review`有效。`needs-changes`, `approved`, `rejected`, 和 `withdrawn`是终端并返回`invalid-transition`。

一个 PostgreSQL `SERIALIZABLE` 事务：

1. 锁定所有者范围的提交并验证其ETag；
2. 将其资源版本增加exactly one，并将其状态更改为`withdrawn`；
3. 将任何 `queued` 或 `running` 扫描任务转换为终端 `cancelled`，清除其租约，并记录 `submission-withdrawn`；
4. 将每个活动审核分配改为`cancelled`；
5. 标记了一个未处理的`submission.scan-requested`出箱事件，因此无法创建新的扫描任务；
6. 完成幂等性记录并附加审计/出箱突变；
7. 一起提交所有效果。

数据库拒绝非法的提交转换、资源版本跳跃、扫描任务复活和删除扫描任务。扫描器的竞争性退出必须首先提交其处理结果或观察到更改的状态、版本或租约并失败掉过时的提交。审查决策使用相同的提交行锁和ETag规则。

## 保留证据

撤回不会删除包、列表、资产、上传块描述符、扫描结果、审核消息、决策、分配、审计事件或出箱历史。后续的修正是一次新的递增修订和一个新的审核。

## 验证

被忽略的 PostgreSQL 接受测试涵盖了空体强制执行、实时 RBAC/2FA/团队隔离、过时的 ETags、精确重放、终端状态拒绝、排队/运行扫描取消、悬而未决的出箱抑制、活动审核取消，以及一个注入的审计失败证明了完整的回滚。

```sh
CP0_STORE_TEST_DATABASE_URL=postgres://... \
cargo +1.85.1 test -p cp0-store-control-server --test postgres -- --ignored --nocapture
```
