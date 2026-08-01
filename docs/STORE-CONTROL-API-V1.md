# Store Control API v1

`schemas/store-control-v1.openapi.json` 是 Developer Portal、`cp0ctl store submit`、Review
Console 和 Release Service 的首版控制面契约。设备上的 System Shell 和 `cp0-stored` 不调用
这个 API，只读取不可变发布面。

## 请求约束

- `cp0ctl` 使用 OAuth Device Authorization Grant，access token 最长有效 1 小时且只授予
  `store.submit` scope；CLI 不保存或上传开发者私钥。
- 当前服务端纵向切片实际签发 15 分钟 token；设备码有效 10 分钟，初始轮询间隔 5 秒，
  过快轮询每次增加 5 秒、上限 30 秒。审批要求实时 owner/developer、`store.submit` 和 2FA，
  并以幂等事务写入 audit/outbox；详见 `STORE-OAUTH-DEVICE-FLOW.md`。
- `/v1` 下所有 POST/PUT 都要求 16-128 字节的 `Idempotency-Key`。
- 修改已有状态的操作同时要求 `If-Match`，服务以 ETag/resource version 拒绝并发覆盖。
- App ID 永久归属一个 team；已删除名称不能自动供其他开发者重新注册。
- package、Listing 和 2-6 个资源对象按声明 SHA-256 上传；每次 PUT 使用 `Content-Range`
  发送最多 256 KiB 的连续分片，`Content-SHA256` 是该分片摘要。相同 part/range 只允许相同
  摘要的幂等重放，不能覆盖成不同内容。
- `finalize` 重新读取所有对象，验证长度和摘要，计算 submission content digest 后冻结 revision。
- `withdraw` 请求体必须为空，并同时要求 `Idempotency-Key` 和当前 `If-Match`；成功返回 `200`、
  更新后的 Submission 和新 ETag。

content digest 固定为 SHA-256：先写入 ASCII domain `CardputerZero Store submission content
v1\0`，再按 package SHA、Listing SHA 分别写入 `u64 big-endian length + UTF-8 bytes`；随后按
Listing 顺序写 icon 和截图的 path、SHA（同样使用长度前缀）、`u64 bytes`、`u16 width`、
`u16 height`，全部为 big-endian。服务端必须独立复算，不能信任 finalize 请求。

错误使用有界的 `application/problem+json`，稳定 `code` 供 CLI 处理；内部路径、SQL、对象存储
key、token 和扫描器输出不能进入 `detail`。

## Team 与认证上下文

Team 读取只返回 access token 当前成员所属的 Team，跨 Team ID 统一返回 `not-found`。成员角色
修改要求 Owner、`store.teams.write`、当前 Team ETag、幂等键、已启用 2FA，以及五分钟内由
受信 IdP 证明的 MFA 时间；仅凭 token 创建时间不能满足 step-up。成功修改会同时递增 Team 和
成员版本、撤销目标成员的全部旧 token，并原子写 audit/outbox。最后一个 Owner 不能被降级。

Portal 的 OIDC/BFF、cookie、CSRF 和账户恢复边界见 `STORE-IDENTITY-TEAMS.md`。Store API 不接收
密码、WebAuthn credential 或 OIDC refresh token。

## Submission 状态机

```text
DRAFT -> UPLOADING -> PROCESSING -> READY_FOR_REVIEW -> IN_REVIEW
  |          |             |               |              |
  +------> WITHDRAWN <------+---------------+              +-> APPROVED
                           +-> NEEDS_CHANGES                +-> NEEDS_CHANGES
                           +-> REJECTED                     +-> REJECTED
                                                          +-> WITHDRAWN
```

`NEEDS_CHANGES`、`APPROVED`、`REJECTED` 和 `WITHDRAWN` 对该 revision 都是终态。开发者修改
package、Listing 或任一资源时必须创建递增的新 revision，不能把旧 revision 重新变回
`READY_FOR_REVIEW`。Review 消息和决定是 append-only 事件，不能改写 submission 内容。

撤回会在同一个 `SERIALIZABLE` 事务内把 revision 置为 `WITHDRAWN`、取消活动扫描任务和审核
分配，并消费尚未交付的 `submission.scan-requested` outbox 事件。已完成的扫描、消息、决定、
上传对象和审计记录都不会删除。`APPROVED`、`REJECTED`、`NEEDS_CHANGES` 或已经 `WITHDRAWN`
的 revision 不能撤回；并发扫描/审核提交必须通过行锁和 resource version 与撤回事务决出唯一结果。
完整约束见 `STORE-SUBMISSION-WITHDRAWAL.md`。

只有自动扫描通过的 revision 可以进入 `READY_FOR_REVIEW`；只有 Review Service 可以进入
`IN_REVIEW/APPROVED/NEEDS_CHANGES/REJECTED`。高风险权限和安全例外由服务端策略要求双人审批，
不能由请求字段关闭。

## Release 状态机

```text
READY -> SCHEDULED -> PUBLISHING -> PUBLISHED -> PAUSED
  |          |             |            |          |
  |          +-> READY     +-> PUBLISH_FAILED      +-> PUBLISHED
  |                             |            |          |
  +-----------------------------+------------+----------+-> REMOVED
```

Release 只能引用 `APPROVED` submission。`PUBLISHING` 由 Release Service 发出摘要授权，经隔离
Signer 签名后生成更高 sequence 的 Catalog；失败进入 `PUBLISH_FAILED`，不能伪装为已发布。
修复失败原因后，`PUBLISH_FAILED` 使用新的 ETag 和幂等键重新进入 `PUBLISHING`，不会绕过签名。
暂停、恢复和下架都创建更高 sequence 的 Catalog，不覆盖已发布对象，也不回滚 sequence。

## 重试与审计

客户端只在网络失败、429 和可重试 5xx 上使用带抖动退避；401 重新授权，409/412 重新读取
资源和 ETag 后由用户决定。服务端为每个状态变化记录 actor、旧/新状态、对象摘要、原因、
request ID 和 idempotency key hash，并通过事务 outbox 发布事件。原始 access token 和完整
idempotency key 不进入审计日志。
