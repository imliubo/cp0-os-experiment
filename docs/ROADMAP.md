# CardputerZero OS Roadmap

Roadmap 以“每个阶段都有可在真机验证的交付物”为原则。日期在完成 CM0 基准测试后
再估算，当前只冻结依赖关系和完成条件。

## Phase 0：架构与开发基线（基础完成）

- [x] 建立独立 Git 仓库和 Rust workspace。
- [x] 冻结硬件约束、信任边界和内存预算。
- [x] 定义 app manifest v1、权限词表和校验器。
- [x] 建立 WIT SDK ABI 草案与示例应用清单。
- [x] 增加基础 CI、SDK/manifest 版本策略和架构测试。
- [x] 加入扁平 ABI 契约解析、三端生成和已发布签名兼容性测试。
- [x] 使用 `wasm-tools` 同源标准 `wit-parser` 完成 WIT 全量语法与名称解析。

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
- [x] 加入 OverlayFS 只读底层根文件系统、initramfs fail-closed 启动和启动后挂载
  自检；三分区产品镜像默认启用，显式移除内核参数时进入可写恢复模式。

完成条件：冷启动进入 DRM 测试画面，所有输入输出设备通过自动化 smoke test，系统
识别至少 400 MB RAM，首页前的基础系统空闲内存低于 180 MB。

## Phase 2：Compositor 与 System Shell

- [x] 固定并裁剪构建 Weston，真机验证 DRM/Pixman kiosk 与单前台测试客户端。
- [x] 使用专用 seat 隔离内部 LCD/键盘，并建立 320x170 headless 测试后端。
- [x] 实现 Phase 2A 原生 Wayland System Shell：首页、状态栏、系统电源弹窗骨架、
  双缓冲 renderer、定时状态刷新和崩溃自动返回首页。
- [x] 实现 Phase 2B Weston policy 核心：独立 compositor/Shell UID、peer UID
  认证、可信层和全局 Home/Back/Tasks/Power 动作通道。
- [x] 将 Launcher、状态栏和权限弹窗迁移到 compositor 侧可信覆盖层，并实现沉浸模式。
- [x] 在 compositor 统一键盘焦点、全局 Home/Back/任务切换快捷键和屏幕休眠。
- [x] Launcher 从 appd 枚举已安装应用，支持启动后自动前台化，以及 Tasks 恢复/停止。
- [x] 实现可信通知横幅、应用焦点保持、权限弹窗优先级和通知像素回归。
- [x] 建立像素级截图回归测试并完成两个客户端的可靠切换与 200 轮压力测试。
- [x] 建立核心服务 SIGKILL 恢复测试和 RAM-backed 24 小时稳定性/内存采样工具。
- [x] 完成 Launcher 启动、Tasks 停止及 F1/F2/F3/F4 全局动作的最终物理按键验收。
- [ ] 完成 24 小时 compositor/Shell/appd 真机稳定性、内存泄漏及 SD 写入验收；
  先前 RAM-backed 测试因设备重启失效，部署当前平台后必须重新开始完整周期。

完成条件：两个测试 Wayland 客户端可可靠切换，应用无法读取非焦点输入或覆盖系统
权限弹窗，连续运行 24 小时无 compositor 泄漏。

## Phase 3：应用运行时与隔离

- [x] 集成 WAMR interpreter/AOT，App Runtime 成为受控 Wayland 客户端。
- [x] 实现 `appd` 生命周期、每应用 UID、namespace、seccomp 和 cgroup。
- [x] 冻结首版 `appd` 沙箱启动契约：稳定应用账户、systemd cgroup、bubblewrap
  namespace、只读应用包和唯一可写私有数据目录。
- [x] 固定并静态构建 WAMR 2.4.5，真机贯通每应用 UID、systemd cgroup、
  bubblewrap namespace、runtime seccomp 和 WASM 执行。
