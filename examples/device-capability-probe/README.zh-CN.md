# 设备能力探测

<!-- doc-locale: zh-CN -->
> [English](README.md) | **简体中文**

设备能力探测是一个自动化的工厂和验收应用程序。它测试有界音频播放、麦克风捕获、逻辑 Grove GPIO 线以及私有存储配额的行为。它不是用户应用程序。

![能力探测结果带](assets/screenshot.png)

四个水平条代表播放、捕获、GPIO和存储。绿色表示成功，蓝色是预期的第一运行存储配额结果，黄色表示不可用或资源受限，红色表示被拒绝，品红色表示失败。还发布了一个机器可读的摘要作为通知，并保存到私有存储中。

## 在模拟器中运行

```sh
cargo run -p cp0ctl -- run examples/device-capability-probe \
  --duration 5000 --permissions allow --keys '' \
  --output target/device-capability-probe.ppm
```

使用 `scripts/device-capability-acceptance.sh` 代表设备上的完整流程。
此探针不包含在八款应用的生产镜像中。
