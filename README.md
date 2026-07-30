# CardputerZero OS

CardputerZero OS 是面向 CardputerZero V0.6（Raspberry Pi CM0、512 MB RAM、SD 卡、
320x170 LCD）的轻量应用操作系统。

项目采用单前台应用模型。第三方应用必须使用 CardputerZero SDK 开发并编译为
WebAssembly；应用不能直接访问 Linux 设备节点、系统总线或其他应用的数据。

## 当前状态

Phase 0 的基础契约已经建立，Phase 1 的精简镜像已完成 V0.6 真机验收，Phase 2
已完成最小 compositor 基线、Phase 2A System Shell、Phase 2B/2C 可信单前台策略，
并进入 Phase 3 应用运行时与隔离实现。
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
- Phase 2C 单前台窗口策略：compositor surface token、应用发现/选择、可信层与应用层
  切换、焦点恢复、四页面像素回归和双客户端真机压力测试。
- Phase 3A 静态 WAMR App Runtime、每应用稳定 UID、bubblewrap namespace、runtime
  seccomp、systemd cgroup 硬限制及资源耗尽真机负向测试。
- Phase 3B socket-activated `appd`、可信安装注册表、Shell peer UID 认证、单运行槽
  和 `cp0ctl app list/start/stop` 真机生命周期闭环。

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
Phase 3A 的 runtime、沙箱契约和真机安全验证见
[App Runtime and Linux sandbox](docs/PHASE3A-APP-RUNTIME.md)。
`appd` 的控制协议、启动前校验与真机生命周期证据见
[appd lifecycle service](docs/PHASE3B-APPD-LIFECYCLE.md)。
