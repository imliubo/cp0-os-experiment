# CardputerZero OS 技术架构

## 1. 目标与约束

目标硬件固定为 CardputerZero V0.6：Raspberry Pi CM0、512 MB RAM、SD 卡和
320x170 LCD。系统只支持单个前台应用，不兼容传统 Linux 桌面应用；第三方开发者
必须使用 CardputerZero SDK。

安全目标是让恶意应用无法直接访问其他应用、硬件设备和未经授权的系统能力。
共享 Linux 内核无法提供数学意义上的“绝对隔离”，因此系统采用 WASM 运行时与
Linux 进程沙箱两层防护。

## 2. 总体分层

```text
Third-party application (.capp / WASM)
        |
Cardputer App Runtime (WAMR, one process per running app)
        |                         \
Cardputer SDK host calls          Wayland surface
        |                           |
Capability services         System Shell + Compositor
        |                           |
appd + permissiond + hardware brokers
        |
systemd + Debian arm64 + DRM/KMS + kernel drivers
        |
LCD / keyboard / audio / battery / camera / LoRa / GPIO
```

## 3. 基础系统

- 初期使用 Debian arm64 minimal 和 systemd，镜像构建复用现有 `pi-gen` 经验。
- 不安装 X11、完整桌面环境、浏览器和本机编译工具链。
- ST7789V 显示驱动迁移到 DRM/KMS TinyDRM/MIPI-DBI；用户应用不能访问 framebuffer。
- 键盘由 libinput/evdev 交给 compositor，应用只收到当前焦点窗口的输入事件。
- 使用 zram，不在 SD 卡上启用常规 swap。
- 系统根分区最终设为只读；A/B OTA 和安全启动暂不进入首版范围。

## 4. 图形与窗口模型

首个可运行版本使用 Weston kiosk shell 验证 DRM、Wayland 和应用生命周期。只有在
Weston 无法满足产品交互时，才实现基于 wlroots 的专用 compositor。

窗口策略固定如下：

- 同一时间只有一个前台应用 surface；后台应用不渲染；
- 普通模式为 320x150，上方约 20 px 系统状态栏；
- 沉浸模式使用完整 320x170；
- 权限、音量、通知和任务切换器由受信任的 System Shell 覆盖显示；
- 渲染目标为 RGB565、最多 30 FPS，并使用 damage region 降低 SPI 刷屏量。

## 5. 应用运行时

设备端采用 WAMR AOT，原因是其常驻内存比完整 JIT/Component Model 运行时更适合
512 MB 设备。WIT 是 SDK 的源级 ABI 描述，由代码生成器映射为 WAMR host calls；
首版不要求设备运行时原生实现 WASM Component Model。

每个运行中的应用由 `appd` 启动到独立沙箱：

- 独立 UID、PID/mount/network namespace 和 cgroup；
- `no_new_privs`、capability 全部移除、seccomp syscall allowlist；
- 只读应用包、一个带配额的私有数据目录、空的设备目录；
- 不暴露 system D-Bus、Wayland socket、evdev、DRM、ALSA 和 GPIO；
- 只有受信任的 App Runtime 持有 Wayland 连接并代理 SDK 调用。

App Runtime 在应用线性内存与 Wayland surface 之间传递 320x170 RGB565 帧。单帧约
106 KiB，即使发生一次内存复制也在硬件预算内。

## 6. 权限与能力服务

应用身份来自经过签名验证的包 ID 和运行进程凭据，不能由 RPC 参数自行声明。
私有存储、当前窗口输入和渲染是隐式能力；其他能力必须写入 manifest。

首版权限词表包括网络客户端、文档选择、音频播放/录音、相机、LoRa、GPIO、
剪贴板读取和通知。敏感能力由 `permissiond` 在首次使用时显示系统弹窗，可选择
本次允许、始终允许或拒绝。

文件共享不暴露路径，通过 Document Portal 返回受限文件描述符。应用间调用通过
Intent Broker 路由，接收方必须显式声明 Intent，不提供任意应用间 socket。

## 7. 包与应用商店

`.capp` 是签名的不可变应用包，至少包含 `app.json`、WASM/AOT 模块、资源和签名。
商店签名与开发者签名分离：开发者负责来源身份，商店在审核后为可安装产物签名。

`appd` 负责校验 schema、哈希、签名、SDK 版本、权限和资源上限，再以原子方式安装
到 `/var/lib/cardputer/apps/<app-id>/<version>`。应用数据位于独立目录，卸载时由
用户决定是否保留。

## 8. 512 MB 内存预算

| 模块 | 目标上限 |
|---|---:|
| 内核、systemd 和基础服务 | 100 MB |
| compositor、Shell、字体和图形缓冲 | 55 MB |
| appd、权限及硬件代理 | 30 MB |
| 单个 App Runtime 与应用 | 96 MB |
| 文件缓存、zram 和突发余量 | 231 MB |

首页空闲常驻内存目标低于 220 MB，应用运行时总使用目标低于 360 MB。超出 manifest
资源上限的应用由 cgroup 限制并由系统 Shell 报告终止原因。

## 9. 信任边界

内核、compositor、System Shell、App Runtime、appd 和能力服务属于可信计算基。
第三方 WASM、应用资源、网络响应和应用商店内容均视为不可信输入。原生第三方
可执行文件不属于支持范围；开发模式也只安装未上架的 WASM 应用。

