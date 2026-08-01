# CardputerZero Store Roadmap

本 Roadmap 覆盖 Web 前端、后端和设备端。每一阶段都要求先有可自动验证的契约，再进入
真机；当前 24 小时稳定性运行结束前，所有工作仅限本地源码、模拟器、协议和构建产物。

## S0：安全安装底座（已完成）

- [x] 确定性 `.capp` 和开发者/Store 双签名。
- [x] manifest、WASM import、权限和精确审核记录绑定。
- [x] 有序限时签名 Catalog、HTTPS 公网限制和目录防回滚。
- [x] `cp0-stored` 独立服务、断点续传、摘要验证和 appd 受控交接。
- [x] appd 原子安装、严格升级、历史回滚和稳定应用 UID。
- [x] 320x170 Store 列表、详情、安装进度和离线/过期状态。

## S1：产品与契约冻结（进行中）

- [x] 定义 Developer Portal、Review Console、Store Operations 和设备端职责。
- [x] 定义控制面/发布面隔离、签名服务和端到端信任链。
- [x] 定义 Today/Apps/Search/Updates 小屏信息架构。
- [x] 冻结 `store-listing-v1` schema、分类、年龄分级和本地化边界。
- [x] 冻结 Submission/Review/Release 状态机与 OpenAPI v1。
- [x] 编写 Store 内容、隐私、审核和下架工程政策（生产文本待产品/法务签署）。

完成条件：架构评审通过，schema/OpenAPI 有严格解析和变异测试。

## S2：设备搜索与发现 MVP

- [x] 增加 `search` 有界分页协议：query、offset、limit、total、next_offset。
- [x] 在 `cp0-stored` 对已验证 Catalog 执行稳定排序的本地搜索。
- [x] 增加 `cp0ctl store search`，拒绝错配响应和越界分页。
- [x] 在 System Shell 实现 Search 输入、最近查询和空结果状态。
- [x] 增加 Search、无结果、最大文本、分页和 stale Catalog 像素回归。
- [x] 增加 Today/Apps/Search/Updates 分段和严格 SemVer Updates 计算模型；
  Catalog v1 的 Apps 显示全部应用，签名 category 字段仍按 S6 引入。

完成条件：64 应用最大目录可以在 CM0 内存预算内搜索；搜索词不离开设备。

## S3：Developer Listing 与提交 CLI

- [x] 新增 `store-listing-v1.schema.json` 和 Rust 严格验证器。
- [x] `cp0ctl store validate` 同时验证 `.capp`、Listing、资源和开发者签名。
- [x] `cp0ctl store submit` 支持 OAuth Device Flow、分片上传、重试和 finalize。
- [x] 输出可机器读取的 Submission ID、摘要和 Portal URL。
- [x] CLI 不读取/上传开发者私钥，不在日志中输出 OAuth token。
- [x] 加入确定性 fixture、断网恢复、过期 token 和摘要错配测试。

完成条件：开发者只使用 SDK、CLI 和浏览器即可提交一个版本并跟踪状态。

## S4：Developer Portal 前端

- [x] 建立账户、团队、角色、2FA 和 Developer Key 页面。
- [x] 建立 App ID 创建、名称检查和永久所有权页面。
- [x] 建立版本、Listing、本地化、图标/截图和隐私说明编辑器。
- [x] 建立上传进度、自动检查、审核消息和版本状态时间线。
- [x] 建立发布方式、预约、分阶段发布、暂停和撤回控制。
- [x] 桌面和移动浏览器可访问性、键盘操作和错误恢复测试。

完成条件：Portal 不接触私钥，所有写操作幂等且可审计。

当前 S4 交付为独立 React/Vite 前端和严格 OpenAPI 客户端，使用内存 mock 演示完整工作流；
真实身份、对象存储和写操作审计由 S5 后端接入，Portal 不进入设备镜像。

## S5：审核与发布后端

