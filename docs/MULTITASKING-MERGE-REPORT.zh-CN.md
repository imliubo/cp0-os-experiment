# 多任务主集成报告

<!-- doc-locale: zh-CN -->
> [English](MULTITASKING-MERGE-REPORT.md) | **简体中文**

## 决策

多任务分支适用于将 `main` 作为经过仿真测试的架构、协议和UI基础进行集成。这不是一个设备发布完成的多任务实现。生产部署仍受MT3-MT6门控限制。

在这次集成中，没有接触任何设备，没有重启任何服务，也没有生成任何镜像或系统包。

## 集成身份

| 角色 | 分支或提交 |
| --- | --- |
| 已审主要基线 | `d0b8a0098c9d4b41b054842676ad625f043e4e91` |
| 多任务来源 | `97438636320f7ed175a0569c482cc0aae96332d4` 在 `codex/multitasking` 上 |
| 集成分支 | `codex/multitasking-main-integration` |
| 挑选集成提交 | `e950a4f0596f36ce9034d8f534f9414a6b8b93b1` |

审查涵盖了所有17个主线提交，包括多任务分支点后的首次启动、引导键盘诊断、Store集成、Owner开发者访问和受限系统电源控制。

## 冲突解决

梅丽克-挑选有兩個文本冲突：

- `system-shell/src/main.c`: 保留主程序的第一启动网络状态，然后集成开发人员访问和电源控制路径，接着进行任务轮询、激活、关闭和多表面跟踪；
- `tests/test-system-shell-ui.sh`: 保留了主线测试输入，并添加了多任务UI源和快照覆盖率。

appd 协议现在是 v2，私有 System Shell Wayland 协议是 v7。
appd 协议、System Shell、compositor 策略和 Runtime 必须作为一个版本化的包发布；混合端点在严格版本协商中失败。

## 审查发现的错误已修复

1. 合并的`struct cp0_ui`达到了66,232字节，并违反了64 KiB的产品限制。固定的十行任务表现在是一个880字节的堆分配，并带有显式的`cp0_ui_deinit()`销毁。
2. 一个检查点或崩溃的逻辑任务没有活跃的systemd单元，所以旧的`is_running()`门框可能会允许升级或卸载，即使该任务仍然引用了旧的包版本。现在，所有非幂等安装、升级、回滚和卸载路径都拒绝任何匹配的逻辑任务。精确版本的Store重放仍然有意图地幂等。
3. 重启后的 App UID 可能会暂时拥有旧的和新的表面令牌。
Shell 现在更偏好最近宣布的令牌。这关闭了本地选择的回归，但不是生产身份解决方案；MT3 必须在 compositor 中绑定 `(task_id, runtime_generation)`。

合并也会保留 Intent 发送者作为后台任务，而不是在成功移交后停止它们。

## 实施的基础

- 一个前台应用和一个键盘焦点；
- 每个App最多一个任务，最多十个逻辑任务；
- App 11 的创建顺序先进先出淘汰策略，独立的 MRU 卡排序；
- 前台, 后台, 冻结, 暂停点保存和崩溃任务状态；
- appd 协议 v2 列表，激活和关闭操作；
- 多个 Runtime 会话记录，支持代际安全退出处理；
- F3 固定大小的 160x85 任务卡片，键盘导航和占位状态；
- 版本化的原子任务日志，有界的检查点封装，受信任的缩略图缓存模型和确定性的资源管理模型；
- 可选的 C、Rust 和 WIT 生命周期 ABI 用于有界检查点/恢复。

任务日志启动恢复、Runtime 控制、真实检查点回调，
 compositor 截图和压力策略在这一切片中仅是模型相关或未连接的。占位符任务卡片不是实际设备缩略图的证据。

## 安全审核

该集成保留了每个应用的 UID、命名空间、cgroup、seccomp 和 broker 边界。后台执行不授予进程、cgroup 或 compositor 控制权。Shell 继续验证 compositor 对等体，并使用内核观察到的 App UID 而不是客户端提供的 app-id 文本。

UID 只适用于当前一个任务对应一个 App 的模拟。它无法区分 stale 表面和同一个 App 重启的 Runtime。因此，生产激活和缩略图交付在 Runtime 和 compositor 都验证任务 ID 和运行时生成之前会失败。

