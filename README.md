# CardputerZero OS

CardputerZero OS 是面向 CardputerZero V0.6（Raspberry Pi CM0、512 MB RAM、SD 卡、
320x170 LCD）的轻量应用操作系统。

项目采用单前台应用模型。第三方应用必须使用 CardputerZero SDK 开发并编译为
WebAssembly；应用不能直接访问 Linux 设备节点、系统总线或其他应用的数据。

## 当前状态

Phase 0 的基础契约已经建立，Phase 1 的精简镜像已完成 V0.6 真机验收，Phase 2
已完成 compositor、System Shell、可信单前台策略、Launcher 与通知覆盖层，
Phase 3 已贯通 WAMR 强隔离运行时和 capability brokers，Phase 4 已冻结 SDK 1.0，
Phase 5 已实现可信安装、审核发布链、设备 Store daemon 与 320x170 Store UI。
当前仓库包含：

- 系统架构与资源预算；
- 分阶段 Roadmap 和初始 ADR；
- 已冻结的 CardputerZero SDK 1.0 WIT 公共契约、可生成 flat WAMR ABI 和精确
  legacy 0.1 兼容快照；
- 应用 manifest v1 和 Rust 校验库；
- `cp0ctl manifest validate` 开发工具。
- 固定版本的 CM0 V0.6 BSP 和 `pi-gen` 外部构建阶段；
- 真机启动内存配置、可回滚部署脚本和硬件 smoke test。
- 不含 Launcher 和高级桌面环境的 224 MB 压缩开发镜像构建配置。
- 面向无调试接口首次启动的 LCD 日志、IP/硬件状态摘要和本地键盘登录控制台。
- 裁剪构建的 Weston 14.0.2 DRM/Pixman kiosk 基线和内部硬件专用 seat；开发镜像
  默认保留恢复控制台，System Shell 完成后再默认启用 compositor。
- 无 GTK/Qt 依赖的 320x170 原生 Wayland System Shell：首页、状态栏、键盘导航、
  电源弹窗状态机、双 SHM buffer 和独立自动重启服务。
- Phase 2B Weston policy module：独立 compositor/Shell UID、peer credential 认证、
  compositor 可信层和全局 Home/Back/Tasks/Power 协议。
- Phase 2C/2D 单前台窗口策略：compositor surface token、应用发现/选择、可信层与
  应用层切换、焦点恢复、标准/沉浸显示、ARGB 状态栏、权限弹窗、屏幕休眠、像素
  回归和双客户端真机压力测试。
- Phase 2E 已安装应用 Launcher：appd 规范目录、启动后 surface 自动前台化、32 项
  滚动列表、启动状态，以及 Tasks 的 Resume/Stop 单运行槽控制。
- Phase 2F 可信通知横幅：私有协议 v4、顶部 88 px compositor 强制可信层、应用
  键盘焦点保持、权限提示优先级、四秒生命周期和 320x170 像素回归。
- Phase 2G 核心恢复与稳定性工具：固定单元 SIGKILL/PID/重绑验证、RAM-backed
  24 小时状态与内存采样、appd ping 和 socket 健康检查。
- Phase 3A 静态 WAMR App Runtime、每应用稳定 UID、bubblewrap namespace、runtime
  seccomp、systemd cgroup 硬限制及资源耗尽真机负向测试。
- Phase 3B socket-activated `appd`、可信安装注册表、Shell peer UID 认证、单运行槽
  和 `cp0ctl app list/start/stop` 真机生命周期闭环。
- Phase 3C root-owned 权限数据库、仅本次/始终/拒绝决策、单待处理可信提示状态机，
  以及经过 peer UID 认证的 Shell 查询与决策协议。
- Phase 3D `notifications.post` typed broker、运行中应用 peer UID 认证、沙箱内唯一
  broker socket 和固定 8 条上限的 Shell 通知队列。
- Phase 3E 首个 WAMR SDK host call、WASM 线性内存参数校验、Unix-only seccomp
  socket 规则和 Hello WASM 真机通知闭环。
- Phase 3F `network.client`：独立低权限 HTTPS broker、公共地址 resolver、
  SSRF/DNS rebinding 防护、5 秒/2 次重定向/2 KiB 上限及三语言 SDK API。
