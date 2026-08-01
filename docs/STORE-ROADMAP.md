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
摘要和事务回滚已有真实 PostgreSQL 验收。S5O 为本地参考对象后端增加默认 dry-run、24 小时宽限、
上传/GC 共享-独占互锁和 fail-closed 路径校验的 mark-and-sweep 工具；生产复制与保留策略仍属
基础设施门禁。S5D 已接通隔离 Scanner：outbox 租约、只读对象重组、
团队有效开发者密钥、WASM host imports/权限、Listing/PNG、重试恢复和原子 scan result 均有
真实 PostgreSQL 验收。动态恶意样本和生产对象存储仍按下列门禁推进。
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
S5K 已冻结外部 OIDC + Portal BFF 身份边界，并接通 Team 读取、Owner 角色修改、成员暂停/恢复和
不可逆成员移除：五分钟 MFA step-up、Team ETag、active last-owner、成员 token 即时撤销、暂停期
token 不复活、终态身份保留、版本单调、幂等和 audit/outbox 回滚均通过 PostgreSQL 17 验收。
Portal BFF 已接通严格外部 OIDC Authorization Code + PKCE、登录/回调、摘要会话、CSRF、空闲/绝对
超时、幂等 MFA step-up、会话轮换和注销；邀请 create/list/inspect/cancel/accept、Team 聚合 ETag、
邮件密文租约/退避/终态清除、七天过期和接受后会话轮换也已通过 PostgreSQL 17 端到端验收。
账户链接 list/begin/remove、双提供方恢复、依赖会话撤销、Membership 状态传播和 Portal 浏览器
适配器也已完成；生产邮件/IdP 一致性接入和 reviewer SSO 仍待实现。
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
screenshot 缓存、精确摘要与 PNG/details 身份复验、独立预算和截图 LRU；S6D 已通过严格 IPC
将图标、详情、截图、权限差异和更新说明接入 System Shell。
S8A 已新增 Catalog v4 editorial 层：Today 主推荐、1-2 个专题和每组最多 4 个应用均来自 approved
且 published 的 Release；运营 revision、Publisher snapshot、设备 `today` IPC 和 320x170 Shell
导航形成完整纵向链路。引用失效时 Publisher 安全退回 v3，不发布过期推荐。
S6E 已新增兼容的签名根索引、分类索引和有界 shard：Publisher 在 64 项或 48 KiB 任一上限先到
时确定性切换，数据库和不可变 generation 原子记录全部对象；`cp0-stored` 完整验证后原子缓存，
并通过 8 项有界 `browse`/`search` 分页访问最多 1024 项。System Shell 的 Store Apps 页现使用
`browse(all)`，在页首/页尾按需请求前后页并显示精确范围；Apps 和 Search 复用同一个 8 项页面
缓存，`cp0_ui` 保持在 64 KiB 内。客户端会拒绝错误页长、offset/next、未排序或意外分类响应。

- [x] 实现 App Registry、Submission、独立双审 Review 和 Release 服务主体。
  App Registry、Submission 上传/finalize/read/withdraw、Release 控制、OAuth 开发者 Device Flow、
  Team 读取/角色管理和受约束 Publisher outbox 均已有 PostgreSQL/HTTP 验收。
- [x] 为本地内容寻址后端实现上传互锁、年龄门禁、dry-run 和 fail-closed 对象 GC。
  生产复制、跨区域恢复和正式保留策略不由本地文件后端替代。
- [ ] 完成 Identity 自助闭环：成员暂停/恢复/移除已完成；Portal BFF、外部身份链接、
  邀请和会话的 v1 安全边界与 OpenAPI 已冻结，PostgreSQL 状态机与绕过验收已完成；
  Portal BFF 的 OIDC 登录/会话/MFA step-up/注销、邀请 HTTP/邮件 worker、外部身份链接及 Portal
  账户安全页真实 BFF 适配器纵向切片已完成；生产邮件供应商及身份提供方一致性接入仍待完成。
- [x] 实现隔离 Scan Worker：包格式、WASM、权限、资源和恶意样本检查。
  当前为无 IP 网络、只读对象根的确定性结构/能力扫描；动态规则、信誉源和运营隔离环境待生产化。
- [x] 实现 Review Console、结构化问题、回复、二审、双人审批和风险分级。
- [ ] 接入 Review Console 生产 workforce SSO/BFF，并完成访问撤销演练。
- [x] 实现不可变发布 generation、事务 outbox、append-only audit 和 transparency log。
  当前 transparency v1 覆盖完整 Catalog snapshot 历史；生产对象存储、compact proof 和
  witness/gossip 属于后续基础设施。
- [ ] 接入生产 HSM 和 key ceremony；当前隔离文件 key 参考 Signer 已保证 Web 服务不能读私钥。
  provider-neutral ceremony evidence v1 已冻结双人职责分离、无私钥字段、HSM/trust-update 摘要、
  key/sequence 切换和有界验证器；HSM 选型、真实 quorum 执行及独立审计仍未完成。
