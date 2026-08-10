# V0.6 启动屏资源

<!-- doc-locale: zh-CN -->
> [English](README.md) | **简体中文**

`splash.png` 是 320x170 的源图像。一个有界的静态 initramfs 辅助程序在 Linux 显示驱动加载之前初始化 SPI0 和 ST7789，然后直接将 `splash.rgb565` 发送到面板 RAM。当 DRM 出现时，相同的资源会通过 Linux 的帧缓冲区重绘。这保留了 64 MB VideoCore 的分割和标准的具有摄像头功能的 Raspberry Pi 固件。帧缓冲区路径使用启动范围内的标记和原子锁，因此 initramfs 工作进程和 systemd 回退不能并发地重绘相同的帧。

官方 `cardputerzero_v0.6` 仓库镜像在 ARM 内核之前显示一个 170x320 RGB565 BMP，来自自定义 `m5stack_bootscreen` VideoCore 固件。这比 initramfs 辅助程序更早，但不透明的固件忽略了产品的 64 MB GPU 预算，只留给 Linux 大约 227 MiB。生产镜像门故意拒绝了它； `splash.png` 仍然是有界 Linux 渲染路径和受信任的 Wayland 手递的规范用户资产。帧缓冲区图像在冷启动稳定窗口中仍然可见；然后 Weston 呈现相同的图像，直到第一个完整的 Setup 或 Home 表面被映射。

使用 FFmpeg 重新生成原始帧：

```sh
ffmpeg -i splash.png -frames:v 1 -f rawvideo -pix_fmt rgb565le splash.rgb565
```

锁定哈希：

```text
17b6b5571fd3be038992df24134d7ca88c75b22cb36e84cf2f007664096298e1  splash.png
75a53d81f5ec087536a030919698c595630d48296e07d5f5f3d04ebebf2efd57  splash.rgb565
```