- Phase 3G `documents.open`：独立低权限 Document Portal、可信 Shell 文件选择器、
  无路径 API、`SCM_RIGHTS` 只读 FD、符号链接/替换防护及 4 KiB 有界读取。
- Phase 3H 音频能力：独立 `cp0-audiod`、ES8389 专用 ALSA 端点、播放/录音分权、
  16 kHz 单声道 S16_LE 格式、每次 1024 帧上限及三语言 SDK API。
- Phase 3I 相机能力：独立 `cp0-camerad`、固定 320x170 RGB565 捕获、密封只读
  memfd 传递、V4L2/Media/dma-heap 设备白名单及三语言 SDK API。
- Phase 3J GPIO 能力：独立 `cp0-gpiod`、V0.6 四路固定逻辑输出、root/appd 身份认证、
  BSP sysfs 权限收紧及三语言 SDK API，不暴露 gpiochip、路径或 BCM 引脚编号。
- Phase 3K LoRa 能力：外接 SX1276 使用固定 SPI0 CS1、独立 `cp0-radiod`、地区/频点
  root 配置、15 秒发送限速、64 字节报文上限及三语言 SDK API，镜像默认禁用发射。
- Phase 3L 私有存储：独立 `cp0-storaged`、manifest 配额、8 KiB 原子 key/value API、
  256 键上限及三语言 SDK；Runtime 不再挂载宿主应用数据目录。
- Phase 3M Intent Broker：manifest 显式 action、唯一接收方路由、8 条/1 KiB 有界
  队列、确认后单前台切换和一次性 Rust/C/C++ SDK `take` API，无任意应用间 socket。
- Phase 3N 恶意应用回归：WASI ambient authority、路径逃逸、设备节点、任意 IPC、
  seccomp 和 cgroup 资源耗尽样本及自动化隔离契约检查。
- Phase 4A dependency-free `no_std` Rust SDK，封装时钟、事件等待、通知 capability
  和稳定错误类型；Hello 示例不再使用 Runtime 私有 FFI。
- `cp0ctl new/build` SDK-only 项目脚手架、结构化 Cargo metadata 构建和规范应用
  产物目录。
- `pi-gen` app platform stage：构建并安装 appd、broker sockets、静态 Runtime、
  稳定测试身份与 SDK 版 Hello，开发镜像默认进入 System Shell。
- Freestanding C11/C++17 SDK 1.0 头文件与 wasm32 编译检查，不暴露 WASI/Linux ABI。
- 三分区不可变根产品配置：只读 ext4 lower、64 MiB RAM upper、可自动扩容的
  `cp0-data`，以及应用/权限/信任、网络、SSH 和设备身份的持久路径白名单。
- Phase 5 双签名商店：审核记录精确绑定提交/权限/WASM imports，`cp0ctl store
  publish` 生成确定性签名目录，设备端 `cp0-stored` 提供 HTTPS 公网下载、断点续传、
  目录防回滚和 appd 独立复验，System Shell 提供 Store 列表、详情与安装进度。
- Phase 5C 设备策略与 Settings：root-owned 家长/组织策略限制 Store、应用白名单和
  全局权限，用户可二次确认切换开发者模式与下次启动的 tty1 恢复控制台。
- Phase 6B 本地诊断与量产门禁：默认脱敏且不联网的 RAM-only 支持包、显式同意的
  原始服务日志，以及只读检查 V0.6 硬件、不可变根、服务与 socket 的工厂验收器。
- Phase 6C 独立恢复镜像 profile：默认 tty1/LCD/SSH 维修环境、可写 lower root、
  compositor/appd/broker 全部禁用，以及与产品镜像分离的发布门禁和产物名称。
- Phase 6D 有界持久数据恢复：版本化单文件备份、逐文件/整体完整性校验、严格路径与
  类型白名单、只向空目标恢复，以及明确确认后的恢复出厂设置。

## 快速验证

```sh
cargo run -p cp0ctl -- manifest validate examples/hello-card/app.json
make check
```

构建开发镜像需要 Docker：

```sh
CP0_FIRST_USER_PASSWORD='development-password' make image
make verify-image
```

详细设计见 [系统架构](docs/ARCHITECTURE.md) 和 [Roadmap](docs/ROADMAP.md)。