S5A 已新增 `cp0-store-control` 事务域核心：服务端 RBAC/2FA、永久 App ID、不可变 revision、
严格状态机、ETag、幂等回放、append-only audit/outbox 和 Catalog sequence 单调约束已可测试；
S5B 已新增首个 PostgreSQL/HTTP 纵向切片：App 注册/读取、实时 token/RBAC/2FA/scope 校验、
SERIALIZABLE 幂等事务、ETag、有界 Problem、append-only 数据库约束和并发/回滚验收。
S5C 已接通 `cp0ctl` 所需的 Submission 创建、256 KiB 连续分片、读取与 finalize：对象按内容
寻址保存，finalize 独立重读并复算全部对象和 content digest，并发 revision、断点 ETag、错误
摘要和事务回滚已有真实 PostgreSQL 验收。S5D 已接通隔离 Scanner：outbox 租约、只读对象重组、
团队有效开发者密钥、WASM host imports/权限、Listing/PNG、重试恢复和原子 scan result 均有
真实 PostgreSQL 验收。动态恶意样本和生产对象存储/GC 仍按下列门禁推进。
S5E 已新增独立 reviewer 身份域和审核后端纵向切片：有界队列、并发唯一领取、结构化决定、
开发者/审核员追加消息、即时 2FA/撤销、ETag、幂等、审计和 outbox 均通过真实 PostgreSQL
验收；风险分级、独立二审、双人审批和 Review Console 前端仍未完成。
S5F 已新增 Release 控制面纵向切片：仅 approved Submission 可创建 Release，owner/
release-manager、2FA、scope、团队隔离、并发唯一创建、预约、发布排队、暂停/恢复/下架、
失败重试、强 ETag、精确幂等、append-only 操作记录和原子 audit/outbox 均通过 PostgreSQL 17
验收。`publish` 只进入 `publishing`，由 S5G 的独立 Publisher 完成发布。
S5G 已新增隔离文件 key Publisher/Catalog Builder：outbox 租约、不可复用 sequence、原始对象
重算、开发者 key 复核、Store 双签名、64 App 有界确定性 Catalog、最新 Release 投影、不可变
generation、原子 Release/audit/outbox 回写与 `current` 崩溃恢复均通过 PostgreSQL 17 验收。
S5H 已为每个成功 Catalog snapshot 增加 append-only transparency leaf 和签名 Merkle
checkpoint，逐前缀验证、数据库/文件篡改拒绝与事务回滚均通过 PostgreSQL 17 验收。当前
文件 key 是受限参考实现；生产 HSM/key ceremony、compact proof 和外部 witness 尚未完成。
S5I 已接通 `cp0ctl` 开发者 OAuth Device Flow：10 分钟设备码、慢轮询、幂等审批/拒绝、实时
role/scope/2FA、15 分钟最小 scope token、并发一次性兑换、撤销、摘要存储及原子 audit/outbox
均通过 PostgreSQL 17 验收。账户注册、团队管理、Portal 会话与 reviewer SSO 仍属 Identity/
Teams 后续工作。
S5J 已补齐 Submission 撤回：owner/developer、实时 2FA/scope、强 ETag、精确幂等、合法状态图，
以及扫描任务、未消费扫描事件和活动审核分配的原子取消均通过 PostgreSQL 17 验收；历史对象、
扫描结果、审核消息和审计事件保持不可变。
S5K 已冻结外部 OIDC + Portal BFF 身份边界，并接通 Team 读取和 Owner 角色修改：五分钟 MFA
step-up、Team ETag、last-owner、成员 token 即时撤销、版本单调、幂等和 audit/outbox 回滚均通过
PostgreSQL 17 验收。账户链接、邀请/移除、Portal session endpoint 和 reviewer SSO 仍待实现。
S5L 已将所有 Submission 升级为独立双审：主审批准进入 pending-secondary-review，原主审不可领取
二审，只有不同审核员的 secondary approval 才进入 approved；decision 强绑定 assignment，Release
数据库触发器重新验证两位审核员和两次批准。并发领取、精确回放、故障回滚和直接 SQL 绕过均通过
PostgreSQL 17 验收。风险分级和 Review Console 前端仍待实现。
S5M 已新增独立 Review Console：primary/secondary/我的活动 assignment 有界队列、搜索、扫描结果、
提交截图、hash、权限/import、消息、审计、领取和结构化决定均可操作；严格客户端绑定 ETag/幂等且
不发送浏览器 cookie。真实 workforce SSO/BFF 和风险分级仍待实现。
S5N 已新增版本化审核风险策略：隔离 Scanner 根据真实 SDK 权限确定 standard/elevated/high，
append-only assessment 绑定 scan/report SHA-256，PostgreSQL 触发器重算并拒绝伪造、乱序、修改和
删除；Review Queue/OpenAPI/Console 使用同一结果。生产 workforce SSO/BFF 仍待实现。
S6A 已新增向后兼容的 Discovery Catalog v2：生产 Publisher 从审核绑定的 Listing 和 App 所属
Team 的权威显示名生成
developer、subtitle、category、keywords、age/privacy 元数据，v1/v2 严格分流；`cp0-stored`
在签名 Catalog 上增加开发者、分类和关键词本地搜索。富媒体资源缓存和小屏展示仍待实现。
S6B 已新增 Catalog v3 富媒体资源层：根 Catalog 摘要绑定 icon 和有界 details 清单，details 再
绑定 320x170 截图；Publisher 将包、图片、details、Catalog 和 transparency 对象写入同一不可变
generation，并支持 v1/v2/v3 渐进升级。S6C 已在 `cp0-stored` 实现内容寻址的 icon/details/
screenshot 缓存、精确摘要与 PNG/details 身份复验、独立预算和截图 LRU；System Shell 展示仍待实现。

- [ ] 实现 Identity/Teams、App Registry、Submission 和 Release 服务。
  当前 App Registry、Submission 上传/finalize/read/withdraw、独立双审 Review 和 Release 控制
  纵向切片已完成；OAuth 开发者 Device Flow、Team 读取/角色管理已完成；Identity 账户链接、
  邀请/移除和 Portal 会话未完成；Publisher 通过受约束 outbox 接入。
