# CardputerZero OS Roadmap

Roadmap 以“每个阶段都有可在真机验证的交付物”为原则。日期在完成 CM0 基准测试后
再估算，当前只冻结依赖关系和完成条件。

## Phase 0：架构与开发基线（基础完成）

- [x] 建立独立 Git 仓库和 Rust workspace。
- [x] 冻结硬件约束、信任边界和内存预算。
- [x] 定义 app manifest v1、权限词表和校验器。
- [x] 建立 WIT SDK ABI 草案与示例应用清单。
- [x] 增加基础 CI、SDK/manifest 版本策略和架构测试。
- [ ] 在 WIT 工具链确定后加入 ABI 解析和兼容性测试。

完成条件：`make check` 可重复通过，manifest 和 ABI 的兼容规则有自动化测试。

## Phase 1：Board Support Package 与镜像

- [x] 采集 CM0 V0.6 当前镜像、内存、DRM、输入、音频、电池和服务基线。
- [x] 确认 ST7789V 已使用 DRM/KMS MIPI-DBI，验证 320x170 RGB565 connector。
- [x] 固定 BSP 源码提交、V0.6 overlay、kernel module 构建入口和启动参数。
- [x] 建立最小 Debian arm64 `pi-gen` 外部阶段和服务裁剪策略。
- [x] 建立可回滚启动配置安装器与真机 smoke test。
- [x] 在现有系统真机验证 memory cgroup、AppArmor 和全部设备接口。
- [x] 构建首个无 Launcher/桌面环境的精简镜像并完成离线内容验收。
- [x] 烧录精简镜像，验证 64 MB GPU/CMA，并测量启动时间、空闲内存和 SD 写入。
- [ ] 加入只读根文件系统原型。

完成条件：冷启动进入 DRM 测试画面，所有输入输出设备通过自动化 smoke test，系统
识别至少 400 MB RAM，首页前的基础系统空闲内存低于 180 MB。

## Phase 2：Compositor 与 System Shell

- [x] 固定并裁剪构建 Weston，真机验证 DRM/Pixman kiosk 与单前台测试客户端。
- [x] 使用专用 seat 隔离内部 LCD/键盘，并建立 320x170 headless 测试后端。
- [x] 实现 Phase 2A 原生 Wayland System Shell：首页、状态栏、系统电源弹窗骨架、
  双缓冲 renderer、定时状态刷新和崩溃自动返回首页。
- [ ] 实现 Launcher、状态栏、系统弹窗、沉浸模式和崩溃返回首页。
- [ ] 统一键盘焦点、Home/Back/任务切换快捷键和屏幕休眠。
- [ ] 建立截图回归测试并完成两个客户端的可靠切换。

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