- [x] 实现 socket-activated `appd` 核心生命周期：可信安装注册表、规范路径解析、
  root/Shell peer UID 认证、单运行槽、分页 list 及 start/stop 真机闭环。
- [x] 将 appd、双 socket、静态 Runtime、稳定测试 UID/注册表和 SDK 示例集成到
  `pi-gen`，新开发镜像默认启动 compositor/Shell。
- [x] 实现 manifest 权限数据库、会话/持久决策和可信 Shell 提示控制协议。
- [x] 实现首个 `notifications.post` typed capability broker、peer UID 身份绑定和
  有界 Shell 通知队列。
- [x] 将 `notifications.post` 接入 WAMR host call，包含线性内存边界、Unix-only
  seccomp 和真机 WASM 调用闭环。
- [x] 将通知队列接入可信 System Shell，在标准/沉浸应用上显示有界横幅且不窃取
  应用键盘焦点。
- [x] 实现 `network.client` HTTPS-only broker、SSRF/DNS rebinding 防护、WAMR
  host call 和 Rust/C/C++ SDK 有界响应 API。
- [x] 实现无路径 Document Portal、可信文件选择器、`SCM_RIGHTS` 只读 FD 传递和
  Rust/C/C++ SDK 有界读取 API。
- [x] 实现 ES8389 有界 PCM 音频 broker、播放/录音分权和 Rust/C/C++ SDK API。
- [ ] 稳定性监控结束后完成音频播放、录音与拒绝权限的真机验收。
- [x] 实现固定 320x170 RGB565 帧的相机 broker、只读密封 FD 传递和三语言 SDK API。
- [ ] 连接兼容传感器后完成相机捕获、拒绝权限和画面方向的真机验收。
- [x] 实现 V0.6 四路逻辑输出 GPIO broker，排除所有板载关键引脚和原始 gpiochip API。
- [ ] 稳定性监控结束后完成 GPIO 读写、权限拒绝及 sysfs 权限收紧的真机验收。
- [x] 实现外接 SX1276 LoRa broker，固定 SPI/调制参数、地区频点边界、发送限速、
  `radio.lora` 授权和 Rust/C/C++ SDK API；镜像默认禁用。
- [ ] 连接 SX1276 模块并确认当地合法频点后完成收发、限速和拒绝权限真机验收。
- [x] 实现 Intent Broker：manifest 显式导出、唯一接收方路由、8 条有界队列、响应后
  单前台切换，以及 Rust/C/C++ SDK 一次性 `take` API。
- [x] 实现应用私有存储 broker 与 manifest 配额，移除 Runtime 的宿主数据目录挂载，
  提供原子有界 key/value Rust/C/C++ SDK API。
- [x] 建立经过真实应用 UID/cgroup/权限链路的音频、GPIO、存储配额与跨应用隔离
  真机验收工具，并用稳定性测试互锁避免污染 24 小时基线。
- [ ] 稳定性监控结束后完成存储持久化、配额拒绝及应用间读取隔离真机验收。
- [x] 建立恶意应用测试集，覆盖 WASI ambient authority、路径逃逸、设备访问、任意
  IPC、seccomp 旁路和 cgroup 资源耗尽，并纳入 `make check`。

完成条件：测试应用只能使用被授予的能力；拒绝权限后没有旁路；OOM/崩溃不会影响
Shell 和其他应用数据。

## Phase 4：SDK 与开发体验

- [x] 建立首个 `no_std` Rust SDK，封装系统时钟、事件等待、通知 capability 和
  稳定错误类型；Hello 示例已移除私有 FFI。
- [x] 建立 freestanding C11/C++17 SDK 头文件并加入 wasm32 双语言编译测试。
- [x] 发布完整 Rust 和 C/C++ SDK，从统一 ABI 契约生成 WAMR/C/Rust bindings，
  并提供 LVGL 9 320x170 适配层。
