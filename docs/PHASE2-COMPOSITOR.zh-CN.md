# 第二阶段：Compositor 带上电

<!-- doc-locale: zh-CN -->
> [English](PHASE2-COMPOSITOR.md) | **简体中文**

## 基线

CardputerZero OS 将 Weston 14.0.2 固定在提交 `015b3b4d4c05da44a22349ea6e651d1a8f678c59`。镜像从源码构建 Weston，只包含以下量产所需部分：

- DRM 和 320x170 无头测试后端；
- Pixman 软件渲染器和阴影帧缓冲区;
- kiosk shell 和 SHM 烟雾测试客户端；
- libseat/seatd 会话控制和 libinput 键盘处理。

EGL, XWayland, 桌面/IVI 窗口管理器, RDP, VNC, PipeWire, GStreamer, VA-API，
远程连接, 示例客户端和上游测试被禁用。分阶段的 Weston 运行时在剥离前约为 4 MB，相比之下，Debian 通用 Weston 包的额外安装大小约为 487 MB。

## 硬件路由

BSP udev 规则将内部 LCD 和键盘分配给`seat-cardputer-zero`并创建稳定的别名：

```text
/dev/dri/cardputer-zero-internal
/dev/input/cardputer-zero-internal
```

compositor 服务选择稳定的 DRM 别名和自定义座位。
这会从可信的 System Shell 输入路径中排除 HDMI、IR 和无关的外部输入设备。

这些别名大约在 CM0 coldplug 后十秒出现。因此，compositor unit 要求相应的 systemd device unit，而不使用一次性 `ConditionPathExists` 检查。键盘 udev 规则带有 `systemd` tag，因此其稳定别名也会获得一个 device unit。这可以防止早期 multi-user target transaction 在 LCD 和键盘别名出现前永久跳过 compositor。

该产品不会静态启用 `getty@tty1` 或 compositor。
在管理器启动时，`cardputerzero-display-generator` 会选择一个确切的会话：对于正常产品启动，选择 compositor；对于恢复镜像或带有持久恢复标记的产品，则选择恢复控制台。这避免了将相互冲突的显示会话放在同一个 systemd 事务中。
compositor 的 `OnFailure=getty@tty1.service` 仍然是在正常会话选择之后失败时的一个独立备用方案。

## 启动移交

产品在冷启动显示稳定器等待期间保持BSP RGB565启动画面可见。然后它恰好启动一次Weston。Kiosk壳程序自动启动`cardputerzero-boot-splash`作为`cp0-compositor`； compositor策略仅接受其预留app-id，并将其置于正常应用之上但低于可信的System Shell。compositor服务在splash客户端的第一帧回调之前进行有限等待，使其变得活跃，因此正常System Shell启动不能与Wayland启动画面竞争。其第一次完整的Setup或Home表面然后覆盖启动画面而无需中间清除。如果splash客户端中断，它不会无限期阻塞恢复或Home。

Splash surface 不参与应用发现、Tasks 和应用状态截图。它在整个启动会话中保持可用，因此 System Shell 的早期重启会露出量产 splash image，而不是黑色 compositor 背景。该交接补充 initramfs 和 framebuffer renderer；它不使用已退役的 VideoCore 固件，也不改变 64/448 MB 内存预算。

V0.6 硬件验证通过，使用 Weston 14.0.2，DRM 原子后端，
Pixman 阴影帧缓冲区，自助终端和 `weston-simple-shm` 在
`320x170@30Hz`。当自定义座位激活时，Libinput 选择了 `tca8418c`。最终图像的 systemd cgroup 报告非根 compositor 和测试客户端使用了 9.7 MiB，且没有重启。停止服务恢复了 `tty1`，随后重启保持 compositor 禁用和恢复控制台激活。

## 开发激活

compositor 保持禁用，直到真正的 System Shell 用 SHM 测试客户端替换它。第二阶段 A 现在提供了那个客户端，但默认激活仍然禁用，直到它的 compositor 侧受信任叠加完成。要将 LCD 从恢复控制台切换到 compositor：

```sh
sudo systemctl start cardputerzero-compositor.service
```

返回到本地登录控制台：

```sh
sudo systemctl stop cardputerzero-compositor.service
sudo systemctl start getty@tty1.service
```

运行时日志保存在tmpfs中的`/run/cardputerzero/weston.log`。

Weston 的 DRM backend 还需要一个 `AF_NETLINK` udev monitor 来发现和跟踪专用输入设备。因此 compositor unit 只允许 `AF_UNIX AF_NETLINK`；若限制为 `AF_UNIX`，backend 会在 DRM 成功打开后创建失败。该故障已于 2026-07-31 在 V0.6 上复现并修复；随后 Weston 以 320x170@30 启用 `UNNAMED-1` 并注册 `tca8418c` 键盘。
