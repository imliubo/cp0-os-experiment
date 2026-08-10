# Phase 2D: 可信叠加和显示策略

<!-- doc-locale: zh-CN -->
> [English](PHASE2D-TRUSTED-OVERLAYS.md) | **简体中文**

## Compositor 合约

版本3的`cp0_system_shell_v1`使显示策略明确。只有内核认证的`cp0-shell`客户端可以选择这些模式之一：

- `full`：可信赖的Shell是不透明的，应用程序是隐藏的，并且键盘焦点属于Shell；
- `status`: 标准应用保持聚焦在可信任的21像素状态条下方；
- `hidden`: 全沉浸应用拥有所有 320x170 像素，而受信任的 Shell 表面不被渲染。

应用程序运行时提供 `cardputerzero:standard` 或 `cardputerzero:immersive` 作为受信任的启动合同标题。这个标题仅选择渲染策略。应用程序身份仍然来自根控制的 appd 架构文件和每个应用程序的 Unix 账户。

compositor 将应用视图和受信任视图保持在分离的 Weston 层上。
它在没有活动应用时拒绝状态或隐藏模式，在活动表面消失时恢复全屏模式，并始终在应用键盘交付之前处理 Home、Back、Tasks 和 Power。因此，应用不能在权限决定之上绘制，也不能在其中一个之下保留焦点。

## System Shell

Shell 使用双缓冲的 ARGB8888 SHM 缓冲区。在状态模式下，行 20 以下的像素是透明的，Wayland 输入区域仅限于状态栏。在通知模式下，仅状态栏和确切的信任提示或系统操作矩形是不透明的；应用程序的其余部分仍然可见，隐藏的 Shell 页面不能成为覆盖背景。在隐藏模式下，输入区域为空。全屏模式覆盖整个显示并接收键盘焦点。

权限提示通过经过身份验证的控制套接字从appd读取。
Shell 使用一个有界8 KiB帧，一个128个令牌的JSON解析器和250毫秒的套接字超时。它只显示规范化的清单数据，并提供ONCE、ALWAYS和DENY选项。另一个受信任控制器解决的提示将在下一秒的轮询中被移除。诊断命令

```sh
cp0ctl permission reset <app-id> <capability>
```

原子地移除一个持久决策并恢复首次使用提示。

Power 对话框现在发送 compositor sleep request。Weston 关闭 output 并负责按键唤醒；其 wake signal 恢复可信 Home view，然后返回焦点。

## 验证

主机测试覆盖了协议/配置不变量、ARGB UI 渲染、权限对话框像素、JSON 转义和 Unicode 解码、无效输入、令牌限制和整数溢出。 compositor、Shell、Runtime、appd 和 cp0ctl 在部署前也跨编译到了 AArch64。

整个阶段在512 MB V0.6设备上热部署而无需刷写或重启。硬件验证表明：

- 标准的 Hello 内容保持在受信任的状态条下方；
- 一个未解决的`notifications.post`请求在其物理LCD上显示了其标准应用名称、权限、原因以及三个决策；
- 一个外部的一次性决策移除了提示，允许等待的WASM宿主调用并产生了预期的通知；
- 停止应用后恢复了Home，而 compositor 和 Shell 仍然保持活动状态；
- 物理 F1, F2, F3 和 F4 通过 compositor 所有的全局绑定分别触发 Home, Back, Tasks 和 Power。

此阶段不需要进行镜像烧录。下一完整`pi-gen`镜像构建将包含相同的源和服务配置。
