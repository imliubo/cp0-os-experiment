# CardputerZero OS

CardputerZero OS 是面向 CardputerZero V0.6（Raspberry Pi CM0、512 MB RAM、SD 卡、
320x170 LCD）的轻量应用操作系统。

项目采用单前台应用模型。第三方应用必须使用 CardputerZero SDK 开发并编译为
WebAssembly；应用不能直接访问 Linux 设备节点、系统总线或其他应用的数据。

## 当前状态

Phase 0 的基础契约已经建立，Phase 1 的精简镜像已完成 V0.6 真机验收，Phase 2
已完成 compositor、System Shell、可信单前台策略、Launcher 与通知覆盖层，
Phase 3 已贯通 WAMR 强隔离运行时、权限系统和首个 capability broker。
当前仓库包含：

- 系统架构与资源预算；
- 分阶段 Roadmap 和初始 ADR；
- CardputerZero SDK 的 WIT ABI 草案；
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
- Phase 4A dependency-free `no_std` Rust SDK，封装时钟、事件等待、通知 capability
  和稳定错误类型；Hello 示例不再使用 Runtime 私有 FFI。
- `cp0ctl new/build` SDK-only 项目脚手架、结构化 Cargo metadata 构建和规范应用
  产物目录。
- `pi-gen` app platform stage：构建并安装 appd、broker sockets、静态 Runtime、
  稳定测试身份与 SDK 版 Hello，开发镜像默认进入 System Shell。
- Freestanding C11/C++17 SDK 0.1 头文件与 wasm32 编译检查，不暴露 WASI/Linux ABI。

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
Rust SDK 公共 API 和真机迁移结果见
[Rust SDK foundation](docs/PHASE4A-RUST-SDK.md)。
