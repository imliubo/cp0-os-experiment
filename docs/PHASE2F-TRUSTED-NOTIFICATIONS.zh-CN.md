# Phase 2F: 可信通知提示条

<!-- doc-locale: zh-CN -->
> [English](PHASE2F-TRUSTED-NOTIFICATIONS.md) | **简体中文**

## 演示合同

应用通知由 `appd` 排队，但只有经过身份验证的 System Shell 才能展示。Shell 通过受保护的 appd 控制套接字获取规范应用身份、显示名称和有界通知文本。应用不能选择横幅的可信表面、位置、颜色或生命周期。

私有 System Shell 协议 v4 增加 `notification` overlay mode。它让可信 Shell surface 保持在应用上方，同时只显示顶部 88 像素。standard 和 immersive 应用在横幅下仍然可见并保留键盘焦点。这与权限提示不同：权限提示会切换到不透明的全屏可信层并消费输入。

横幅显示标准应用名称，一个必需的标题和最多两行可选正文。它在四个一秒钟的Shell计时器tick后失效。主页、任务、电源、应用退出和权限提示可以取消当前横幅。权限和电源UI在状态重叠时也会防御性地抑制通知渲染。

## 边界和失败行为

- Shell 只接受匹配请求 ID 和 `next-notification` 响应类型的 appd 响应。
- 通知ID、应用ID、应用名称、标题和正文被解析为固定大小的缓冲区，没有未绑定的分配。
- 空队列与错误响应不同；错误数据会被忽略，不会改变当前的UI状态。
- appd 列表响应每帧最多包含八个摘要，因此其最坏情况仍然低于 8 KiB 控制协议限制。
- 应用程序ID在整个 compositor 激活路径中使用manifest的最大值128字节。

纯 C 测试覆盖有效和空通知响应、响应身份、空 body、状态清除和权限优先级。320x170 像素快照锁定可信横幅布局。compositor profile 测试锁定协议 v4、notification enum 和 128 字节应用 ID 限制。

## V0.6 验证

AArch64 Shell、Weston policy module、appd 和 `cp0ctl` 已热部署到 V0.6 设备，无需重新
烧录镜像或重启。安装后的哈希如下：

- System Shell: `f4d375c0798818ef3529471dabb80ed0c9d7f0a04da756b075b1a4961bd5cbdc`;
- Weston 策略: `37c336e95f6860a4fcfbcb180b892e499dacf529a7c8e5193d8ad96e94ef7420`;
- appd: `2ea26313737eb968cc04871addea63bbe04616d6547a76ab07218b83a8262f4d`;
- cp0ctl: `a7743b43eb2fdab37d1e83d5a592ef79777614c196432475167afbbcb7f080bc`.

Hello Card生成了一个标准的`notifications.post`提示。两者`once`和
持久化`always`决策都被执行了。Shell日志记录将每个`notification=<id> visible`事件与四秒后的`expired`事件配对。
由于桌面摄像头缓存了短暂的帧，一个有界的瞬态测试服务每五秒重复启动已授权的应用程序。
4K Camera2检查显示物理LCD从Home切换到受信任的`HELLO CARD / HELLO CODEX / NATIVE HOST CALL IS ACTIVE`横幅，然后再回到Home。
当横幅存在时，像素区域帧差异从摄像头噪声的2.6上升到11.19。

临时测试自行退出，应用目录返回了`running: false`，而 compositor、System Shell 和 appd 都保持活跃。
对于`notification`模式的应用焦点保留是由 compositor 强制执行并由策略测试覆盖的；最终的物理启动器激活检查在第二阶段路线图中单独跟踪。
