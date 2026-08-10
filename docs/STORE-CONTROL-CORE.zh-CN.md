# Store 控制事务核心

<!-- doc-locale: zh-CN -->
> [English](STORE-CONTROL-CORE.md) | **简体中文**

`cp0-store-control` 是 S5 后端切片的第一个实例。它是一个与框架无关的 Rust 领域核心，用于冻结的 Store Control API。它不监听网络端口，并且不链接到设备镜像中。

核心在任何 PostgreSQL 或 HTTP 适配器运行之前拥有这些不变量：

- 团队角色来自当前服务器端成员身份，且每位用户写入需要两步验证；
- 一个团队始终保留至少一个Owner和成员的电子邮件身份；团队内部的成员电子邮件身份是唯一的；
- App IDs 有一个永久的所有者并且没有删除/回收过渡；
- 提交包，列表和资产描述符按修订版本冻结；
- 只有扫描器和审阅者服务角色才能执行其确切状态转换，只有经过批准的提交才能创建一个 Release。
- 变异需要精确的资源版本和有界幂等键；
- 精确重试会返回原始结果，而重用则在另一个请求中失败并返回`idempotency-conflict`；
- 每次成功的变更都会附加一个清理后的审计事件和一个出箱事件；失败的前提条件既不会附加清理后的审计事件也不会附加出箱事件；
- 审计事件存储请求和幂等键的哈希值，而不是原始的幂等键；
- 已完成的目录发布序列是非零且全局单调的。

`ControlPlane` 将状态保存在内存中，以便确定性测试完整事务语义；它不能替代量产持久化。
持久化适配器由 `cp0-store-control-server` 实现，覆盖 App 注册/查询、submission 的
创建/上传/完成/读取、人工 Review 和开发者 Release 控制。每次 mutation 对应一个
PostgreSQL 可串行化事务，其中包含资源状态、幂等结果、audit row 和 outbox row。上传
字节进入 Owner 唯一的内容寻址后端；数据库只引用声明的大小、SHA-256 和不可变 chunk
descriptor。运行细节和剩余缺口记录在 `STORE-CONTROL-SERVER.zh-CN.md`。隔离的
`cp0-store-scan-worker` 使用带过期时间的 lease 消费 finalize outbox 事件，重新验证字节并
提交 append-only 结果，详见 `STORE-SCAN-WORKER.zh-CN.md`。Release 控制有意停在
`publishing`；独立的 `cp0-store-publisher` 负责 Store 签名、确定性 Catalog 发布和崩溃
恢复，详见 `STORE-PUBLISHER.zh-CN.md`。

适配器在查找前对承载令牌进行哈希处理，并在数据库事务中验证令牌过期、撤销、当前团队角色、当前双因素认证状态和作用域。它仅接受有界OpenAPI JSON，返回有界的`application/problem+json`，重试序列化/死锁失败，并默认绑定到本地回环。非本地回环绑定需要外部过程中的显式环境网关和TLS终止。

迁移还强制执行永久的应用所有权、不可变的提交内容和上传块描述符、一次性最终化摘要、不可变的发布身份、精确的发布过渡、只读的审核/发布/审计记录、至少一个团队所有者、稳定的成员身份和单向的令牌撤销。剩余的控制面操作必须重用这些交易和响应边界。

生产ID必须由持久化适配器或注入的加密ID源分配。内存中的计数器是确定性测试框架，必须不在公共部署中暴露。

## 验证

```sh
cargo test -p cp0-store-control
cargo clippy -p cp0-store-control --all-targets -- -D warnings
cargo test -p cp0-store-control-server
cargo clippy -p cp0-store-control-server --all-targets -- -D warnings
cargo +1.85.1 check -p cp0-store-control-server --all-targets

# Requires a disposable PostgreSQL database.
CP0_STORE_TEST_DATABASE_URL=postgres://... make store-control-db-check
```
