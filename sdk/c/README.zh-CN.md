# CardputerZero C/C++ SDK 1.1

<!-- doc-locale: zh-CN -->
> [English](README.md) | **简体中文**

从一个独立的 Clang C11 或 C++17
项目包含 `include/cardputerzero.h`，该目标针对 `wasm32-unknown-unknown`。头文件仅声明公共 CardputerZero 运行时导入；它不暴露 WASI、Linux 系统调用或本地链接。

`include/cardputerzero_imports.h` 中的原始导入声明来自 `sdk/abi/cardputerzero-hostcalls-v1.json`。应用程序仅包括 `cardputerzero.h`；直接使用生成的原始函数是不支持的。`tests/test-sdk-abi.sh` 确保 C 声明、Rust 导入和 Runtime 注册表字节对字节同步与合约保持一致。

`cp0_key_event_t.character` 是平台翻译后的可打印ASCII字节；零表示该事件没有文本。它遵循与首次启动和System Shell相同的Shift键和V0.6 `Sym` 映射。应用程序使用此字段进行文本输入，并使用`code`进行导航或游戏控制，而不是维护一个私有的evdev到字符的映射表。

希望可恢复任务驱逐的应用可以导出`cp0_app_checkpoint`和`cp0_app_restore`，并使用`cardputerzero.h`中的声明。Runtime拥有临时线性内存缓冲区，最多复制8 KiB，并拒绝schema版本零。回调是可选的；一个在容量驱逐后省略它们的应用将在干净状态下重启。

字符串是带有显式长度的 UTF-8 字节缓冲区。应用程序应将通知标题保持在 32 个 Unicode 字符以内，将正文保持在 160 个字符以内；运行时和代理在信任边界上再次遵守字节、编码和字符限制。

`cp0_http_get` 接受一个 HTTPS URL 和一个由调用者拥有的响应缓冲区，该缓冲区不超过 2048 字节。SDK 1.1 添加了 `cp0_http_get_range`，具有确切的偏移量，并且每调用 8 KiB/每资源 256 MiB 的流式传输绑定。两者仅返回一个有界 HTTP 状态/体长度记录。SDK 故意不暴露 POSIX 套接字、DNS、TLS 覆盖或任意头 API。

`cp0_audio_play` 和 `cp0_audio_capture` 交换调用者拥有的 16 位带符号 PCM 缓冲区。它们的兼容格式固定为 16 kHz 单声道 S16_LE，并且一次调用最多限制为 1024 帧。SDK 1.1 还提供了 `cp0_audio_play_stereo_48khz`，最多每调用提供 1920 交错立体声帧。播放和捕获需要单独的声明权限；SDK 暴露没有 ALSA 设备、混音器、编解码器或格式协商 API。

`cp0_camera_capture` 填充恰好一个由调用者拥有的 320x170 RGB565 预览帧。`cp0_camera_capture_photo` 返回系统管理的 1280x720 JPEG 的照片 ID 以及相册缩略图。预览需要`camera.capture`；静止图像捕获还需要`photos.write`。应用程序不能选择传感器、访问 V4L2 或接收原生描述符。

`cp0_gpio_read` 和 `cp0_gpio_write` 只接受 `cp0_gpio_line_t`。该枚举包含四个 V0.6 逻辑连接器输出；它故意不能表示 BCM GPIO 编号、gpiochip、路径、引脚方向或引脚复用模式。

`cp0_lora_send` 和 `cp0_lora_receive` 与固定的外部 SX1276 配置最多交换 64 字节。应用程序不能选择 SPI、频率、调制或功率。该镜像在根提供有效区域配置之前使无线电保持禁用状态。

`cp0_storage_put`、`cp0_storage_get` 和 `cp0_storage_delete` 提供私有键/值存储。键绑定到 64 个安全 ASCII 字节，值绑定到 8 KiB；安装的清单文件的 `storage_mb` 字段由存储代理强制执行。运行时不会暴露可写的宿主机文件系统。

`cp0_intent_send` 和 `cp0_intent_take` 提供由 Manifest 路由的应用间交接。action 是最长
96 字节的反向域名 ASCII 名称，payload 最大 1024 字节，取出的消息只消费一次。SDK 不会
暴露目标 App、套接字或原生 IPC handle。

`cp0_media_session_update` 注册前台应用的播放状态和支持的播放/暂停、上一首和下一首操作。
`cp0_media_take_action` 消费一个由 appd 路由的动作。应用程序不能命名另一个应用程序或附加媒体元数据，注册也不替代 `audio.playback` 权限，该权限用于声音输出。`cp0_photos_*` 调用暴露单独权限管理的共享照片库。使用 `cp0_photos_import_rgb565` 以原子方式添加一个固定帧，使用 `cp0_photos_remove` 以原子方式移除一个选定 ID；应用程序不直接更新相册索引。`photos.read` 和 `photos.write` 保持独立权限。分页格式和迁移合同在 `docs/PHOTO-LIBRARY-V2.md` 中描述。
`cp0_photos_load_view_rgb565` 返回固定大小为 320x170 的 Fit，半分辨率或 1:1 视口的 Camera JPEG 原始图像。平移坐标从 `CP0_PHOTO_VIEW_PAN_MIN` 归一化到 `CP0_PHOTO_VIEW_PAN_MAX`；原始字节和路径保持代理私有。