- [x] 实现确定性 Catalog Builder、sequence 分配、发布和紧急撤回。
  pause/resume/remove 生成更高 sequence，过期控制事件安全合并且 sequence 永不复用。
- [ ] 灾备、key rotation、权限提升和内部威胁测试。
  Catalog key rotation 的设备端交叠/切换/旧钥撤销语义和操作 runbook 已完成；生产 HSM 双人
  ceremony、签名 OS 信任根更新、离线设备 cohort、CDN promotion 与独立审计仍是外部门禁。

完成条件：已审核 submission 才能发布；任意对象变化都会要求新 revision 和新审核。

## S6：富媒体 Discovery Catalog

- [x] Catalog v2 增加 developer、subtitle、category、keywords、age/privacy 元数据。
  S6A 的生产 Publisher 已生成严格 v2，设备服务同时验证 v1/v2；当前 System Shell 仍使用有界
  summary 响应，富字段详情展示属于后续切片。
- [x] 定义 32x32/48x48 图标和 320x170 截图的格式、尺寸、摘要和缓存上限。
  S6B 已冻结 PNG/descriptor/details 契约、单资源与 CM0 总缓存预算，并由 Publisher 原子发布；
  `cp0-stored` 缓存实现仍由下方独立任务跟踪。
- [x] 增加 Today 专题和精选集合。
  S8A 通过 Catalog v4、严格 `today` IPC 和 System Shell 单前台集合导航完成；Updates 已由 S7
  使用 verified Catalog 与 appd 安装快照计算。
- [x] 增加签名分类索引。
  S6E 根签名绑定精确分类计数和 shard ordinal，设备从已验证应用集重算后才接受；`browse`
  IPC 和 `cp0ctl store browse` 提供 all/category 的 8 项分页。
- [x] 规模超过 64 应用时切换为签名根索引和有界 shard。
  实现同时处理更早达到 48 KiB 的目录，最多 16 个 shard/1024 项；缺失或篡改 shard 不切换缓存。
- [x] `cp0-stored` 原子缓存 Catalog/资源，资源解析失败不影响本地应用启动。
  S6C 在 Catalog 提交后尽力预取图标，details/截图按需缓存；截断、替换、错误尺寸、错误身份和
  不安全缓存 inode 均不会生成最终对象，且不回滚 Catalog 或阻断已验证包安装。
- [x] System Shell 增加图标、截图单页查看和权限差异展示。
  S6D 通过严格 details/media IPC 接入富详情；图片只以单个只读 `SCM_RIGHTS` 描述符传递，
  Shell 对应用/版本/类型/索引/尺寸/长度重新绑定并用 libpng 解码。五页详情覆盖图标、可滚动描述、
  320:170 单截图、升级权限差异和更新说明，UI 状态仍小于 64 KiB。
- [x] System Shell Apps/Search 通过有界页面访问完整 1024 项 Catalog。
  Apps 使用无分类 `browse`，Search 保持纯本地查询；两者切换时重新取页，不保留第二份应用数组。

完成条件：CDN 修改、截断或替换任一资源都会在设备端被拒绝。

## S7：下载、更新与恢复体验

S7A 已增加严格 `control` 协议和 `paused`/`canceled` 状态；暂停保留摘要命名分片，取消在
全局作业锁内删除分片，恢复绑定当前 Catalog 的应用版本和包摘要，appd handoff 后拒绝控制。
设备端、CLI 和 320x170 Shell 使用同一封闭失败原因，并覆盖目录变化、stale Catalog、竞态和
网络/存储/验证/安装器故障。具体契约见 `STORE-DOWNLOAD-CONTROL-V1.md`。
S7B 已增加 1 至 8 项的严格 `install-batch` 协议；daemon 在同一 Catalog 身份快照下原子接受、
串行执行整批任务，单项暂停、取消或失败不会阻塞后续项。Updates 页只收集可重试更新，排除
活动任务并在 stale Catalog 上禁止提交。具体契约见 `STORE-UPDATE-QUEUE-V1.md`。
S7C 已增加全局 Store 后台状态：Store 页面或活动任务期间每秒轮询，其它页面每五秒轮询；
Home、Tasks 和普通前台应用均可显示有界 `DL n%`、`INSTALL` 或 `QUEUE N` 状态。安装完成只由
同一 App ID/版本的状态转换生成，首次 Catalog 不回放历史通知，多项完成聚合且不抢占权限、文档
或确认界面。具体契约见 `STORE-BACKGROUND-STATUS-V1.md`。
S7D 已将安装改为强制两步预检：签名 Catalog sequence、完整应用对象和 60 秒单次授权绑定；
daemon 在下载前及授权消费时检查 root-owned 设备策略、allowlist、持久分区和 `/run` 峰值空间。
Shell 只对新增权限弹出默认 Cancel 的可信确认，并显示策略屏蔽权限及所需/可用空间；Resume 也会
重新预检策略和空间。具体契约见 `STORE-INSTALL-PREFLIGHT-V1.md`。
S7E 已补齐中断恢复门禁：摘要命名分片可跨服务重启继续，错误 HTTP Range 在写入前拒绝，错误
摘要会同步截断且绝不进入 appd；appd 对完整复验后的同版本同内容请求提供幂等重放，daemon 启动
只清理严格命名的陈旧交接文件。进程、协议和故障注入测试已完成，真实断电证据仍由 S9 真机门禁
采集。具体契约见 `STORE-INTERRUPTION-RECOVERY-V1.md`。
S7F 已增加默认关闭的自动应用更新：私有原子偏好和六小时限频跨 daemon 重启保留，仅在外部供电、
带默认路由的有线网络及 root-owned 独立策略同时允许时检查；候选必须是严格版本升级且不新增权限，
每批最多八项。appd 只向 Store UID 暴露最小安装快照，并在自动 handoff 时重复策略、签名、摘要和
版本复核。具体契约见 `STORE-AUTO-UPDATE-V1.md`。