- [x] 实现 `cp0ctl new/build` 的 SDK-only 项目生成、Cargo metadata 解析和规范产物树。
- [x] 实现 `cp0ctl run/package/sign/install/logs`，包含 PC 到真机的 SSH 安装/日志路径。
- [x] 建立 PC WASM 模拟器、权限模拟、evdev 输入映射和 JSON 性能分析工具。
- [x] 迁移 Calculator、Camera 示例，不提供传统 Linux 应用兼容层。
- [x] 冻结 SDK 1.0 ABI、精确 legacy 0.1 兼容策略、权限词表和开发者文档。

完成条件：新开发者可只使用 SDK 在 PC 编写、调试、签名并安装一个应用到真机。

## Phase 5：应用包与商店

- [x] 冻结 `.capp` v1 可复现容器，实现开发者签名和独立商店审核签名。
- [x] 完成 root-owned 信任目录、双身份吊销和显式开发者模式验签策略。
- [x] 实现 `.capp` 原子安装、升级历史、断电恢复式重试和双向回滚。
- [x] 实现设备端 320x170 商店列表/详情、安装进度、已安装版本对账和离线状态。
- [x] 实现独立 `cp0-stored`、HTTPS 公网限制、签名目录、防回滚和断点下载恢复。
- [x] 建立 WASM 静态扫描、权限/import 审查元数据绑定和确定性发布工具。
- [x] 扩充商店协议/下载器确定性变异测试与针对目录、Range 响应的恶意样本集。
- [x] 增加家长/组织策略和面向用户的开发者模式/恢复模式开关。
- [ ] 配置测试商店端点后完成刷新、断点续传、安装、升级和离线目录真机验收。

完成条件：商店应用只能由可信签名安装，断电不会产生半安装状态，旧版本可回滚。

## Phase 6：产品化与后续安全

- 启动时间、功耗、SD 写放大、内存和刷屏性能优化。
- [x] 将 journald、临时目录和稳定性报告保持在 RAM，增加 64 MiB/24h SD 写入验收。
- [x] 加入内核 sysctl 与 compositor/Shell/appd systemd 产品安全加固基线。
- [x] 实现独立 `cp0-data` 分区、initramfs 幂等扩容、持久路径白名单和默认不可变
  底层根文件系统；loopback 扩容/重入与最终镜像发布门禁已通过。
- [ ] 烧录三分区候选，完成首次扩容、重启持久性、断电恢复和 24 小时 SD 写入真机
  验收后关闭不可变根产品化条目。
- [x] 建立默认不联网的本地脱敏支持包、显式同意的原始日志模式和 RAM-only
  诊断结果，排除应用数据、网络/设备身份与密钥。
- [x] 建立 V0.6 只读量产验收器，检查固定硬件、不可变根、数据分区、核心服务、
  socket 权限和 appd 控制路径；真机执行随三分区镜像烧录验收进行。
- [x] 增加独立 `recovery` 镜像构建 profile：可写维修根、tty1/LCD/SSH、禁用
  compositor 与全部应用入口、独立产物名和双 profile 最终镜像门禁。
- [x] 实现有界 `CP0 backup v1`、双遍完整性校验、只向空目标恢复、分区/维护模式门禁
  和与产品信任根绑定的恢复出厂设置流程。
- [ ] 烧录恢复介质并完成备份/恢复/出厂重置、真机启动及量产工位物理输入输出测试。
- [x] 完成系统威胁模型、量产阻断项和 dm-verity、RAUC A/B、U-Boot/FIT、硬件信任根
  的条件式架构评估；当前开发镜像不宣称 verified boot。
- [x] 为 manifest、`.capp`、Store、appd 控制帧和恢复备份建立 libFuzzer/ASan 入口、
  有界本地 smoke 和定期 CI。
- [ ] 在备用硬件/可擦写 SD 上实现并验证 A/B、verity、签名启动元数据、故障注入和
  自动回滚；不得在唯一 V0.6 设备上写入不可逆 OTP 状态。
- [ ] 委托第三方安全评审，跟踪并关闭或由产品负责人明确接受全部发现。
