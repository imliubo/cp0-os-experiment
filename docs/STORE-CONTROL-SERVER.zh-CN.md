# Store 控制服务器

<!-- doc-locale: zh-CN -->
> [English](STORE-CONTROL-SERVER.md) | **简体中文**

`cp0-store-control-server` 是 Store Control API 的 PostgreSQL 和 HTTP 调用适配器。它是一个开发人员/审阅者控制平面服务，并不是 CardputerZero 设备镜像的一部分。

## 实现的 API 切片

- `POST /oauth/device/code`, `/oauth/device/authorize`, 和 `/oauth/token`;
- `GET /v1/teams/{team_id}`；
- `POST /v1/teams/{team_id}/members/{member_id}:set-role`；
- `POST /v1/teams/{team_id}/members/{member_id}:remove`；
- `POST /v1/teams/{team_id}/members/{member_id}:suspend|restore`；
- `POST /v1/apps` 和 `GET /v1/apps/{app_id}`；
- `POST /v1/apps/{app_id}/submissions`；
- `PUT /v1/submissions/{submission_id}/parts/{part_name}`；
- `POST /v1/submissions/{submission_id}:finalize`；
- `POST /v1/submissions/{submission_id}:withdraw`；
- `GET /v1/submissions/{submission_id}`；
- `POST /v1/submissions/{submission_id}/messages`；
- `GET /v1/review/submissions`；
- `POST /v1/review/submissions/{submission_id}:begin`；
- `POST /v1/review/submissions/{submission_id}/decisions`；
- `POST /v1/releases` 和 `GET /v1/releases/{release_id}`；
- `POST /v1/releases/{release_id}:schedule|publish|pause|resume|remove`；
- `POST /reports/v1/content`；
- `GET /v1/moderation/reports` 和
  `POST /v1/moderation/reports/{report_id}:decide`；
- `GET /v1/apps/{app_id}/moderation-notices`；
- `POST /v1/moderation/notices/{notice_id}:appeal` 和
  `POST /v1/moderation/appeals/{appeal_id}:decide`.

所有写操作都验证哈希后的持有令牌并重读当前团队角色，
2FA 状态和范围在一个 PostgreSQL `SERIALIZABLE` 事务中。应用写操作需要 `store.apps.write`；提交写操作需要 `store.submit`。内部 `store.control` 范围可以执行任一操作。精确的幂等重试返回存储的状态/主体/ETag，而另一个请求重用的键失败 `idempotency-conflict`。

开发者 OAuth 设备授权流程仅发放 15 分钟的 `store.submit` 令牌。
设备代码和访问令牌仅以哈希形式存储；审批需要具有精确范围和启用两步验证的活生生的所有者/开发者身份。轮询时间、一次性交换、审批幂等性、状态转换、审计和出箱操作均通过事务强制执行。请参阅 `STORE-OAUTH-DEVICE-FLOW.md` 了解协议以及剩余的生产身份/团队边界。

Moderation v1 是一个非生产工程切片。公共输入只接受一个精确发布的发布版本和一个固定原因；它需要一个随机的幂等键，并且不接受任何自由文本、账户/设备身份、联系数据、请求时间戳、IP、User-Agent、日志或附件。一个活跃的2FA `admin` 可以处理精确的 `store.moderation` 范围内的边界SLA队列。团队所有者/开发者只能阅读他们自己的App通知，并且可以创建一个结构化的申诉。所有转换都是可序列化的、版本化的、幂等的、审计过的、封箱的，并且由只追加修订支持。该切片不改变发布或目录状态；生产执行仍受批准政策、双重控制、可逆抑制和操作所有权的阻塞。详见 `STORE-MODERATION-V1.md`。

团队阅读揭示调用者的有界活跃/暂停成员列表。角色、暂停/恢复和终端移除更改需要拥有者具有 `store.teams.write`，一个强大的团队 ETag，以及在五分钟内通过 MFA 认证。事务推进团队/成员版本，保留最后一个活跃的拥有者，撤销目标成员的现有令牌，并发出一个审核/出箱事件。暂停的身份仍然可见但不能认证；移除保留身份行以供引用和审核，但在响应中隐藏它。恢复会再次撤销并从不发放凭证。

外部 OIDC 和 Portal BFF 边界在 `STORE-IDENTITY-TEAMS.md` 中冻结；凭证在该服务之外。

上传端点接受一个连续的最多 256 KiB 的片段，并检查 `If-Match`, `Content-Range` 和 `Content-SHA256`. 片段再次被哈希并存储在一个仅所有者可访问的 `0700` 内容寻址根下。数据库只存储片段的不可变描述符。最终锁定提交，重新打开每个片段，重新计算每个声明的对象摘要和冻结的提交内容摘要，然后原子地将 `uploading` 更改为 `processing` 并通过事务出箱发出扫描请求。

