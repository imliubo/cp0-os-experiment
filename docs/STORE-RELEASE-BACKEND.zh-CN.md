# Store 发布后端

<!-- doc-locale: zh-CN -->
> [English](STORE-RELEASE-BACKEND.md) | **简体中文**

S5F 为经过审核的 Store 版本添加了面向开发者的 PostgreSQL 和 HTTP 控制路径。它运行在设备镜像之外，并不持有 Store 签名密钥。

## API 切片

- `POST /v1/releases` 从一个已批准的提交创建一个不可变的 Release 身份。
- `GET /v1/releases/{release_id}` 由调用者的团队拥有的 Release。
- `POST /v1/releases/{release_id}:schedule` 记录了将来的发布时间。
- `POST /v1/releases/{release_id}:publish` 排队隔离发布并返回 `202` 以及状态 `publishing`。
- `POST /v1/releases/{release_id}:pause|resume|remove` 记录开发人员控制决策并请求更高版本的目录重建。

所有写操作都需要 `Idempotency-Key`。现有资源的修改也需要一个强 `If-Match` ETag。安排和删除接受严格的有界 JSON；发布、暂停和恢复需要一个空的主体。精确的重试返回存储的状态、主体和 ETag。

## 授权和所有权

读写需要当前角色为`owner`或`release-manager`的活跃团队成员，并且其令牌具有`store.release`或内部`store.control`权限。写操作还需要当前的双重认证。服务通过其永久应用所有者加入发布或提交，因此返回一个跨团队ID`not-found`，而不是暴露其存在。

创建锁定提交并仅接受最终的`approved`。数据库触发器独立地需要完成的主要和次要任务，两个批准决策和两个不同的审阅者身份。仅仅编写一个`approved`状态不能创建一个发布。数据库唯一约束允许每个不可变的提交只有一个发布身份，即使在并发请求下也是如此。发布百分比和发布身份在创建后不能更改。

## 状态和事务边界

开发者控制的过渡是：

```text
ready          -> scheduled | publishing | removed
scheduled      -> publishing | removed
publish-failed -> publishing | removed
published      -> paused | removed
paused         -> published | removed
```

只有孤立的发布者可以将 `publishing` 完成为 `published` 或 `publish-failed`。成功的完成必须绑定一个非零且全局有序的目录序列。S5F 不会暴露一个 HTTP 短捷方式来暴露那个内部信任边界。

每次写操作使用一个包含实时身份验证、所有者团队查找、幂等性预留、行锁定、ETag和状态验证、资源更新、审计事件和出箱事件的 PostgreSQL `SERIALIZABLE` 事务。状态变化还会附加 `release_operations`；数据库触发器拒绝更新或删除这些记录，并拒绝跳过的版本、可变发布或非法状态元数据。

移除备注保留在追加只读操作详情中供授权操作员保留。Outbox载荷仅携带结构化的理由代码，从不携带完整的备注。暂停、恢复和移除事件发出 `catalog.rebuild-requested`；此事件本身并不声称存在新的签名目录。

## 验证

```sh
cargo test -p cp0-store-control-server
cargo clippy -p cp0-store-control-server --all-targets -- -D warnings

# Requires a disposable PostgreSQL 17 database.
CP0_STORE_TEST_DATABASE_URL=postgres://... make store-control-db-check
```

PostgreSQL 门禁覆盖独立双人审批创建、直接状态绕过拒绝、实时角色/scope/2FA 检查、
跨 Team 隐藏、精确重放、并发唯一性、仅允许未来时间的调度、过期 ETag、发布队列语义、
模拟 Publisher 完成、暂停/恢复/移除、发布失败重试、经过清理的 outbox payload、
append-only 操作、非法 SQL 转换，以及 audit/outbox 原子性。

S5G 现在在这一信任边界中实现 `cp0-store-publisher`: 达到
`publishing` 只有队列有效，而隔离进程重新验证不可变内容，签名包和目录，保存快照然后提交
`published`.S5H 原子地将每个提交的快照绑定到一个只读追加的透明度叶和签名检查点。见 `STORE-PUBLISHER.md` 并且
`STORE-TRANSPARENCY.md`. 生产 HSM 集成和密钥仪式仍需外部基础设施控制。
