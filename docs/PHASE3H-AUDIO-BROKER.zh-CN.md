# Phase 3H: 受限音频代理服务

<!-- doc-locale: zh-CN -->
> [English](PHASE3H-AUDIO-BROKER.md) | **简体中文**

## 范围

音频 API 提供了有界同步 PCM 播放和捕获操作，而不向 WASM 应用程序暴露 ALSA、混音控制或设备选择。设备合同固定为 CardputerZero V0.6 ES8389：

- ALSA 终端点：`hw:ES8389Audio,0`;
- 格式：带符号的16位小端PCM
- 遗留/通用音频：16 kHz单声道，最多1024帧，2048字节或64 ms；
- SDK 1.1 音乐播放：48 kHz 立体声，每次调用最多1920帧，7680字节或40 ms。
- 权限：`audio.playback` 和 `audio.capture` 是独立的。

在受信任路径中使用这两种确切格式可以避免谈判、编解码器和容器解析器。较长的声音和录音由重复的有界调用构建。原始的ALSA ioctls、混音更改、任意采样格式和HDMI音频不是SDK功能。PCM WAV解析保留在沙箱中的音乐应用中；未来的压缩格式应属于一个单独的有界系统解码器。

V0.6 ES8389 硬件 PCM 端点以 48 kHz 运行，并且恰好有两个交错通道。Audiod 在音频活动时重用一个播放句柄，然后在 200 ms 内没有完成写操作时关闭它，以防止闲置的编解码器路径使扬声器发出嘶嘶声。50 ms 生命周期检查与播放共享设备互斥量，因此它不能在写操作期间关闭句柄。暂停和停止不会改变用户的混音静音状态；后续播放会重新打开固定端点。SDK 1.1 音乐帧直接发送，通过将每个 16 kHz 样本重复三次并复制到两个硬件通道来保留单声道 SDK 合同。捕获保持在硬件边界处以 16 kHz 双声道运行，并将左/右样本平均回一个单声道样本。应用程序不能选择或观察硬件布局。
捕获流在首次读取之前显式启动，因为 V0.6 驱动程序返回 `EIO` 而不是执行 ALSA 的隐式启动。

只有可信的 System Shell 使用音频协议 v3 的输出设置和按键音命令。
它们固定在ES8389 `DACL`、`DACR` 和 `Speaker` 简单混音器元素上；
请求不接受任何声卡、元素或路由名称。音量限制在 0% 到 100%，快捷调节采用固定 10% 步长，每次写入返回实际音量和静音状态。持久化按键音设置由 audiod 管理，因此 System Shell 和前台应用的按键事件遵循同一策略。输入密码和 Wi-Fi 密钥时保持静音。

可听见的键提示是一个固定的12 ms、16 kHz单声道PCM的UI SFX Soft衍生物
`typing` (CC0-1.0). Audiod 在写入cue到ALSA之前附加20 ms的零值样本：32 ms的提交超过了端点的周期对齐的20 ms自动启动阈值，而可听见的尾部保持12 ms。在服务模式下， `play-key-click` 将一个令牌放入一个有八项上限的队列中，并在触摸ALSA之前返回。一个专用的128 KiB堆栈工人按顺序处理每个接受的令牌。队列满时，会施加回压而不是丢弃关键反馈。在Key Sounds禁用时，触及硬件访问之前会丢弃这些令牌。

## 信任流

```text
WASM audio SDK call
  -> Runtime validates one linear-memory pointer/length pair
  -> Runtime sends bounded base64 PCM over its only Unix broker socket
  -> appd derives identity from SO_PEERCRED
  -> appd verifies active systemd cgroup and root-owned manifest
  -> permission engine evaluates playback or capture independently
  -> appd forwards to root-only cp0-audiod socket
  -> cp0-audiod validates the protocol again and accesses ES8389 through ALSA
```

`cp0-audiod` 以 `cp0-audio` 运行，并仅包含标准 `audio` 补充组。其能力集为空。systemd 使用 `DevicePolicy=closed` 并仅授予 `char-alsa rw`；所有其他设备节点仍被拒绝。该服务仅包含 `AF_UNIX`，内存限制为 16 MiB，无交换空间，任务数为八，系统视图只读，并且有一个私有状态目录用于 Key Sounds 布尔值。

该服务动态解析小 `libasound.so.2` PCM API，因此跨编译的 Rust 二进制文件不需要宿主机 ALSA 头文件或链接时的 sysroot 库。它选择命名的 ES8389 卡而不是 ALSA 卡号零，防止 HDMI 列表更改将应用程序音频重定向到其他位置。

## 协议和失败行为

Runtime-to-appd和appd-to-audiod协议是严格的新行分隔的JSON帧，分别限制在16 KiB和12 KiB。二进制样本使用标准base64编码。每一层都拒绝空数据、对齐错误、过大或非标准数据，拒绝捕获帧计数在1到1024之外，音乐请求在1到1920立体声帧之外，请求ID不匹配和返回长度不匹配的数据。

忙碌的设备映射到稳定的SDK资源限制结果。缺少ALSA支持，
设备故障和待处理的权限提示映射到不可用。拒绝或未声明的权限映射到拒绝。本地ALSA错误字符串和主机设备详细信息从不跨入应用。

audiod 套接字只能由 `cp0-audio-control` 穿越。`cp0-audiod` 使用 `SO_PEERCRED` 授权 root/appd 仅用于 PCM 捕获和前台应用键点击，而 `cp0-shell` 接收输出设置、键音策略和 Shell 键点击。Shell 仍然不能提交任意 PCM，而 appd 仍然不能改变音量或持久键音设置。套接字 成员身份本身从不授予命令。

## 验证

自动化覆盖包括：

- 协议往返次数、最大帧数和畸形的 canonical-base64 情况；
- 仿真设备mono/stereo播放，精确捕获和无效长度分发；
- appd-to-audiod 请求/响应关联;
- appd 中分离的 manifest 权限路由；
- 运行时捕获解码和长度不匹配拒绝
- 缓存 48 kHz 播放，16 到 48 kHz 转换，键点击启用/禁用，
静默阈值填充，audiod 重启后持续存在，禁用时快速点击队列无损清空，以及禁用时无硬件访问；
- Rust、C11、C++17 和 WIT SDK 接口；
- 加固的 systemd 服务和 image-stage 断言；
- AArch64 audiod/appd 和静态 Runtime 加上 wasm32 Hello Card 构建。

设备报告ES8389播放和录制为`card 0, device 0`；HDMI是一个单独的只播放卡。真实身份允许/拒绝探测和结果框架在`PHASE3M-DEVICE-CAPABILITY-ACCEPTANCE.md`中有文档说明。SDK 1.1 音乐、不间断键击行为以及延迟/欠采样接受仍然需要产品捆绑包或镜像和物理V0.6验证。
