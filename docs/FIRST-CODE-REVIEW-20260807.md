# CardputerZero-OS 首次代码审查报告

- 审查日期：2026-08-07（Asia/Shanghai）
- 审查基线：`main@15e7f29bafc72fbf72e6e3afe4b4289a92f6778a`
- 审查类型：首次全仓库需求驱动审查
- 结论：未发现 P0；确认 4 项 P1、4 项 P2 和 2 项 P3。4 项 P1 均已有代码修复，
  CR-004 在 2026-08-07 真机复验后追加了方向/色彩/速度修复，仍等待新镜像复验；
  P2/P3 中需要真机、生产基础设施或产品决策的事项保持开放。

## 1. 范围、依据与方法

本轮覆盖 Rust workspace、System Shell C 代码、镜像和 Shell 门禁、GitHub
Actions、三个 Web 控制台、Store 服务及其依赖。它不是逐行形式化审计，也不把已有
实现等同于真机或生产验收。

需求和发布边界以以下文件为准：

- `docs/ROADMAP.md`：阶段完成条件和真机门禁；
- `docs/REMAINING-ROADMAP-AUDIT.md`：实现证据与外部验收的边界；
- `docs/ARCHITECTURE.md`、`docs/THREAT-MODEL.md`：信任边界与发布阻断项；
- `docs/PHASE6E-SECURITY-VALIDATION.md`：内部安全基线要求 `make check`、
  workspace Clippy 和 clean diff；
- `docs/SYSTEM-EXPERIENCE-ROADMAP.md`、`docs/HOME-SYSTEM-APPS-ROADMAP.md`、
  `docs/STORE-ROADMAP.md`：系统体验、系统应用和 Store 的未关闭条件；
- ADR 0006、0007、0008：更新/回滚、首次启动和早期 splash 的已接受决策。

审查方法包括差异与调用路径阅读、危险 C 解析路径检查、固定工具链构建、主门禁和
Web 门禁执行、RustSec/npm 依赖审计，以及 roadmap 与实现状态交叉核对。

严重性定义：P0 为可立即导致发布阻断或关键安全失陷；P1 为确定的正确性、启动或
持续集成门禁缺陷；P2 为发布前必须关闭或明确接受的风险；P3 为可维护性和部署前
加固事项。

## 2. 发现与处置

### CR-001 [P1 已修复] System Shell 截断设置文件触发未初始化读取

**需求依据**：Settings 必须通过可信 Shell 和 broker 应用设置；无效或损坏的持久化
状态必须 fail closed，且不能污染当前设置。

**证据与影响**：原 `cp0_shell_settings_load()` 在 `fscanf()` 只解析部分字段时，先用
未赋值的 `theme`、`timeout`、`key_sounds` 构造候选结构体，再判断解析结果。读取未
初始化的自动变量属于 C 未定义行为。截断或掉电损坏的 `settings.conf` 可造成不确定
设置、错误返回，理论上也可能被优化器放大。

**方案与代码**：在 `system-shell/src/shell_settings.c:39-54` 保存精确解析字段数，先
验证 `parsed == 4`、`fclose()`、版本和布尔范围，再构造候选结构体。失败时不修改调用
方输出。`tests/system-shell-settings.c:24-36` 新增仅含版本字段的截断文件回归测试。

**验证**：`./tests/test-system-shell-ui.sh` 通过；完整 `make check` 覆盖该测试。

### CR-002 [P1 已修复] 主 CI 未执行仓库定义的验收边界

**需求依据**：`docs/PHASE6E-SECURITY-VALIDATION.md:46-51` 明确要求内部验收执行
`make check`、workspace Clippy 和 clean diff；`docs/THREAT-MODEL.md:103-113` 将
`make check` 定义为 schema、协议、包、沙箱、权限、恶意应用和恢复等安全映射。

**证据与影响**：原 `.github/workflows/ci.yml` 只检查格式、两个 JSON 和
`cargo test`。镜像 profile、生产访问边界、compositor/Shell/appd 协议、SDK ABI、
恶意应用、安全验证、Store 协议以及三个 Web 控制台均可在 PR 中回归而保持 CI 绿色。

**方案与代码**：`.github/workflows/ci.yml:13-40` 现在安装固定 Rust 1.85.1、
`wasm32-unknown-unknown`、Clippy/rustfmt 和 Node 22，安装三个锁定的 npm 依赖集，执行
完整 `make check`、`clippy::correctness` 阻断、三个 Web `check` 和
`git diff --check`。`Makefile:74-78` 为 workspace check/test 增加 `--locked`，防止
验证时偏离 `Cargo.lock`。

