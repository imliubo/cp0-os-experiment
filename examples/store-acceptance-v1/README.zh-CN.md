# Store 接收 v1

<!-- doc-locale: zh-CN -->
> [English](README.md) | **简体中文**

Store Acceptance v1 是用于确定性 Store 安装、中断和升级接受流程的初始 1.0.0 载荷。它没有任何控制或权限，并不打算作为用户应用程序。

![Store 接受 v1 绿色载荷](assets/screenshot.png)

白色标题和绿色主体使版本1在安装后视觉上易于区分。使用模拟器运行它：

```sh
cargo run -p cp0ctl -- run examples/store-acceptance-v1 \
  --duration 250 --permissions deny --keys '' \
  --output target/store-acceptance-v1.ppm
```

使用 `scripts/build-test-store.sh` 生成带符号的 v1/v2 目录，并使用 `scripts/device-store-acceptance.sh` 生成完整的设备序列。此负载不包含八前台应用产品的镜像。