- [x] 增加暂停、继续、取消和失败原因的稳定协议。
- [x] 增加 Updates 页、单项更新和有界 Update All 队列。
- [x] 增加下载状态栏、离开 Store 后进度和安装完成通知。
- [x] 增加新增权限确认、策略限制和存储空间预检。
- [x] 验证断电、断网、HTTP Range 错误、摘要错误和 appd handoff 崩溃恢复。
  本地门禁覆盖跨 daemon 实例和 appd 提交后断线；真实 CM0 断电/断网复验保留在 S9。
- [x] 自动更新保持默认关闭，并按外部供电、有线网络和独立设备策略显式启用。

完成条件：任何中断都不会生成半安装状态或绕过重新验证。

## S8：运营质量与隐私

S8A 已完成 Today editorial 控制面纵向切片：独立 operator token 域、role/2FA/state/expiry/
revocation/scope 校验，首次创建和 ETag 更新、精确幂等、不可变 revision、audit/outbox、Catalog v4
确定性投影和 v3 fail-closed 降级均通过真实 PostgreSQL 验收；设备 `cp0-stored` 与 System Shell
同步完成 Today/专题消费。具体契约见 `STORE-EDITORIAL-V1.md`。

S8B 已完成默认关闭的 Store 周聚合：设备只保留安装、启动和崩溃计数，不创建设备身份，
策略撤销或关闭同意会原子清空；失败重试复用随机批次，后端仅接受上一完整周和已发布精确版本，
公开聚合需至少 20 个批次。具体契约见 `STORE-METRICS-V1.md`。

S8C 已实现内容治理的非生产纵向切片：匿名结构化举报不接收自由文本、联系方式、设备身份、
IP 或 User-Agent；控制面提供有界 SLA 队列、结构化开发者通知和一次性申诉。当前 SLA 常量及
原因词表仅用于工程验收，生产启用、自动处置和最终政策文本必须通过产品/法务/安全批准。
具体契约见 `STORE-MODERATION-V1.md`。

S8D 已冻结搜索隐私边界：查询仅经本机 Unix socket 在已验证 Catalog 上执行，最近查询只存在于
Shell 进程内；设备状态、日志和严格聚合指标均不能承载查询。指标同意不授权实验，未来搜索实验
必须使用独立的默认关闭同意、字段白名单和明确保留期限。具体契约见
`STORE-SEARCH-PRIVACY-V1.md`。

S8E 已完成工程环境韧性演练：1024 项/16 shard 容量、CDN Range/离线/替换、Publisher 事务
回滚和 lease 重放、PostgreSQL 17 独立 dump/restore、append-only 恢复验证及文件 key signer
故障均有可重复证据。生产多区域演练与 HSM ceremony 仍分别受基础设施和 S5 HSM 门禁约束。
具体契约见 `STORE-RESILIENCE-DRILL-V1.md`。

- [x] Today/专题运营工具只能引用 approved Release。
- [x] 建立最小化、可选、去标识化的安装、启动和崩溃聚合指标。
- [x] 搜索词默认不上传；任何实验功能需要单独同意和保留期限。
- [ ] 建立内容举报、下架申诉、开发者通知和安全响应 SLA。
  S8C 先交付结构化举报、SLA 队列、通知与申诉 API；自动下架、双人处置批准、外部安全值班和
  生产 SLA 尚未完成，因此本项保持未关闭。
- [x] 进行容量、CDN 故障、数据库恢复、队列重放和签名服务演练。
- [ ] 独立隐私、安全和审核公平性评审。

## S9：真机门禁（稳定性结束后）

- [ ] 部署最新 Store 协议、`cp0-stored`、appd 和 System Shell。
- [ ] 验证 App Metrics 默认关闭、无待发数据，以及 appd 的单阻塞生命周期 observer。
- [ ] Camera2 验证 Today/Apps/Search/Updates、详情和下载进度。
- [ ] 完成刷新、续传、安装、升级、离线缓存和过期拒绝六步证据。
- [ ] 测量最大 Catalog 的内存、CPU、输入延迟和 SD 写入。
- [ ] 验证下载中 Home/Tasks/权限弹窗、重启和断电恢复。
- [ ] 取回全部证据并使用独立验证器复算后才能关闭 Store 产品门禁。
