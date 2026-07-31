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

- [ ] 建立账户、团队、角色、2FA 和 Developer Key 页面。
- [ ] 建立 App ID 创建、名称检查和永久所有权页面。
- [ ] 建立版本、Listing、本地化、图标/截图和隐私说明编辑器。
- [ ] 建立上传进度、自动检查、审核消息和版本状态时间线。
- [ ] 建立发布方式、预约、分阶段发布、暂停和撤回控制。
- [ ] 桌面和移动浏览器可访问性、键盘操作和错误恢复测试。

完成条件：Portal 不接触私钥，所有写操作幂等且可审计。

## S5：审核与发布后端

- [ ] 实现 Identity/Teams、App Registry、Submission 和 Release 服务。
- [ ] 实现隔离 Scan Worker：包格式、WASM、权限、资源和恶意样本检查。
- [ ] 实现 Review Console、结构化问题、回复、二审和双人审批。
- [ ] 实现不可变对象、事务 outbox、append-only audit 和 transparency log。
- [ ] 接入 HSM/隔离 Store Signer，任何 Web 服务都不能读取私钥。
- [ ] 实现确定性 Catalog Builder、sequence 分配、发布和紧急撤回。
- [ ] 灾备、key rotation、权限提升和内部威胁测试。

完成条件：已审核 submission 才能发布；任意对象变化都会要求新 revision 和新审核。

## S6：富媒体 Discovery Catalog

- [ ] Catalog v2 增加 developer、subtitle、category、keywords、age/privacy 元数据。
- [ ] 定义 32x32/48x48 图标和 320x170 截图的格式、尺寸、摘要和缓存上限。
- [ ] 增加 Today 专题、精选集合、分类和更新索引。
- [ ] 规模超过 64 应用时切换为签名根索引和有界 shard。
- [ ] `cp0-stored` 原子缓存 Catalog/资源，资源解析失败不影响本地应用启动。
- [ ] System Shell 增加图标、截图单页查看和权限差异展示。

完成条件：CDN 修改、截断或替换任一资源都会在设备端被拒绝。

## S7：下载、更新与恢复体验

- [ ] 增加暂停、继续、取消和失败原因的稳定协议。
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
