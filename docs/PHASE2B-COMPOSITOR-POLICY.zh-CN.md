# Phase 2B: 可信 compositor 策略

<!-- doc-locale: zh-CN -->
> [English](PHASE2B-COMPOSITOR-POLICY.md) | **简体中文**

## 安全合约

Phase 2B 策略必须保留这些不变量：

- 只有内核报告的`cp0-shell` UID 的进程才能绑定私有 System Shell 协议；
- 注册的表面必须属于那个 Wayland 客户端，并且具有预期的 System Shell app-id；
- 一个可见的信任层放置在正常和全屏应用层之上，而一个隐藏的信任层不被渲染；
- Home, Back, 任务和电源 由 compositor 消耗并独立于应用程序键盘焦点交付给认证的 Shell。
- 应用程序不能加入`cp0-wayland`, 打开DRM/输入设备, 或以可信图形账户运行。

app-id 检查不是身份验证。Wayland 同伴凭证和专用系统账户是安全边界。

## 组件

`cardputerzero-policy.so` 是一个在 kiosk shell 之后加载的 Weston 运行时模块。它创建了一个 `TOP_UI` 信任层，一个隐藏层，全局快捷键绑定和 `cp0_system_shell_v1` 全局。表面提交会安排空闲重新声明，以防止 kiosk-shell 焦点或堆叠变化使信任视图离开应用程序层。

该私有协议支持：

- 注册恰好一个可信的 xdg 表面；
- 显示或隐藏可信层;
- 家, 返回, 任务和电源操作事件。

System Shell 现在在启动时需要这个协议。静默回退故意被拒绝，因为这会使权限对话框在没有 compositor 强制的情况下显得可信。

## 进程边界

Weston 以 `cp0-compositor` 运行，包含视频、渲染和输入组。System Shell 以 `cp0-shell` 运行，不包含直接硬件组。两者都使用 `cp0-wayland` 组的 `0770` 运行时目录和使用 umask `0007` 创建的套接字。Weston 警告，因为通用的 XDG 运行时推荐是 `0700`；这个专用的系统 compositor 故意使用组访问权限，以便不同权限的 Shell 可以连接。只有根拥有者 systemd 单元和服务程序是进入任一账户的唯一方式。

未来的应用运行时将使用每应用独立的UID。它们必须不接收Wayland组；只有在运行时沙盒建立后，appd才会提供一个狭义范围的连接。

## 当前实现边界

此增量提供了协议、策略模块、凭证检查、可信层、全局动作交付、镜像集成和进程账户拆分。现有的主屏幕仍然是唯一的可信前台界面。

Phase 2C 添加了应用发现、单前台切换、焦点恢复和截图回退。其设计和 V0.6 结果在 `PHASE2C-WINDOW-SWITCHING.md` 中。权限提示和沉浸模式仍将在后续 compositor 增量中实现。

## V0.6 硬件验证

模块和更新后的Shell是针对固定版本的Weston 14.0.2构建的AArch64 ELF二进制文件，构建时将警告视为错误。模块导出了`wet_module_init`，并且没有安装Weston、Wayland服务器和libc集外的运行时依赖。

Phase 2B 文件被热部署到了刷写的 V0.6 镜像上。Weston 加载了模块，并记录了 `trusted uid=988 policy active` 和 可信 System Shell 注册。 compositor 使用了 7.4 MiB，Shell 使用了 780 KiB，它们都在 systemd cgroups 中。在负测试之后，两个服务仍然保持活动状态。摄像头检查确认了物理 LCD 上的 320x170 Home 屏幕。

过程边界在三层中进行了测试：

- 普通的 `pi` 无法穿越 `/run/cardputerzero`；
- `cp0-shell` 无法读取DRM或输入设备别名；
- 一个临时客户端使用了错误的UID但被迫进入`cp0-wayland`到达了套接字，然后收到了`cardputerzero system shell protocol is restricted`并在绑定私有协议时被断开连接。

强制对 Shell 执行 `SIGKILL` 后，PID 从 2731 变为 2798，`NRestarts=1`。compositor 保持 active，policy 接受替换后的可信 surface，Camera 检查确认 Home 恢复。物理 Fn 层 F1/F2/F3/F4 验证确认 Home、Back、Tasks 和 Power 由 compositor 传递。双客户端焦点恢复仍待处理。
