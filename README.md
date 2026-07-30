# CardputerZero OS

CardputerZero OS 是面向 CardputerZero V0.6（Raspberry Pi CM0、512 MB RAM、SD 卡、
320x170 LCD）的轻量应用操作系统。

项目采用单前台应用模型。第三方应用必须使用 CardputerZero SDK 开发并编译为
WebAssembly；应用不能直接访问 Linux 设备节点、系统总线或其他应用的数据。

## 当前状态

Phase 0 已启动，当前仓库包含：

- 系统架构与资源预算；
- 分阶段 Roadmap 和初始 ADR；
- CardputerZero SDK 的 WIT ABI 草案；
- 应用 manifest v1 和 Rust 校验库；
- `cp0ctl manifest validate` 开发工具。

## 快速验证

```sh
cargo run -p cp0ctl -- manifest validate examples/hello-card/app.json
make check
```

详细设计见 [系统架构](docs/ARCHITECTURE.md) 和 [Roadmap](docs/ROADMAP.md)。

