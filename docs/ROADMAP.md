# CardputerZero OS Roadmap

Roadmap 以“每个阶段都有可在真机验证的交付物”为原则。日期在完成 CM0 基准测试后
再估算，当前只冻结依赖关系和完成条件。

## Phase 0：架构与开发基线（进行中）

- [x] 建立独立 Git 仓库和 Rust workspace。
- [x] 冻结硬件约束、信任边界和内存预算。
- [x] 定义 app manifest v1、权限词表和校验器。
- [x] 建立 WIT SDK ABI 草案与示例应用清单。
- [x] 增加基础 CI、SDK/manifest 版本策略和架构测试。
- [ ] 在 WIT 工具链确定后加入 ABI 解析和兼容性测试。

完成条件：`make check` 可重复通过，manifest 和 ABI 的兼容规则有自动化测试。

## Phase 1：Board Support Package 与镜像

- 建立最小 Debian arm64 镜像，只包含 systemd、SSH 调试入口和硬件服务。
- 固化 CM0 V0.6 的 kernel config、device tree overlay 和模块版本。
- 将 ST7789V 迁移到 DRM/KMS，并验证 320x170 RGB565、damage update 和背光。
- 验证键盘、电池、音频、摄像头、LoRa/GPIO 的内核接口。
- 加入 zram、只读根文件系统原型和启动/内存基准采集。

完成条件：冷启动进入 DRM 测试画面，所有输入输出设备通过自动化 smoke test，空闲
内存低于 180 MB。

## Phase 2：Compositor 与 System Shell

- 集成 Weston kiosk shell，建立单前台窗口策略。
- 实现 Launcher、状态栏、系统弹窗、沉浸模式和崩溃返回首页。
- 统一键盘焦点、Home/Back/任务切换快捷键和屏幕休眠。
- 建立虚拟 320x170 PC 测试后端和截图回归测试。

完成条件：两个测试 Wayland 客户端可可靠切换，应用无法读取非焦点输入或覆盖系统
权限弹窗，连续运行 24 小时无 compositor 泄漏。

## Phase 3：应用运行时与隔离

- 集成 WAMR interpreter/AOT，App Runtime 成为受控 Wayland 客户端。
- 实现 `appd` 生命周期、每应用 UID、namespace、seccomp 和 cgroup。
- 实现 manifest 权限数据库及网络、文档、音频、相机和 LoRa/GPIO broker。
- 定义 Intent Broker 和应用私有存储配额。
- 建立恶意应用测试集，覆盖路径逃逸、设备访问、IPC 和资源耗尽。

完成条件：测试应用只能使用被授予的能力；拒绝权限后没有旁路；OOM/崩溃不会影响
Shell 和其他应用数据。

## Phase 4：SDK 与开发体验

- 发布 Rust 和 C/C++ SDK，封装 WIT bindings 与 LVGL 320x170 组件。
- 实现 `cp0ctl new/build/run/package/sign/install/logs`。
- 建立 PC 模拟器、权限模拟、输入映射和性能分析工具。
- 迁移 Calculator、Camera 等示例，不提供传统 Linux 应用兼容层。
- 冻结 SDK 1.0 ABI、兼容策略和开发者文档。

完成条件：新开发者可只使用 SDK 在 PC 编写、调试、签名并安装一个应用到真机。

## Phase 5：应用包与商店

- 完成 `.capp` 可复现打包、开发者签名、商店审核签名和吊销机制。
- 实现设备端商店、下载恢复、原子安装、升级和回滚。
- 建立静态扫描、权限审查、恶意样本测试和发布后台。
- 增加家长/组织策略、开发者模式开关和恢复模式。

完成条件：商店应用只能由可信签名安装，断电不会产生半安装状态，旧版本可回滚。

## Phase 6：产品化与后续安全

- 启动时间、功耗、SD 写放大、内存和刷屏性能优化。
- 故障遥测、隐私策略、恢复镜像和量产测试。
- 评估只读 rootfs、dm-verity、RAUC A/B、U-Boot 回滚及硬件信任根。
- 完成威胁模型审计、模糊测试和第三方安全评审。