撤回接受空体，并允许所有者/开发者关闭一个`draft`, `uploading`, `processing`, `ready-for-review`, 或 `in-review` 提交。相同的交易会更新其ETag，取消任何排队/运行中的扫描任务和活跃的审核分配，抑制未被消费的扫描请求事件，并将`submission.withdrawn`追加到审核/出箱记录中。文件和之前的事件保持不变。参见`STORE-SUBMISSION-WITHDRAWAL.md`。

`cp0-store-scan-worker` 通过一个过期的数据库租约消耗该事件。
它独立地重新打开只读对象，将包密钥绑定到一个活跃的团队开发人员密钥，执行有界包/WASM/列表/PNG 检查，并在提交前原子地记录一个追加的结果。请参见`STORE-SCAN-WORKER.md` 了解其单独的信任边界和主机配置文件。

人工审核使用单独的内部审核员身份和令牌域；审核员从不被表示为App团队成员。队列读取和审核写入需要一个活跃的身份、2FA、一个实时的一小时令牌和确切的`store.review`范围。开始和决策操作保留与开发人员突变相同的SERIALIZABLE、ETag、幂等性、审计和出箱保证。有关分配和消息规则，请参见`STORE-REVIEW-BACKEND.md`。

释放读取和写入需要拥有者或发布管理者的`store.release`权限（或内部`store.control`范围）；写入还需要实时2FA。创建锁定并验证拥有者团队批准的提交。调度、发布、暂停、恢复和删除使用强ETag，并在同一事务中附加不可变操作记录。发布仅进入`publishing`并发出`release.publish-requested`；一个隔离的发布者必须在设置`published`和Catalog序列之前签署并发布一个Catalog。S5G在`cp0-store-publisher`中实现了该边界；参见`STORE-RELEASE-BACKEND.md`和`STORE-PUBLISHER.md`。

数据库在对象写入后回滚可能会留下一个不可到达的内容寻址片段。它没有任何提交状态。单独的`cp0-store-object-gc`维护二进制文件现在提供了一个默认的干运行、24小时宽限期的标记和清扫过程，与PostgreSQL顾问锁协调；请参阅`STORE-OBJECT-GC.md`。生产复制、保留批准和多区域对象生命周期仍然与这个本地文件系统引用后端分开。

## 运行

该二进制文件需要：

- `CP0_STORE_DATABASE_URL`: PostgreSQL 连接 URL；
- `CP0_STORE_OBJECT_ROOT`: 绝对，服务拥有的对象目录；
- `CP0_STORE_LISTEN_ADDR`: 可选，默认值为`127.0.0.1:8787`.

非回环绑定被拒绝，除非设置了`CP0_STORE_ALLOW_NON_LOOPBACK=1`。
那扇门不添加TLS：一个生产部署必须在单独的、加固的入口处终止HTTPS，并保持服务/数据库/对象根私有。

```sh
CP0_STORE_DATABASE_URL=postgres://... \
CP0_STORE_OBJECT_ROOT=/var/lib/cardputerzero-store/objects \
cargo run -p cp0-store-control-server
```

## 验证

```sh
cargo test -p cp0-store-control-server
cargo clippy -p cp0-store-control-server --all-targets -- -D warnings
cargo +1.85.1 check -p cp0-store-control-server --all-targets

# Requires a disposable PostgreSQL 17 database.
CP0_STORE_TEST_DATABASE_URL=postgres://... make store-control-db-check
```

数据库闸门覆盖了精确重放，竞争 App ID，并发提交修订分配和审查声明，实时 RBAC/2FA/范围/撤销检查，256 KiB 块边界，过时的 ETags，非连续范围，摘要不匹配，最终重放，独立主次分配授权，结构化决策，开发人员/审查员消息，仅批准的 Release 创建，并发 Release 唯一性，调度，发布队列，暂停/恢复/移除，发布重试，提交撤回/清理/重放，注入交易回滚和只读数据库触发，团队隔离，多因素认证新鲜度，最后所有者保护，角色/生命周期重放，即时令牌撤销，暂停令牌拒绝，终端成员数据库强制执行，双重批准 Release 强制执行和次要决策回滚。

相同的数据库门覆盖了报告接收时的隐私领域拒绝，精确匿名重放，发布身份绑定，操作员范围/角色/双因素认证检查，SLA队列排序，团队隔离的通知，一次性申诉，原子申诉解决，修订不可变性，以及报告表中不存在身份/网络列。

身份账户链接、邀请、门户会话、动态恶意软件智能、生产审查SSO、生产对象存储和通用外部交付未由此HTTP切片实现。本地引用后端垃圾回收由单独的维护二进制实现。
隔离签名/目录发布和透明日志记录由`STORE-PUBLISHER.md`中描述的S5G/S5H发布边界实现。
