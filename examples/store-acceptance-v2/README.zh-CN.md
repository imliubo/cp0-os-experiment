# Store 接受 v2

<!-- doc-locale: zh-CN -->
> [English](README.md) | **简体中文**

Store Acceptance v2 是用于确定性 Store 恢复、更新和持久化接受流程的1.1.0升级载荷。它没有任何控制或权限，并不打算作为用户应用。

![Store 接受 v2 蓝色载荷](assets/screenshot.png)

白色标题和蓝色主体使版本2在更新后视觉上与版本1区分开来。使用模拟器运行它：

```sh
cargo run -p cp0ctl -- run examples/store-acceptance-v2 \
  --duration 250 --permissions deny --keys '' \
  --output target/store-acceptance-v2.ppm
```

使用 `scripts/build-test-store.sh` 生成带符号的 v1/v2 目录，并使用 `scripts/device-store-acceptance.sh` 生成完整的设备序列。此负载不包含八前台应用产品的镜像。
