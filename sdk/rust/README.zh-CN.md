# CardputerZero Rust SDK 1.1

<!-- doc-locale: zh-CN -->
> [English](README.md) | **简体中文**

这个 `no_std` 箱是 CardputerZero 应用程序支持的 Rust API。
应用程序编译为目标 `wasm32-unknown-unknown`，并且必须不声明私有
Runtime 直接导入。

`lifecycle` 模块定义可选的多任务 checkpoint 契约及其 8 KiB 上限。应用通过导出
`cp0_app_checkpoint` 和 `cp0_app_restore` 并使用稳定的 core-WASM 签名来选择加入；
不提供这些导出的应用仍然有效，并会在容量回收后从干净状态重启。

SDK 1.1 API 暴露了显示和聚焦输入、单调时钟、有界事件等待、通知、文档、受限的 HTTPS GET/Range、固定格式 PCM 音频、固定帧相机、逻辑 GPIO、LoRa、私有存储和意图能力。`network::http_get` 接受一个由调用者拥有的最多 2048 字节的缓冲区。`network::http_get_range` 接受最多 8 KiB 的数据，且必须在一个 256 MiB 资源的精确偏移处。两者都只返回 HTTP 状态和主体长度。`Error::Unavailable` 表示一种能力可能正在等待 System Shell 的权限决定，或者一个临时服务可能不可用，但可以稍后重试。

`input::KeyEvent::character` 包含平台翻译后的可打印 ASCII 字节，用于文本输入，或 `None` 用于导航、修饰和释放事件。它使用与 System Shell 相同的 V0.6 映射，因此应用程序不需要解释 evdev 键码或实现自己的 Shift/`Sym` 表。原始的 `code` 和 `modifiers` 仍然可供非文本控件使用。

`audio::play_pcm_s16le` 和 `audio::capture_pcm_s16le` 至多接受 1024 个 16 位带符号单声道帧，采样率为 16 kHz。应用程序清单必须分别声明播放和捕获。`audio::play_pcm_s16le_stereo_48khz` 至多接受 1920 个交错的 48 kHz 双声道帧，并使用相同的播放权限。没有 API 暴露编解码器、ALSA 设备、混音控制或格式协商。

`camera::capture_rgb565` 填充一个由调用者拥有的320x170 RGB565 预览帧。
`camera::capture_photo` 暂停预览并返回一个系统管理的1280x720 JPEG 图片及其图库缩略图的ID；JPEG 不会被复制到WASM 内存中。
`photos::load_view_rgb565` 从原始图像中渲染一个固定的320x170 视口。
使用 `ViewZoom::{Fit, Half, Actual}` 和边界限定的平移坐标；SDK 从不暴露原始JPEG 图片或其存储路径。
应用程序清单必须声明 `camera.capture`；SDK 不暴露任何传感器选择、相机设备、捕获过程或文件路径。

`gpio::read` 和 `gpio::write` 只接受 V0.6 连接器功能定义的四个 `gpio::Line` 变体。它们暴露布尔值而不是 Linux gpiochip 编号、设备路径、引脚方向或引脚复用配置。

`storage::put`、`storage::get` 和 `storage::delete` 在 manifest 的 `storage_mb` 配额内提供私有键/值存储。存储隔离是自动的，不需要 manifest 权限。

`intents::send` 将一个反向域动作和最多1024字节的负载路由通过 appd。`intents::take` 只返回当前应用绑定的下一个消息并消耗它一次。发送者不能命名目标应用或连接到另一个应用的进程。

`media::update_session` 寄存器记录当前前台 Runtime 的播放状态和一组有界的全局动作。`media::take_action` 消费一个播放/暂停、上一首或下一首动作。API 不包含目标应用或媒体元数据，并不授予单独的`audio.playback`权限。

`ui::Canvas` 是为 320 像素宽显示设计的无分配引用渲染器。它提供了裁剪的 RGB565 矩形，一个紧凑的 5x7 字体，覆盖所有可打印的 ASCII 字符，并具有不同的大写和小写字符，还提供了按钮和进度条。应用程序拥有自己的帧缓冲区，并通过 `display::present_rgb565` 提交；SDK 从未创建 Linux 窗口或暴露帧缓冲区设备。

在PC上构建并运行一个SDK应用程序：

```sh
cargo run -p cp0ctl -- build examples/calculator
cargo run -p cp0ctl -- run examples/calculator --keys 1,2,plus,3,equal
```

请参阅 `docs/DEVELOPER-GUIDE.md` 以获取完整的软件包、权限、模拟器、签名和设备安装工作流程。