检查点载荷保持应用私有，限制在8 KiB以内，带版本和哈希。
SDK 不暴露预留的检查点命名空间，超时或无效载荷无法防止FIFO被驱逐。

## 开发者模式热更新评估

Owner Developer Access 可以安装已签名的 `.capp` 包，并代理有界的应用生命周期命令。它不能替换 appd、System Shell、compositor policy、Runtime、systemd unit 或 OS 镜像，因此不能热更新多任务所需的系统组件。

MT3-MT5 实现后，设备集成需要经过所有者授权的协调系统包和正常重启，或者新刷的镜像。开发者模式可以安装 SDK 测试应用，但不能自己建立多任务系统基线。

## 验证矩阵

| 检查 | 结果 |
| --- | --- |
| `git diff --check` | 通过 |
| `cargo fmt --all -- --check` | 通过 |
| `cargo check --workspace --all-targets` | 通过 |
| `cargo test -p cp0-appd` | 通过，117项测试 |
| `cargo test -p cp0-sdk` | 通过，18项测试 |
| `tests/test-system-shell-ui.sh` | 通过 |
| `tests/test-appd-profile.sh` | 通过 |
| `tests/test-compositor-profile.sh` | 通过 |
| `tests/test-developer-access.sh` | 通过 |
| `tests/test-power-control.sh` | 通过 |
| `tests/test-device-deployment.sh` | 通过 |
| `tests/test-malicious-apps.sh` | 通过 |
| `tests/test-security-validation.sh` | 通过 |
| `tests/test-patch-cm0-dtb.sh` | 通过 |
| ARM64 System Shell 和 compositor 策略完整链接 | PASS |
| `make check` 排除本地监听器 Store 原始情况 | PASS |

ARM64 链接使用了仓库固定好的 Weston 14 输入。制品哈希值：

- System Shell: `4f149fa3d9a036f6c24ebb45bd2bf33e118736d69785d263b97826528fb470b0`;
- compositor 策略: `130b98fedf8bce9bc616381b582aad88c7742e46d308b1bfd29269fd1ed9227f`.

两个 Store 原始检查不能在受管主机沙箱中运行，因为绑定 `127.0.0.1` 返回 `EPERM`：

- `tests/test-store-origin.sh`；
- `cp0-stored::tests::rejects_mismatched_http_range_without_appending_and_then_recovers`.

这些都是环境限制，不是多任务失败。Store代码没有改变以绕过监听器限制。

Tasks 和 Notifications 的 320x170 模拟快照保持稳定：

- 任务: `879c45ff089f2ef29fbbeb019199dfd4797d06c9bd4e01590b47d1c381f95d80`;
- 通知: `9339f99f3b7134f1df3089248ecfacc60a7f461a01971eda10c780360ac2f1ec`.

## 释放门

- MT3 / 第6L-C阶段：连接任务日志启动校正，经过身份验证的运行时控制和 compositor `(task_id, runtime_generation)` 绑定。
- **MT4 / Phase 6L-D:** 将 compositor 所有的 RGB565 缩略图捕获到密封的只读对象中，强制执行每 2 Hz 更新，并通过过时/伪造身份测试。
- MT5 / Phase 6L-E-F：带电线和截止日期约束的WAMR检查点/恢复，
私有代理持久性，App 11 先入先出行为和CM0资源阈值，用1个、3个和10个App测量。
- **MT6 / Phase 6L-G:** 只在授权后部署一个完整的包，然后正常重启，接着验证F3、Intent、开发者访问生命周期、App 11、appd重启、持久性和断电恢复，再发布镜像。

在所有门禁通过前，不得将该分支描述为已提供实时任务缩略图、持久的设备恢复或量产级 CM0 后台策略。

## 合并建议

将集成分支合并到`main`作为Phase 6L-A/B的基础，同时保持Phase 6L-C到6L-G开放。只有在确认`main`仍然指向已审核的基础线之后，才安全地进行快进合并或普通合并；如果它已经向前发展了，就需要将其重新应用或合并到集成工作区，并重新运行矩阵。不要独立地将此提交部署到设备上，因为v2/v7协议端点故意与之前的捆绑包不兼容。
