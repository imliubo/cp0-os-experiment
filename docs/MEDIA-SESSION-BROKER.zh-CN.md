# 媒体会话代理服务

<!-- doc-locale: zh-CN -->
> [English](MEDIA-SESSION-BROKER.md) | **简体中文**

媒体会话代理完成全局 `Fn+Q`、`Fn+W` 和 `Fn+E` 动作而不向 System Shell 或应用程序提供一般进程间通信。这是协调元数据，而不是音频设备能力。播放声音仍然需要清单权限 `audio.playback`。

## 信任和身份

应用程序只能通过其现有的Runtime能力代理套接字发送 `update-media-session` 和 `take-media-action`。这两个请求都不包含应用程序ID。appd从对等的UID推断身份，验证该进程属于已安装应用程序的systemd cgroup，并要求它是当前的Runtime会话。

System Shell 通过受信任的 appd 控制套接字发送一个无目标的 `dispatch-media-action` 命令。appd 快照唯一的前台 Runtime，并仅将该动作路由到由该确切应用程序注册的媒体会话。成功响应包括权威的应用程序 ID 和接受的动作。Shell 在显示 `SENT` 之前严格检查两者。

应用程序不能提供标题、艺术作品、路径、套接字、进程ID或目标应用程序。安装的清单保留对其身份和显示名称的权威性。

## 边界状态

注册仅包含：

- 播放状态: `inactive`, `paused` 或 `playing`;
- 支持播放/暂停、上一首和下一首的三位位操作掩码。

一个不活跃的会话必须使用空掩码。一个暂停或播放的会话必须支持至少一个动作。未知位和未知枚举值在SDK、Runtime和appd协议边界处被拒绝。

代理存储一个会话和最多四个待处理操作。不支持的操作返回`UNAVAILABLE`；队列满时返回`BUSY`. 执行操作会消耗该操作一次。注册替换会丢弃队列中的操作，减少支持掩码会移除不再有效的队列中的操作。

## 生命周期

媒体状态在显式停止、意外退出、应用程序替换或成功卸载和无效注册时被清除。生命周期代码独立于 Runtime 锁定清除代理状态，因此过时的应用程序无法接收本应发送给新前台进程的动作。

公共 SDK 暴露：

- C/C++: `cp0_media_session_update` 和 `cp0_media_take_action`;
- Rust: `media::update_session` 和 `media::take_action`;
- WIT：导入的`media`接口。

appd 中没有轮询线程。应用程序选择何时调用 `take-action`，通常是从其有限事件循环中调用。这使得空闲 CPU 和内存使用量适合 512 MB CM0 目标。

## 设备接受

本地测试覆盖了无目标的严格框架、调用者隔离、支持动作过滤、四入口队列、生命周期清理、生成的SDK绑定以及Shell `SENT`、`UNAVAILABLE`、`BUSY`和`FAILED`叠加。物理验收仍需要一个媒体兼容测试应用和V0.6硬件上的真实`Fn+Q/W/E`键组合。
