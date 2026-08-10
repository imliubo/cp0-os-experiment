# 摄像头

<!-- doc-locale: zh-CN -->
> [English](README.md) | **简体中文**

相机从受信任的30 FPS摄像头流水线显示320x170的预览。
Enter或空格键将下一帧编码为1280x720的JPEG格式，并保存原始图像以及320x170的缩略图到共享照片库，而不重启预览。

![保存照片后的摄像头预览](assets/screenshot.png)

## 控制按钮

- Enter 或 空格: 捕获并保存一张1280x720的照片。
- `Esc`：通过 System Shell 退出应用。

第一次启动可能会显示对`camera.capture`和`photos.write`的权限提示。拒绝任一权限会使应用隔离，并产生明确的不可用或拒绝状态。

## 在模拟器中运行

```sh
cargo run -p cp0ctl -- run examples/camera \
  --duration 700 --permissions allow --keys enter \
  --output target/camera.ppm
```

相机是包含在产品镜像中的八个应用程序之一。