没有使用 `-D warnings`：当前 warning 中包含接口复杂度、可读性和示例目标的非正确性
技术债。把它们一次性升级为发布阻断会扩大本轮改动面；本轮只阻断 correctness 类别。

**验证**：固定工具链 Clippy 通过；完整 `make check` 和三个 Web check 通过。

### CR-003 [P1 已修复] audiod 不兼容仓库声明的 Rust 1.85

**需求依据**：`Cargo.toml:53` 声明 `rust-version = "1.85"`，DevKit 与文档固定
Rust 1.85.1，`.github/workflows/devkit.yml:24-27` 也使用该版本。

**证据与影响**：`cp0-audiod` 使用 `if let ... && let ...` let-chain。它在较新本机
工具链可编译，但 Rust 1.85.1 报 `E0658`，因此固定工具链 CI 和声明的最低版本构建
失败。该问题也说明此前没有在主 CI 上验证 MSRV。

**方案与代码**：`crates/cp0-audiod/src/lib.rs:881-894` 改为语义等价的两层
`if let`，保留持久化失败时返回 Internal、且不更新内存状态的原行为。

**验证**：`cargo +1.85.1 clippy --workspace --all-targets --locked --
-D clippy::correctness` 通过；`cargo +1.85.1 test -p cp0-audiod --locked`
10/10 通过。

### CR-004 [P1 代码修复待真机复验] 早期 splash 启动、方向与色彩错误

**需求依据**：ADR 0008 要求 splash 失败不能阻止标准 initramfs root discovery 和
Home 启动；V0.6 是 BCM2837 平台。

**证据与影响**：直接 SPI helper 原使用 BCM2835 的 `0x20000000` peripheral base，
且 RX FIFO drain 无独立次数上限。寄存器映射或 SPI 状态异常可能令早期启动路径卡住。

**方案与代码**：提交 `15e7f29` 将
`image/pi-gen/stage-cardputerzero-os/00-bsp/files/early-splash-spi.c` 改为 BCM2837
`0x3f000000` base，为 helper 和 RX drain 增加边界；
`early-splash-initramfs` 再以 BusyBox `timeout -s KILL 2` 包围 helper。镜像 profile
测试锁定地址、双重超时和有界 drain。本项已在审查基线中，不属于当前未提交差异。

**验证**：`tests/test-image-profile.sh` 和完整 `make check` 通过。未执行需要成品镜像的
`tests/test-built-rootfs-profile.sh`；仍需在下次产品镜像真机启动时确认实际像素和启动
时序。

**2026-08-07 真机跟进**：启动阻塞已关闭，但 direct-SPI 第一次显示上下反转、颜色错误，
随后 DRM framebuffer 重绘才正确。根因是实验原型的 `MADCTL=0x60` 和最小初始化序列只
验证过纯色；固定 BSP 的真实配置使用 `MADCTL=0xa0`、完整 power/gamma 参数和 display
inversion。当前未提交修复位于
`image/pi-gen/stage-cardputerzero-os/00-bsp/files/early-splash-spi.c`：同步 BSP 配置，
并将逐字节 RX drain 改为有界 TX/RX FIFO 流式泵送。`tests/test-image-profile.sh` 已锁定
新方向、色彩初始化和传输结构。host 门禁通过后仍需烧录新镜像复验冷启动方向、颜色、
首次显示时间及 DRM 接管跳变。

### CR-005 [P2 已加固] GitHub Actions 依赖使用可移动标签

**需求依据**：威胁模型 `SUPPLY-01` 要求固定构建输入；workflow 本身处于发布和 fuzz
产物信任链中。

**证据与影响**：三个 workflow 使用 `actions/checkout@v4`、
`actions/setup-node@v4`、`actions/upload-artifact@v4`。标签可被上游移动，无法把一次
构建严格绑定到已审查 action 内容。

**方案与代码**：`.github/workflows/ci.yml`、`devkit.yml`、`fuzz.yml` 已固定为：

- `actions/checkout@11d5960a326750d5838078e36cf38b85af677262`（v4）；
- `actions/setup-node@49933ea5288caeca8642d1e84afbd3f7d6820020`（v4）；
- `actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02`（v4）。

保留版本注释便于 Dependabot/人工升级。后续每次更新 SHA 仍需审查 release notes。

### CR-006 [P2 开放] Store PostgreSQL 纵向验收不在默认门禁中

