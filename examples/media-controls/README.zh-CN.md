# 媒体控制

<!-- doc-locale: zh-CN -->
> [English](README.md) | **简体中文**

这个 SDK 1.0 示例将前台应用注册为媒体会话，并消费受信任的 System Shell 的播放/暂停、上一首和下一首操作。它不播放音频，因此不需要请求任何权限。

![媒体控制在全局播放操作后的状态](assets/screenshot.png)

## 控制按钮

- `Fn+Q`: 播放/暂停。
- `Fn+W`: 上一页。
- `Fn+E`: 下一个。
- 空间：应用程序本地的播放/暂停备用方案。

在确定性模拟器中运行所有三个全局动作：

```sh
cargo run -p cp0ctl -- run examples/media-controls \
  --duration 600 --permissions deny \
  --media-actions play-pause,previous,next \
  --output target/media-controls.ppm \
  --profile target/media-controls.json
```

在V0.6硬件上，`Fn+Q`、`Fn+W`和`Fn+E`是受信任的全局操作。空格是应用本地的播放/暂停备用。实际声音输出是一个单独的功能，并需要`audio.playback`声明权限。

媒体控制是产品镜像中包含的八个应用程序之一。
