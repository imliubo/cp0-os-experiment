# Phase 2A: 系统 Shell 原型

<!-- doc-locale: zh-CN -->
> [English](PHASE2-SYSTEM-SHELL.md) | **简体中文**

## 范围

Phase 2A 用受信任的本地 Wayland 客户端替换了诊断 `weston-simple-shm` 启动客户端。它不使用 GTK、Qt、Cairo 或桌面壳。确定性渲染器拥有两个 XRGB8888 SHM 缓冲区，大约 425 KiB，在 320x170 大小，并提供：

- 一个包含应用、设备、网络和电源的四项主屏界面；
- 一个 21 px 状态栏，显示时间、网络状态和电池容量；
- Home, Back, 任务和电源状态通过键盘导航；
- 一个可信赖的电源对话状态机;
- 一个30秒的计时器用于刷新空闲状态。

渲染器与Wayland独立。`tests/test-system-shell-ui.sh`
在宿主机上编译它，并在原生LCD分辨率下检查状态转换、边界防护和稳定布局像素。

## 输入映射

Wayland 客户端从 compositor 消费 Linux 键盘键码。原型将箭头和 Enter 映射用于导航，Escape/Backspace/F2 用于返回，Home/Homepage/F1 或 Meta+H 用于首页，F3 或 Meta+Tab 用于任务，Power/F4 用于电源对话框。映射故意不直接打开 evdev。

V0.6硬件测试确认了Weston/libinput将`tca8418c`附加到内部输出，并给予System Shell键盘焦点。物理验证确认了方向导航、Enter、Escape和Backspace。
Phase 2B现在在compositor中处理Home、Back、Tasks和Power。物理验证确认了F1、F2、F3和F4分别调用这四个动作。

## 进程监督

Weston 和系统 Shell 是分开的 systemd 服务。启动 `cardputerzero-compositor.service` 拉取 Shell；Shell 等待 Wayland 套接字，使用 32 MiB 的 cgroup 限制，并在失败后重启。停止 compositor 通过 `BindsTo` 和 `PartOf` 停止 Shell。

在最终的 V0.6 镜像中，Shell 的 systemd cgroup 约使用 1.2 MiB，RSS 为 2.0 MiB。强制发送 SIGKILL 将 PID 从 988 变为 1082 时，
`NRestarts=1`新进程返回到Home，而Weston保持活跃。

## 图像候选

集成的Phase 2A镜像为：

```text
deploy/image_2026-07-30-cardputerzero-os-phase2a-cp0-os-dev.img.xz
SHA-256 93793244fa610cfa82203ef325045119ad9c03d1cc64a1c6bd67017bf91179b5
```

它压缩后是223 MiB。离线验证检查了两个部署哈希值，两个包清单，arm64 System Shell 可执行文件，compositor/Shell 单元，32 MiB Shell 限制，恰好一个受管理的 BSP 块以及 compositor 的默认禁用状态。临时仓库 502 也锻炼了恢复构建；BSP 和 DTB 贴路径现在可以安全地运行多次。

## 最终刷写镜像验证

上述镜像已烧录到 V0.6 硬件，并启动 `6.18.34+rpt-rpi-v8` 内核。系统在 24.060 秒内到达 `multi-user.target`，把根文件系统扩展到 28.2 GiB SD 分区，并报告 424756 KiB RAM。恢复控制台和 `seatd` 处于 active 状态，compositor 按预期默认禁用。

启动 `cardputerzero-compositor.service` 停止了 `getty@tty1`，并启用了 Weston 和 System Shell，没有初始重启。Weston 使用了 Pixman 渲染器，选择了 `320x170@30`，并通过 libinput 连接了 `tca8418c` 键盘。它的 cgroup 使用了大约 9.8 MiB。设备烟雾测试通过了型号、内存、cgroup、AppArmor、LCD、帧缓冲区、键盘、音频、电池和启动时间检查，没有失败。可选的 `/dev/video0` 接口仍然是从 Phase 1 硬件基线发出的警告，而不是 Phase 2 的回退。I2C-1 内核总线现在通过 sysfs 进行检查，因为产品故意不提供原始 `/dev/i2c-1` 访问。

第一次 compositor 激活也暴露了一个 V0.6 特有的移交问题：即使 Weston DRM CRTC 和主平面处于活动状态，帧缓冲控制台仍可能停留在 `FB_BLANK_POWERDOWN`。面板是黑色的，而背光、连接器和两个进程看起来都很健康。恢复 `tty1` 并将 `0` 写入 LCD 帧缓冲空白控制使控制台可见；在相同的操作后启动 Weston 使 System Shell 可见。compositor 服务现在等待其新的 Wayland 套接字并通过一个仅 root 的后启动助手来解锁 LCD。其非特权 Weston 进程和设备权限没有改变。助手解析 `/dev/fb_lcd` 而不是假设一个帧缓冲编号：LCD 在验证重启前是 `fb1`，重启后是 `fb0`。模拟非零空白状态和真正的重启都恢复到了 `blank=0`，并且摄像头检查确认 compositor 启动后 Home 屏幕可见。当 compositor 禁用时，恢复控制台仍然可见。

## 安全边界

Weston kiosk shell 仍然是一个调试组件。普通的 xdg-toplevel 无法证明其对话框位于不可信客户端之上，并且其快捷键仅在它拥有键盘焦点时可用。因此 Phase 2A 仍未满足安全权限对话框或全局 Home/Back 的要求。

在启用第三方应用程序之前，compositor 侧必须暴露一个经过身份验证的 System Shell 控制协议或等效内置策略，以保留系统覆盖层，强制执行一个前台应用并拥有全局快捷键。只有这样，权限提示才能被视为安全边界。

## 开发激活

compositor 默认是禁用的。在构建新的镜像后，使用以下命令启动集成的Shell：

```sh
sudo systemctl start cardputerzero-compositor.service
```

使用以下命令返回到恢复控制台：

```sh
sudo systemctl stop cardputerzero-compositor.service
sudo systemctl start getty@tty1.service
```