**需求依据**：Store roadmap 以 PostgreSQL 17 纵向测试作为身份、审核、发布、扫描和
Catalog 状态机的证据，不能用内存 core 测试替代。

**证据与影响**：以下五个集成测试均因需要 `CP0_STORE_TEST_DATABASE_URL` 而默认
`#[ignore]`，`make check` 不会运行它们：

- `crates/cp0-store-control-server/tests/postgres.rs:75`；
- `crates/cp0-store-portal-server/tests/postgres.rs:121`；
- `crates/cp0-store-workforce-server/tests/postgres.rs:122`；
- `crates/cp0-store-scan-worker/tests/postgres.rs:19`；
- `crates/cp0-store-publisher/tests/postgres.rs:30`。

因此本轮只能确认代码可编译，不能声明当前 commit 已重新通过真实数据库事务、约束和
HTTP 纵向验收。

**方案**：增加独立 CI job，使用固定 digest 的临时 PostgreSQL 17 服务、随机测试库和
无生产凭据的 `CP0_STORE_TEST_DATABASE_URL`，执行 `make store-control-db-check`，并将
失败设为合并阻断。生产迁移仍需另行演练。当前未修改 workflow，因为仓库尚未冻结
PostgreSQL CI 镜像 digest、资源预算和运行频率；这三项应先由维护者确认。

### CR-007 [P2 已知依赖告警] `rsa 0.9.10` 命中 RUSTSEC-2023-0071

**证据**：`cargo-audit` 扫描 `Cargo.lock` 的 376 个依赖，报告 Marvin timing attack，
CVSS 5.9 medium，且当前无修复版本。路径为
`rsa -> openidconnect -> cp0-store-portal-server -> cp0-store-workforce-server`。

**可利用性判断**：当前代码通过 OIDC/JWKS 使用 RSA 公钥验证 JWT，不持有 RSA 私钥，
也不执行 RSA 私钥解密。该 advisory 的密钥恢复前提在当前服务路径不可达，故不是当前
P0/P1，但不能静默忽略。

**方案**：跟踪 `openidconnect`/`rsa` 上游；升级可用后在 PostgreSQL/OIDC 测试中复验
RS256。若未来引入 RSA 私钥操作，必须在合入前重新提升风险等级。不要为消除扫描结果
而擅自移除产品既定的 RS256 兼容性。

### CR-008 [P2 发布阻断] 关键真机和生产安全证据尚未关闭

这不是单个代码 bug，但会阻止“生产完成”声明：

- `docs/ROADMAP.md:60` 的 Phase 2 24 小时 compositor/Shell/appd 稳定性、内存和
  SD 写入证据仍开放；
- 首次启动、Developer Access、系统电源控制、Store S9、Owner USB Media、连续音频
  与按键音共存仍有 V0.6 真机门禁；
- `docs/THREAT-MODEL.md:89-101` 明确当前无可信 verified boot、数据静态加密、独立
  安全评审和 production USB VID/PID；
- ADR 0006 中的 dm-verity、RAUC A/B、U-Boot/FIT 和硬件信任根仅是接受的架构，尚未
  获得备用硬件故障注入和回滚证据；
- Store 的生产 HSM/key ceremony、IdP/JWKS、CDN、多区域恢复、正式治理政策和第三方
  安全/公平性评审仍依赖真实基础设施与负责人决策。

**方案**：保持 roadmap checkbox 开放，按硬件接受脚本收集完整 run directory、持续
时间、失败数、重启/内存/SD 写入摘要和独立验证器输出；生产基础设施由产品、安全和
运维共同冻结后再实现。不得用本轮 host 测试替代这些证据。

### CR-009 [P3 开放] 多任务仍是模型/协议基础，不是完整运行链路

**证据**：`docs/MULTITASKING-MERGE-REPORT.md:71-85` 明确 TaskJournal 启动恢复、
Runtime 控制、真实 checkpoint callback、compositor capture 和压力策略未连接；仅以 UID
认证不能区分同一 App 重启后的陈旧 surface。

**影响**：当前不能宣称提供实时缩略图、可靠设备恢复或生产 CM0 后台策略。

**方案**：严格按 ROADMAP Phase 6L-C 至 6L-G 推进：绑定
`(task_id, runtime_generation)`，连接 journal/reconciliation，增加 compositor 密封
缩略图与 2 Hz 限频，接入有 fuel/deadline 限制的 WAMR checkpoint，再用 1/3/10 App
和掉电场景完成真机验收。本轮不为未冻结的运行时协议补写实现。

### CR-010 [P3 开放] Web 部署与通用静态质量策略尚未冻结

