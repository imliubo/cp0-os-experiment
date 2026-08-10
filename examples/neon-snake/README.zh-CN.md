# 霓虹蛇

<!-- doc-locale: zh-CN -->
> [English](README.md) | **简体中文**

Neon Snake 是一个完整的 CardputerZero SDK 1.0 应用程序。它在受信任系统状态栏下方渲染一个 320x150 RGB565 表面，仅读取聚焦的键盘事件，并将最高分存储在应用的隔离私有存储中。

![霓虹蛇游戏板](assets/screenshot.png)

控制:

- 箭头键：操控
- 空间：暂停或恢复；
- 输入或 R：游戏结束后重启。

在仓库根目录中构建并运行它：在确定性PC模拟器中构建并运行它：

```sh
cargo run -p cp0ctl -- build examples/neon-snake
cargo run -p cp0ctl -- run examples/neon-snake \
  --duration 2400 --permissions deny \
  --keys up,left,down,right,space,space \
  --output target/neon-snake.ppm \
  --profile target/neon-snake.json
```

创建一个不可信的可重复生成的应用程序包：

```sh
cargo run -p cp0ctl -- package examples/neon-snake target/neon-snake.capp
```

在目标设备处于开发模式并且信任开发人员公钥时，请使用 `cp0ctl key generate`、`cp0ctl sign developer` 和 `cp0ctl install` 如 `docs/DEVELOPER-GUIDE.md` 中所述。

游戏请求不使用任何能力。其帧缓冲区、固定蛇阵数组和游戏状态均为调用者拥有的静态内存；没有分配器和没有Linux兼容API。

Neon Snake 是包含在生产镜像中的八个应用程序之一。
