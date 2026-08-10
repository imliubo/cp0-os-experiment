# CardputerZero OS

<!-- doc-locale: zh-CN -->
> [English](README.md) | **简体中文**

CardputerZero OS 是面向 CardputerZero V0.6 的轻量应用操作系统，目标硬件固定为
Raspberry Pi CM0、512 MB 内存、SD 卡和 320x170 LCD。系统不使用传统 Linux 桌面，
而是提供键盘优先的 System Shell、可信 compositor 策略和单前台应用模型。

第三方应用必须使用 CardputerZero SDK 开发并打包为 WebAssembly 应用。应用不能直接
访问 Linux 设备节点、系统总线、任意 IPC、宿主路径或其他应用的数据。敏感操作统一由
类型化能力代理服务和所有者管理的权限进行控制。

## 系统模型

- 基于精简 Debian/systemd，设备镜像不包含 X11、浏览器或完整桌面环境。
- 原生 320x170 System Shell 提供 Home、Apps、Tasks、Settings、Store、可信覆盖层、
  初次启动配置和 recovery 入口。
- 基于 Weston 的 compositor 策略强制单个可见前台应用、可信系统层、全局按键、焦点、
  屏幕休眠和任务切换。
- 使用 WAMR AOT；每个运行中的应用有独立 Linux 进程和稳定 UID，并受到 namespace、
  seccomp、cgroup、空设备视图和有界运行资源的约束。
- root 所有的应用生命周期和权限服务，以及相互隔离的网络、文档、音频、相机、GPIO、
  LoRa、存储、Intent、截图、照片、显示、电源和媒体交换能力代理服务。
- 签名 `.capp` 包、确定性审核和发布记录、设备端 Store 服务，以及与设备镜像分离的
  Web 控制面应用。
- 产品、开发和 recovery 访问配置分别约束 SSH、控制台、可写根、更新、备份和恢复
  出厂设置。

由于所有沙箱仍共享 Linux 内核，这套设计提供的是纵深防御，不是数学意义上的“绝对
隔离”。具体安全边界和残余风险见[威胁模型](docs/THREAT-MODEL.zh-CN.md)。

## 项目状态

仓库已经包含可运行的 OS 软件栈、SDK 1.1、示例应用、镜像构建流水线、Store 组件、
recovery 工具及 host/真机验收套件。核心 Shell、应用运行时、能力代理、应用包、不可变
根文件系统、诊断、recovery、量产访问和验证更新基础已经实现，并在 V0.6 硬件上逐步
完成过验证。

项目目前仍处于工程开发阶段，不是已经公开发布的量产版本。硬件、长时间稳定性、生产
基础设施、发布签名、许可证和 rollout 的未关闭门禁，以 [Roadmap](docs/ROADMAP.zh-CN.md)
及其链接的专项 roadmap 为准。host 测试通过不能替代真机或最终镜像验收。

## 仓库结构

- `image/` 和 `bsp/`：可复现镜像阶段与固定版本的 V0.6 BSP。
- `system-shell/`、`compositor/` 和 `protocol/`：可信用户界面、窗口策略和私有
  Wayland 协议。
- `app-runtime/`、`appd/` 和 `crates/`：WASM Runtime、生命周期服务、能力代理、
  Store 服务和命令行工具。
- `sdk/`、`devkit/`、`simulator/`、`examples/` 和 `skills/`：公共应用契约、开发
  工具、模拟器、示例应用和构建指导。
- `developer-portal/`、`review-console/` 和 `store-operations/`：不进入设备镜像的
  Store Web 控制面。
- `docs/`、`schemas/`、`tests/` 和 `scripts/`：架构、决策、契约、构建工具和验收
  自动化。

## 快速开始

验证 manifest 并运行仓库门禁：

```sh
cargo run -p cp0ctl -- manifest validate examples/hello-card/app.json
make check
```

为当前 host 构建可重定位 App DevKit：

```sh
make devkit
```

构建镜像需要 Docker。开发镜像必须显式传入仅用于开发的密码：

```sh
CP0_FIRST_USER_PASSWORD='development-password' make image
make verify-image
```

构建不带共享登录密码或 SSH key 的量产候选镜像：

```sh
CP0_ACCESS_PROFILE=production make image
make verify-image
```

这些命令通过并不代表可以直接发布镜像或 DevKit。发布前仍需满足相关文档中的许可证、
签名、release 和真机门禁。

## 文档入口

- [技术架构](docs/ARCHITECTURE.zh-CN.md)
- [Roadmap](docs/ROADMAP.zh-CN.md)
- [开发者指南](docs/DEVELOPER-GUIDE.zh-CN.md)
- [App DevKit 分发](docs/APP-DEVKIT-DISTRIBUTION.zh-CN.md)
- [SDK 版本策略](docs/SDK-VERSIONING.zh-CN.md)
- [Store 架构](docs/STORE-ARCHITECTURE.zh-CN.md)
- [威胁模型](docs/THREAT-MODEL.zh-CN.md)
- [Recovery 镜像](docs/PHASE6C-RECOVERY-IMAGE.zh-CN.md)
- [量产访问配置](docs/PHASE6G-PRODUCTION-ACCESS.zh-CN.md)
- [文档本地化规则](docs/LOCALIZATION.zh-CN.md)

英文是默认文档语言。每份维护中的 Markdown 文档都配有对应的简体中文
`*.zh-CN.md` 文件，可通过标题下方的语言切换链接在两种版本之间切换。