三个 Vite 控制台均设置 `sourcemap: true`。这对工程调试有价值，但若将 `.map` 公开
部署，会暴露源代码结构和内部路径；仓库也尚未实现生产域名、TLS/CSP 和真实 IdP/JWKS
部署契约。此外标准 Clippy 仍报告 precedence、large enum variant、type complexity、
`Result<_, ()>`、不必要 cast 和低效 hex formatting 等非 correctness 告警；代码中
`unwrap`/`expect` 的统一生产策略也未冻结。

**方案**：部署设计确定后，将 sourcemap 作为私有错误追踪产物或在生产 build 关闭，
并用 CSP/headers 集成测试锁定。另建技术债批次按 crate 清理 Clippy；优先处理协议位
运算括号和服务错误类型，再讨论大 enum/API 变更。本轮不改变部署行为或公共接口。

## 3. 修改位置汇总

| 修改 | 位置 | 状态 |
| --- | --- | --- |
| 截断设置文件 fail closed | `system-shell/src/shell_settings.c`、`tests/system-shell-settings.c` | 未提交，已验证 |
| 主 CI 扩展到仓库/Web/Clippy 门禁 | `.github/workflows/ci.yml`、`Makefile` | 未提交，已验证 |
| Actions 固定 commit SHA | `.github/workflows/ci.yml`、`devkit.yml`、`fuzz.yml` | 未提交，已验证 |
| Rust 1.85 audiod 兼容 | `crates/cp0-audiod/src/lib.rs` | 未提交，已验证 |
| 早期 splash 有界失败与正确外设地址 | commit `15e7f29` 的 6 个文件 | 已提交，启动阻塞真机关闭 |
| 早期 splash 方向、色彩初始化与 FIFO 传输 | `early-splash-spi.c`、`test-image-profile.sh`、ADR 0008、`PHASE1-BSP.md` | 未提交，host 门禁通过，待真机复验 |
| 首次审查归档 | `docs/FIRST-CODE-REVIEW-20260807.md` | 本文 |

## 4. 验证结果

| 检查 | 结果 |
| --- | --- |
| `make check`（沙箱外，允许 localhost listener） | PASS |
| Rust 1.85.1 workspace Clippy，`-D clippy::correctness` | PASS |
| `cargo +1.85.1 test -p cp0-audiod --locked` | PASS，10 tests |
| `./tests/test-system-shell-ui.sh` | PASS |
| Developer Portal `npm test` + production build | PASS |
| Review Console `npm test` + production build | PASS |
| Store Operations `npm test` + production build | PASS |
| 三个 npm lockfile audit | PASS，0 vulnerabilities |
| GitHub workflow YAML parse | PASS |
| `cargo fmt --all -- --check` | PASS |
| `git diff --check` | PASS |
| `cargo-audit` | FAIL（1 个已分析告警，见 CR-007） |

完整 `make check` 在沙箱外执行，是因为 `tests/test-store-origin.sh` 和
`cp0-stored` HTTP Range 测试需要监听 `127.0.0.1`；受管沙箱会以 `EPERM` 拒绝监听，
该限制不是产品失败。沙箱外两项均通过。

## 5. 未执行与声明边界

本轮未执行 PostgreSQL 17 ignored tests、长时间 fuzz campaign、完整 Docker
compositor builder、成品 rootfs/压缩镜像挂载检查，也没有可用的 V0.6 真机、测试 Store
端点、生产 IdP/JWKS/CDN/HSM 或外部功率计。因此报告只证明当前 host 层代码和门禁
状态，不构成产品镜像、硬件稳定性或生产安全认证。

## 6. 后续顺序

1. 合入本报告中的未提交 P1/P2 修复，并观察 GitHub 主 CI 在 Ubuntu 上首轮通过。
2. 冻结 PostgreSQL 17 CI service digest/预算，关闭 CR-006。
3. 完成 Phase 2 24 小时证据后，按 roadmap 顺序执行首次启动、Developer Access、
   Power、Store S9、USB Media、音频和性能真机门禁。
4. 在协议与真机基础稳定后推进多任务 6L-C 至 6L-G，不提前声明完整多任务。
5. 生产发布前完成 verified update/boot 决策落地、HSM/IdP/CDN/治理基础设施和独立
   安全评审；对 CR-007 上游状态做每次 release 复核。

本报告是第一次 review 的基线。后续发现应保留 `CR-NNN` 编号、状态变化、修复 commit
和验收证据，避免在 roadmap checkbox 与代码实现之间丢失追踪关系。
