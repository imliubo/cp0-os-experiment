# Phase 3I: 限制摄像头代理

<!-- doc-locale: zh-CN -->
> [English](PHASE3I-CAMERA-BROKER.md) | **简体中文**

## 范围

摄像头API不向WASM应用程序暴露V4L2、Media Controller、dma-heap、VideoCore设备、本地描述符或捕获过程参数。它们的固定合同是：

- 权限: `camera.capture`;
- 前台预览：320x170 RGB565 小端序，目标帧率为30 FPS，V0.6传感器安装时旋转180度；
- 照片捕获：1280x720 JPEG格式加上一个320x170 RGB565格式的Gallery缩略图；
- 预览结果：调用者拥有的WASM内存恰好包含108800字节；
- 照片结果：一个代理拥有的`photo_id`；JPEG 文件从未进入WASM内存。

在SDK 1.0中没有传感器选择、任意分辨率、编解码器、容器或输出路径。`camera::capture_photo()`还需要`photos.write`，从同一个直播管道编码下一帧，并原子地存储原始图像和缩略图，而不停止或重启传感器。

## 信任流

```text
WASM camera SDK call
  -> Runtime validates one exact 108800-byte linear-memory range
  -> appd derives identity from SO_PEERCRED and active systemd cgroup
  -> appd checks the root-owned manifest and camera.capture decision
  -> appd requires the caller to be the System Shell's current foreground runtime
  -> root-only cp0-camerad socket accepts only appd
  -> cp0-camerad keeps one fixed 1280x720 @ 30 FPS /usr/bin/rpicam-vid YUV420 stream
  -> preview frames are downscaled to 320x170 RGB565_LE at a 30 FPS target
  -> photo requests send the next frame to the fixed /dev/video31 JPEG encoder
  -> a bounded planar-YUV software encoder remains available as a fail-safe
  -> sealed memfd is reopened read-only and sent with SCM_RIGHTS
  -> appd verifies type, length, access mode and all write seals
  -> Runtime repeats metadata/seal checks and copies pixels into WASM memory
```

`cp0-camerad` 以 `cp0-camera` 身份运行，仅包含 `video` 辅助组，空的能力集，没有网络地址族，也没有可写的系统目录或家目录。`DevicePolicy=closed` 只授予视频4linux、媒体、dma-heap 和 `/dev/vchiq`，这些都是 Raspberry Pi 摄像头管道所需要的。应用程序沙盒没有这些设备。该服务及其摄像头子进程以 `Nice=10` 和 `CPUWeight=10` 运行，因此当受限的 CM0 忙碌时，摄像头工作会让位给 compositor 和键盘路径。

两种服务协议都是严格的新行分隔的JSON。私有摄像头协议限制在2048字节，并允许一个CLOEXEC描述符。预览数据从未进入JSON：代理传递一个只读的不可变描述符，Runtime执行最终的有界复制。静止照片返回给appd作为一个包含固定缩略图和最多4MiB JPEG数据的有界描述符。appd验证信封并直接将其提交给系统照片库。缺失、可写、未密封、非常规或大小不正确的描述符会关闭失败。

## 图像和硬件状态

该应用平台镜像安装了`rpicam-apps-lite`；之前的最小镜像移除了它，因为当时不存在相机服务。固定执行文件路径和参数向量由代理拥有，而不是由应用程序或清单拥有。连续的YUV420过程使用40 FPS的内部传感器目标，以便协议传输和下采样可以维持公共30 FPS的预览合同。它的进程和相机管道在预览和照片请求之间重用，并在两秒内没有请求后释放，因此冻结/后台的相机任务不会无限期地保留传感器。预览和JPEG质量90的照片捕捉都使用相同的固定1280x720、180度旋转的帧。避免第二个`rpicam-still`过程可以消除传感器发现和模式切换的延迟。进程创建在短寿命的后台线程上运行，每个前台请求最多等待50 ms的帧进度。在冷启动期间，camerad会保留子进程和任何部分YUV帧，直到重试20秒。在第一个完整帧之后，如果没有完整帧出现500 ms，则会丢弃并重建子进程。缓慢的libcamera发现因此可以在不阻塞相机输入循环的完整发现截止日期或进程创建延迟的情况下完成。
V0.6硬件JPEG路径直接接受流的平面YUV420帧，因此捕获不需要分配或转换完整的RGB888图像。如果那个固定的内核编码器不可用，软件回退也会直接读取平面YUV缓冲区，而不是构建一个RGB中间图像。曝光质量仍然是物理接受的一部分。

当前的V0.6镜像通过Unicam管道检测IMX219。物理预览吞吐量、帧布局/方向、前台撤销和输入延迟仍然需要在每次相机代理更改后协调设备接受。

## 验证

自动化覆盖包括：

- 严格的请求/响应框架和一对一文件描述符传输；
- 确切的1280x720 YUV420 输入尺寸，RGB565 缩放和JPEG编码；
- 边界为1280x720的JPEG封装和原图加缩略图交易；
- 前台运行时摄像头撤销和空闲管道释放；
- 固定元数据，超时和捕获失败映射；
- appd 和 Runtime 中的只读普通文件大小和 Linux 封印验证；
- `camera.capture` 表现权限路由和拒绝行为；
- Rust、C11、C++17 和 WIT SDK 接口；
- 加固的 systemd 服务，镜像包和安装断言；
- AArch64 camerad/appd/Runtime 和 wasm32 Hello Card 构建。

CardputerZero V0.6 硬件路径的设备基准是一次 23.6 ms 的热预览请求，随后一次
1280x720 JPEG capability broker 请求耗时 49.7 ms。该数据不包括一次性的 sensor/libcamera
冷启动和随后 appd 的照片库事务；Camera 已启动时按下快门不会再次承担冷启动开销。

你好，Card 将 `C` 绑定以捕获并在受信任的状态栏下方显示顶部 320x150 部分。绿色、红色、黄色和洋红色的状态标记分别表示成功、拒绝、 unavailable 和内部错误。
