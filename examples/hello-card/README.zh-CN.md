# 你好 Card

<!-- doc-locale: zh-CN -->
> [English](README.md) | **简体中文**

Hello Card 是产品量产镜像中包含的广泛 SDK 能力演示。左下角的彩色动作块报告成功、拒绝、不可用或内部错误；右下角的块标识 Runtime 最后交付的密钥。

![你好 Card 能力表面](assets/screenshot.png)

## 控制按钮

- `N`: HTTPS 网络请求。
- `D`: 可信文档选择器和受限读取。
- `P`: 播放一个短的生成音调。
- `R`: 捕获一段短的麦克风样本。
- `C`: 捕获并显示一个摄像头帧。
- `G`: 读取并切换逻辑 Grove GPIO 线。
- `L`: 接收一个受限的LoRa数据包。
- `S`: 将值写入私有应用存储。
- `I`: 将应用意图发送回Hello Card。

每项受保护的操作都由声明的manifest权限和受信任的代理服务进行调解。私有存储和应用意图不会暴露Linux路径或通用IPC通道。

## 在模拟器中运行

```sh
cargo run -p cp0ctl -- run examples/hello-card \
  --duration 500 --permissions allow --keys c,g,s \
  --output target/hello-card.ppm
```

Hello Card 是产品镜像中包含的八个应用程序之一。
