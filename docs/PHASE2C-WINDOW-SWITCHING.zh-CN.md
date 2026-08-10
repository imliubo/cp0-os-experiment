# Phase 2C: 单前台窗口切换

<!-- doc-locale: zh-CN -->
> [English](PHASE2C-WINDOW-SWITCHING.md) | **简体中文**

## 窗口策略

compositor 策略现在拥有三个显式层：

- 在 `TOP_UI` 的受信任 System Shell 层；
- 一个普通的应用层；
- Shell 或所有不活跃应用的隐藏层。

当Shell可见时，每个应用程序视图都会隐藏。激活一个应用程序会隐藏Shell，并将恰好一个已映射的应用程序视图移动到正常层。其他所有应用程序视图仍然隐藏并且不会接收键盘焦点。全局系统操作会在传递操作事件之前将Shell移动回受信任层。

桌面子表面和弹出表面解析到其根顶级。只有根接收应用程序令牌；其子表面遵循相同的受信任、活动或隐藏层，因此对话框不能成为独立的启动器条目。

该策略失效闭合。如果 Shell 断开连接，应用程序视图将隐藏，直到可信的替代品注册。如果当前应用程序卸载或退出，策略会清除其令牌，显示 Shell 并发出 Home 事件。

## 协议版本 2

`cp0_system_shell_v1`版本2增加了应用程序发现和激活功能。
compositor 为每个已映射的 desktop surface 分配不透明的非零 token，并发送 `app_added` 和 `app_removed` 事件。Shell 使用 `activate_app(token)` 激活一个表面。

如果在激活请求正在进行时存在一个表面， compositor 会返回 `activation_failed(token)`. 这个生命周期竞态不会断开受信任的 Shell；Shell 会移除 stale 行并保持可见。

令牌，而不是 app-id，选择 compositor 所有的表面。app-id 保持不受信任的显示数据，并且在到达 Shell 之前仅限于 47 个可打印标识符字符。未来的 appd 协议将使令牌与验证过的清单标识关联；版本 2 不授予普通应用程序对受信任协议的访问权限。

320x170 Shell 渲染器最多保持四行可见的应用程序行。其状态测试涵盖添加、更新、选择、删除和打开事件。像素精确的 SHA-256 截图涵盖主页、应用、任务和电源状态。

## V0.6 硬件验证

政策、Shell 和私有协议针对 pinnned Weston 14.0.2 ABI 编译为 AArch64。包含 `-Werror`。二进制文件热部署；不需要镜像烧录。

`weston-simple-shm` 作为第二个 Wayland client 运行，UID 与 Shell 不同。policy 分配 token 1，Shell 收到相同 token。全屏 client 持续提交 buffer 时，Camera 检查确认它无法覆盖可信 Home 屏幕。

一个受信任的测试控制器激活了令牌1。然后摄像头检查显示应用程序全屏。重启量产Shell隐藏了仍在运行的应用程序并恢复了Home。一次压力测试完成了200个应用程序/Shell层过渡； compositor、Shell和测试应用程序保持活动状态，并且最终的物理LCD帧是Home。

一个过期令牌测试使用了`UINT32_MAX`，并收到了`activation_failed`，而受信任的客户端连接保持活跃。停止临时应用程序产生了`app_removed`；生产 compositor 和 Shell 仍然活跃。

测试控制器仅作为源代码包含在 `tests/` 中，并未安装在镜像中。产品验证仍然需要真实的 App Runtime 连接路径、非聚焦键盘事件测试、权限叠加、沉浸模式、屏幕睡眠以及 24 小时 compositor 运行。
