# Remaining Roadmap Audit

本文件记录 2026-08-01 的证据审计，不用“已有实现”替代“已经完成真机/外部验收”。
总 Roadmap 仍是最终状态来源；本文件按当前是否可以在不接触真机的条件下推进来分组。

## 1. 可以继续本地实施

### System Experience

- appd 应用详情、安装时间、包/数据大小和卸载协议正在并行开发；
- Settings 分组、应用管理、硬件诊断和官方 Fn 快捷键仍未完成；
- 最大文本、禁用能力、确认弹窗和完整像素回归仍需补齐；
- ARM64 compositor 构建和 `make check` 必须在每个本地里程碑通过。

### Store 产品化

- Developer Portal 本地前端 MVP 已完成；Review Console、Store Operations 和真实控制面接入尚未实现；
- Submission/Review/Release 事务域核心已完成；App Registry、Submission
  create/upload/finalize/read、单审核员 Review 和 Release 控制的 PostgreSQL/HTTP 纵向切片
  已完成，Identity/Teams、OAuth、其余 Submission、二审/双人审批、生产对象存储/GC、动态
  恶意样本和生产 HSM/key ceremony 尚未实现；隔离 Scanner、文件 key Publisher/Catalog
  Builder 与完整前缀 transparency log 的 outbox/租约、签名、失败恢复和原子结果纵向切片已完成；
- 设备端 Today/Apps/Search/Updates、分页搜索、图标/截图和更新队列尚未实现；
- 后端可从 approved Release 自动发布，Portal 的真实身份/OAuth 接入仍未形成完整自助流程；
- 当前 Store 安全底座可以复用，详细阶段见 `STORE-ROADMAP.md`。

### 产品安全与发布工程

- A/B、verity 和签名启动已有离线策略/模型，但真实 boot chain 集成仍需备用介质；
- recovery、production access 和三分区 profile 的构建/静态门禁可以继续强化；
- 量产 runbook、key ceremony、Catalog key rotation 和灾备演练文档仍可本地完成。

## 2. 当前 24 小时运行结束后才能触碰唯一真机

- 取回并独立验证 compositor/Shell/appd/stored 连续运行、内存增长和 SD 写入证据；
- 部署最新 Home、稳定性硬互锁、Phase 6F 限制和 Store 本地改动；
- 正常重启并确认 Home、开机时序和服务连续性；
- 执行 factory、performance、capability full 和 persistence-only 证据；
- 完成音频播放/录音/拒绝权限真机验收；
- 完成 GPIO 读写/拒绝权限/sysfs 收紧真机验收；
- 完成私有存储持久化、配额和跨应用隔离真机验收；
- 完成 Store 刷新、续传、安装、升级、离线和过期拒绝六步验收。

当前正式运行是
`/run/cardputerzero-stability/acceptance/20260731T170228Z-10620`，开始于
2026-08-01 01:02:28 CST。在结果完整取回前不得部署、重启或启动应用。

## 3. 需要用户提供或确认外设

- 相机 broker：需要连接与 V0.6 兼容的传感器，验证方向、捕获和权限拒绝；
- LoRa broker：需要 SX1276 模块，并由产品负责人确认部署地区和合法频点；
- 功耗：需要校准的外部 USB 功率计，定义亮度、网络、空闲和应用负载工况。

这些项目不能用模拟器或板载电池 gauge 代替最终证据。

## 4. 需要重新烧录或备用介质

- 三分区产品候选：首次扩容、重启持久性、断电恢复和 24 小时 SD 写入；
- recovery 介质：启动、备份、恢复、出厂重置和工位物理 I/O；
- production access 候选：确认 SSH、tty、sudo、开发者模式和产品内恢复入口均关闭；
- A/B、dm-verity、签名 FIT、故障注入和自动回滚：必须使用备用硬件/可擦写 SD；
- 唯一 V0.6 设备不得写入不可逆 OTP 状态。

需要烧录时必须先生成明确的镜像文件、profile、SHA-256 和验收步骤，再暂停等待用户协助。

## 5. 外部组织门禁

- 第三方安全评审尚未委托；
- 内容政策、隐私政策、审核申诉和开发者协议需要产品/法务确认；
- Store HSM/key ceremony、生产域名、CDN、备份地域和运营值班需要真实基础设施决策。

## 6. 阶段结论

| Phase | 实现状态 | 尚缺的完成证据 |
| --- | --- | --- |
| Phase 2 | 核心窗口/可信 UI/输入已实现 | 24 小时证据、扩展系统体验和最终真机部署 |
| Phase 3 | Runtime、sandbox 和能力 broker 已实现 | 音频/GPIO/存储/相机/LoRa 的对应真机门禁 |
| Phase 4 | SDK 1.0、CLI、模拟器和 DevKit 已实现 | 后续 Store submit CLI 属 Store 产品阶段 |
| Phase 5 | 双签名、原子安装、静态 Store、隔离扫描、单审核和 Release 控制纵向切片已实现 | OAuth、二审、隔离签名/Catalog 发布、搜索发现和六步真机证据 |
| Phase 6 | profile、验证器和安全工具已实现 | 性能/功耗、烧录介质、A/B 硬件和第三方评审 |
