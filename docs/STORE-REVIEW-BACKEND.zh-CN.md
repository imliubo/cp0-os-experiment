# Store Review Backend

<!-- doc-locale: zh-CN -->
> [English](STORE-REVIEW-BACKEND.md) | **简体中文**

S5E 添加了第一个PostgreSQL和HTTP垂直切片，用于人类Store审核。
S5L 在此基础上增加了强制独立二次审核和双重批准。
S8J 添加了不可变审核读模型、有界提交详细信息API以及
生产导向的审核控制台数据路径。
它运行在Store控制平面中，永远不会安装在CardputerZero设备上。

## API 切片

- `GET /v1/review/submissions?cursor=&limit=` 列出了可领取的
  `ready-for-review` 和 `pending-secondary-review` 提交，加上调用者的
  活动任务，使用稳定且有界游标。每个 `ReviewQueueItem`
  包含 `review_stage`，`assigned_to_caller`，以及与其扫描器报告绑定的不可变版本风险评估，加上权威的应用显示名称、开发者名称和类别；主要审阅员永远不会看到自己的提交作为次要审阅候选。默认页面大小为 25，硬上限为 50。
- `GET /v1/review/submissions/{submission_id}` 返回权威的 提交和应用摘要，绑定风险评估，验证扫描摘要，导入项，权限，发现项，分配项，决策项，最新六条消息，以及最新32条安全相关的审计预测。显式截断标志区分完整历史记录和有界尾部。
- `POST /v1/review/submissions/{submission_id}:begin` 原子地声明一个 Submission 并将其改为 `in-review`。
- `POST /v1/review/submissions/{submission_id}/decisions` 应用一个结构化的 `needs-changes`、`approved` 或 `rejected` 决策并完成当前分配。主要批准移至 `pending-secondary-review`；只有来自不同次要审核人的批准才能达到最终 `approved`。
- `POST /v1/submissions/{submission_id}/messages` 在不更改提交内容或ETag的情况下，附加一个开发人员或分配的审阅者消息。

开始和决策变异需要同时有 `Idempotency-Key` 和一个强大的 `If-Match` ETag。消息需要 `Idempotency-Key`。精确重试返回存储的响应；使用不同的请求进行键重用返回 `idempotency-conflict`。

## 评审信任边界

审阅者是内部身份，不是App团队成员。`reviewers`和`reviewer_access_tokens`形成一个单独的身份域，这些不变量包括：

- 审阅者角色是`reviewer`、`senior-reviewer`或`admin`；
- 身份必须处于活动状态并且已启用双因素认证；
- 审阅者令牌包含恰好一个 `store.review` 范围，在一小时内过期，仅以 SHA-256 散列存储，并且只能变为作废状态。
- 一个数据库建议锁和跨表触发器防止一个令牌摘要同时存在于开发者域和审查者域中；
- 共享消息认证即使数据库不变量被绕过也会拒绝模糊凭证。

开发者消息仍然使用活跃团队成员身份、所有者团队检查,`store.submit`和2FA。审阅者消息需要对该提交已有分配。未对该提交进行认领的审阅者不能对该提交进行评审或加入其消息线程。

## 交易和数据模型

每次写操作都在一个 PostgreSQL `SERIALIZABLE` 事务中运行。认证、角色、2FA、范围、分配、当前状态、ETag、幂等性预留、资源突变、审计事件和出箱事件一起提交。提交请求在提交项上获得行锁，确保并发的提交请求为每个阶段生成一个活跃分配。

审核任务保留不可变的审核人、类型和源版本绑定，并仅从`active`过渡到`completed`或`cancelled`。每个只读决策都有一个唯一的外键链接到其活跃的任务。数据库触发器强制执行主项在次项之前、审核人独立性、合法状态过渡，以及在最终`approved`之前两个不同的批准。

发布创建独立地加入主要和次要任务及其审批决策；直接编写一个`approved`提交不能绕过这个门。非审批决策需要至少一个独特的结构化原因代码和一个有边界的操作性备注。S5L对每个提交应用更强的双审标准。S5N增加确定性风险等级和数据库防篡改检查，详见`STORE-RISK-POLICY.md`。

S8J 在扫描工人的成功结果事务中存储 `submission_review_metadata`。它不可变地绑定提交、待审核扫描、应用显示名称、类别、默认区域和创建时间。数据库约束触发器拒绝不符合要求或未准备好审核的扫描，并使投影只读。队列和详细读取内连接此投影；因此，更早的扫描在预览之前失败并必须重新扫描才能进入队列。

S5M 添加了独立的 React/Vite 审查控制台，包含队列阶段/搜索过滤器，提交页面检查，精确哈希值，扫描发现，权限，导入，消息，审计历史，索赔控制和结构化决策。其严格的 API 客户端省略了浏览器凭据，并将索赔/决策绑定到 ETags 和幂等性键。S8I 提供了针对特定受众的工作force BFF；S8J 移除了运行时预设项，内存中获取短期 `store.review` 令牌，从 Store Control 读取队列/详细状态，并在每次变更后刷新权威状态。生产 IdP/JWKS 和部署仍为外部关卡。

## 验证

```sh
cargo test -p cp0-store-control-server
cargo clippy -p cp0-store-control-server --all-targets -- -D warnings

# Requires a disposable PostgreSQL 17 database.
CP0_STORE_TEST_DATABASE_URL=postgres://... make store-control-db-check
```

PostgreSQL 网关覆盖了有界分页、畸形游标、严格操作路径、必需的 ETags、实时 2FA/过期/撤销、跨域凭证、精确重放、并发声明、主要审查人排除次要审查、分配授权、结构化决策验证、开发团队隔离、追加记录、注入的次要决策回滚、双重批准发布强制执行、数据库令牌域唯一性、不可变审查元数据绑定、关闭失败遗留扫描、详细界限和扫描报告摘要/风险重新验证。