- [x] 实现隔离 Scan Worker：包格式、WASM、权限、资源和恶意样本检查。
  当前为无 IP 网络、只读对象根的确定性结构/能力扫描；动态规则、信誉源和运营隔离环境待生产化。
- [ ] 实现 Review Console、结构化问题、回复、二审和双人审批。
  当前结构化问题、回复、独立二审、双人审批、风险分级和 Console UI 已完成；生产 SSO 待实现。
- [x] 实现不可变发布 generation、事务 outbox、append-only audit 和 transparency log。
  当前 transparency v1 覆盖完整 Catalog snapshot 历史；生产对象存储、compact proof 和
  witness/gossip 属于后续基础设施。
- [ ] 接入生产 HSM 和 key ceremony；当前隔离文件 key 参考 Signer 已保证 Web 服务不能读私钥。
- [x] 实现确定性 Catalog Builder、sequence 分配、发布和紧急撤回。
  pause/resume/remove 生成更高 sequence，过期控制事件安全合并且 sequence 永不复用。
- [ ] 灾备、key rotation、权限提升和内部威胁测试。

完成条件：已审核 submission 才能发布；任意对象变化都会要求新 revision 和新审核。

## S6：富媒体 Discovery Catalog

- [x] Catalog v2 增加 developer、subtitle、category、keywords、age/privacy 元数据。
  S6A 的生产 Publisher 已生成严格 v2，设备服务同时验证 v1/v2；当前 System Shell 仍使用有界
  summary 响应，富字段详情展示属于后续切片。
- [x] 定义 32x32/48x48 图标和 320x170 截图的格式、尺寸、摘要和缓存上限。
  S6B 已冻结 PNG/descriptor/details 契约、单资源与 CM0 总缓存预算，并由 Publisher 原子发布；
  `cp0-stored` 缓存实现仍由下方独立任务跟踪。
- [ ] 增加 Today 专题、精选集合、分类和更新索引。
- [ ] 规模超过 64 应用时切换为签名根索引和有界 shard。
- [x] `cp0-stored` 原子缓存 Catalog/资源，资源解析失败不影响本地应用启动。
  S6C 在 Catalog 提交后尽力预取图标，details/截图按需缓存；截断、替换、错误尺寸、错误身份和
  不安全缓存 inode 均不会生成最终对象，且不回滚 Catalog 或阻断已验证包安装。
- [x] System Shell 增加图标、截图单页查看和权限差异展示。
  S6D 通过严格 details/media IPC 接入富详情；图片只以单个只读 `SCM_RIGHTS` 描述符传递，
  Shell 对应用/版本/类型/索引/尺寸/长度重新绑定并用 libpng 解码。五页详情覆盖图标、可滚动描述、
  320:170 单截图、升级权限差异和更新说明，UI 状态仍小于 64 KiB。

完成条件：CDN 修改、截断或替换任一资源都会在设备端被拒绝。

## S7：下载、更新与恢复体验

S7A 已增加严格 `control` 协议和 `paused`/`canceled` 状态；暂停保留摘要命名分片，取消在
全局作业锁内删除分片，恢复绑定当前 Catalog 的应用版本和包摘要，appd handoff 后拒绝控制。
设备端、CLI 和 320x170 Shell 使用同一封闭失败原因，并覆盖目录变化、stale Catalog、竞态和
网络/存储/验证/安装器故障。具体契约见 `STORE-DOWNLOAD-CONTROL-V1.md`。

- [x] 增加暂停、继续、取消和失败原因的稳定协议。
- [ ] 增加 Updates 页、单项更新和有界 Update All 队列。
- [ ] 增加下载状态栏、离开 Store 后进度和安装完成通知。
- [ ] 增加新增权限确认、策略限制和存储空间预检。
- [ ] 验证断电、断网、HTTP Range 错误、摘要错误和 appd handoff 崩溃恢复。
- [ ] 自动更新保持默认关闭；后续按充电/网络/策略显式启用。

完成条件：任何中断都不会生成半安装状态或绕过重新验证。

## S8：运营质量与隐私

- [ ] Today/专题运营工具只能引用 approved Release。
- [ ] 建立最小化、可选、去标识化的安装和崩溃聚合指标。
- [ ] 搜索词默认不上传；任何实验功能需要单独同意和保留期限。
- [ ] 建立内容举报、下架申诉、开发者通知和安全响应 SLA。
- [ ] 进行容量、CDN 故障、数据库恢复、队列重放和签名服务演练。
- [ ] 独立隐私、安全和审核公平性评审。

## S9：真机门禁（稳定性结束后）

- [ ] 部署最新 Store 协议、`cp0-stored`、appd 和 System Shell。
- [ ] Camera2 验证 Today/Apps/Search/Updates、详情和下载进度。
- [ ] 完成刷新、续传、安装、升级、离线缓存和过期拒绝六步证据。
- [ ] 测量最大 Catalog 的内存、CPU、输入延迟和 SD 写入。
- [ ] 验证下载中 Home/Tasks/权限弹窗、重启和断电恢复。
- [ ] 取回全部证据并使用独立验证器复算后才能关闭 Store 产品门禁。