Phase 1 构建和真机验证方法见 [BSP 与镜像说明](docs/PHASE1-BSP.md)，Phase 2
compositor 基线见 [Compositor bring-up](docs/PHASE2-COMPOSITOR.md)。
System Shell 原型的实现边界和真机结果见
[System Shell Phase 2A](docs/PHASE2-SYSTEM-SHELL.md)。
Phase 2B 的安全不变量与实现边界见
[Trusted compositor policy](docs/PHASE2B-COMPOSITOR-POLICY.md)。
Phase 2C 的单前台策略和真机切换结果见
[Single-foreground window switching](docs/PHASE2C-WINDOW-SWITCHING.md)。
可信状态栏、权限覆盖层、沉浸模式和屏幕休眠见
[Trusted overlays and display policy](docs/PHASE2D-TRUSTED-OVERLAYS.md)。
已安装应用枚举、启动和 Tasks 生命周期见
[Installed application Launcher](docs/PHASE2E-LAUNCHER-LIFECYCLE.md)。
通知可信呈现、焦点和生命周期策略见
[Trusted notification banners](docs/PHASE2F-TRUSTED-NOTIFICATIONS.md)。
核心服务故障恢复与 24 小时验收方法见
[Core recovery and stability acceptance](docs/PHASE2G-RECOVERY-STABILITY.md)。
Phase 3A 的 runtime、沙箱契约和真机安全验证见
[App Runtime and Linux sandbox](docs/PHASE3A-APP-RUNTIME.md)。
`appd` 的控制协议、启动前校验与真机生命周期证据见
[appd lifecycle service](docs/PHASE3B-APPD-LIFECYCLE.md)。
权限持久化、提示状态机和 Shell 控制契约见
[permission decisions and trusted prompts](docs/PHASE3C-PERMISSIONS.md)。
首个应用能力调用链及资源边界见
[notification capability broker](docs/PHASE3D-NOTIFICATION-BROKER.md)。
Runtime host call ABI 与真机结果见
[Runtime capability host calls](docs/PHASE3E-RUNTIME-HOSTCALLS.md)。
受限 HTTPS API、独立 networkd 和 SSRF 防护见
[restricted HTTPS client broker](docs/PHASE3F-NETWORK-BROKER.md)。
无路径文件选择、只读 FD 传递和 Runtime 读取边界见
[restricted Document Portal](docs/PHASE3G-DOCUMENT-PORTAL.md)。
有界 PCM 播放/录音、ALSA 设备隔离和服务加固见
[restricted audio broker](docs/PHASE3H-AUDIO-BROKER.md)。
固定帧相机 API、密封 FD 传递和捕获设备隔离见
[restricted camera broker](docs/PHASE3I-CAMERA-BROKER.md)。
V0.6 逻辑输出映射、GPIO 隔离和 sysfs 权限收紧见
[restricted GPIO broker](docs/PHASE3J-GPIO-BROKER.md)。
外接 SX1276 的固定无线参数、地区配置和 SPI 隔离见
[restricted LoRa broker](docs/PHASE3K-LORA-BROKER.md)。
私有数据的配额、原子写入和 Runtime 文件系统收紧见
[quota-enforced private storage](docs/PHASE3L-PRIVATE-STORAGE.md)。
应用间受控路由、确认顺序和单前台切换见
[isolated Intent Broker](docs/PHASE3M-INTENT-BROKER.md)。
恶意样本、Runtime 失陷边界和负向验收见
[malicious application regression set](docs/PHASE3N-MALICIOUS-APPLICATIONS.md)。
Rust SDK 公共 API 和真机迁移结果见
[Rust SDK foundation](docs/PHASE4A-RUST-SDK.md)。
应用审核、确定性发布、设备 Store 信任边界和离线行为见
[reviewed application store](docs/PHASE5B-APPLICATION-STORE.md)。
设备策略、Settings 开关和恢复控制台退出流程见
[device policy and user-controlled modes](docs/PHASE5C-DEVICE-POLICY.md)。
不可识别诊断数据边界、敏感日志同意和量产门禁见
[privacy-preserving diagnostics and factory acceptance](docs/PHASE6B-DIAGNOSTICS-FACTORY.md)。
独立维修介质的构建、启动约束和发布门禁见
[independent recovery image profile](docs/PHASE6C-RECOVERY-IMAGE.md)。
有界离线备份、恢复和产品 factory seed 约束见
[bounded backup, restore and factory reset](docs/PHASE6D-RECOVERY-DATA.md)。
