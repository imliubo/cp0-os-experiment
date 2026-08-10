# 存储隔离探测器

<!-- doc-locale: zh-CN -->
> [English](README.md) | **简体中文**

存储隔离探测器验证它不能在不同应用程序身份下读取设备能力探测器写入的标记。这是一个接受载荷，不是用户应用程序。

![存储隔离探测成功结果](assets/screenshot.png)

当没有外来标记可见时，屏幕为绿色；当数据跨应用身份泄露时，屏幕为红色；当存储返回错误时，屏幕为品红色。结果还会发出通知，并存储在该应用的私有命名空间中。

## 在模拟器中运行

```sh
cargo run -p cp0ctl -- run examples/storage-isolation-probe \
  --duration 1000 --permissions allow --keys '' \
  --output target/storage-isolation-probe.ppm
```

在设备上运行它，需要在能力探测之后使用 `scripts/device-capability-acceptance.sh`。它不包含在八款应用的生产镜像中。
