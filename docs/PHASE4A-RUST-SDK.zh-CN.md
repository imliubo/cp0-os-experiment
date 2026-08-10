# 阶段 4A: Rust SDK 基础

<!-- doc-locale: zh-CN -->
> [English](PHASE4A-RUST-SDK.md) | **简体中文**

## 公共应用API

`sdk/rust` 是第一个支持的 CardputerZero 应用程序 SDK。它是一个无需依赖的 `no_std` 箱子，适用于 `wasm32-unknown-unknown`；应用程序代码不需要声明 WAMR 导入或依赖 Linux API。

这一阶段最初引入了预发布SDK 0.1 API。相同的公共接口现在是冻结的SDK 1.0的一部分，并提供了：

- `system::monotonic_milliseconds()`；
- `system::wait_event()`，最大等待时间为1秒；
- `system::post_notification()` 与manifest能力代理服务结合；
- 稳定的 `Denied`, `Unavailable`, `InvalidArgument`, `ResourceLimit` 和 `Internal` 错误。

SDK 在跨过WASM边界之前验证字符计数和控制字符。运行时私有的整数状态码和导入符号保持封装。待处理的权限提示映射到`Unavailable`，允许单个前台事件循环重试而不阻塞。

你好，现在Hello Card 只依赖于`cp0-sdk`。它的源码没有任何原始的`extern "C"`、套接字、路径或权限协议知识。

## 运行时 ABI

Runtime 添加了 `cp0_monotonic_milliseconds: () -> i64`，它是通过 `CLOCK_MONOTONIC` 实现的。它补充了现有的有界等待和类型化的通知调用。WIT 仍然是源级合约。Phase 4E 将所有手动维护的私有导入替换为从标准扁平 WAMR ABI 合约生成的绑定，同时保留公共 Rust API。

## 验证

- 工作区单元测试覆盖SDK错误映射和宿主机上的参数限制。
- SDK 和 Hello 在 `wasm32-unknown-unknown` 上构建成功。
- 最终 aarch64 运行时 SHA-256：
  `8cb76b9e34309a5a85adb0999d132d8a2eaf50975ea66854d75dc407cd9aeccd`.
- SDK 基础 Hello WASM SHA-256：
  `d1830261bec651deb3cabc35f05e8bf524a97fd136c61b9cefc68da87d91eff6`.
- 在V0.6版本中，Hello通过appd启动，通过SDK发布了通知ID4，并干净地停止。Appd、compositor和System Shell保持活跃。

## 项目工作流程

第一个宿主机开发命令是：

```sh
cp0ctl new <directory> <app-id> <display-name>
cp0ctl build <directory>
```

`new` 拒绝覆盖现有路径，在写入之前验证生成的清单，并创建一个 `no_std` cdylib，其中没有私有 Runtime 导入。在 SDK 1.0 发布到开发者注册表之前，其 Cargo 依赖指向当前检出中的 canonical SDK 路径。

`build` 验证 `app.json`，读取 Cargo 的结构化元数据以找到实际 cdylib 目标和目标目录，构建 `wasm32-unknown-unknown`，并在 `target/cardputerzero/<app-id>/<version>` 下构建一个包形状的树。测试执行完整的生成项目构建而不是仅检查模板。

## 图像集成

`image/build-image.sh` 在调用 pi-gen 之前构建固定 aarch64 appd、Runtime 和 Hello 构建体。`02-app-platform` 阶段只安装这些发布构建体，而不安装编译器工具链。它创建预留的 UID/GID 20000，私有数据目录，根拥有的包，然后请求`cp0-appd register-installed` 创建规范的注册表，而不是手动编写受信任的状态。

两个套接字单元都已启用，现在 compositor 默认启动。阶段有离线配置文件测试；完整的镜像构建和烧录延迟到剩余启动/只读根文件系统的工作准备好进行一次集中硬件验证周期为止。

## C和C++ ABI

`sdk/c/include/cardputerzero.h` 向独立的 C11 和 C++17 应用程序暴露相同的 Runtime 导入。它定义了 SDK/ 显示常量和稳定的结果代码，但没有 WASI 或 Linux 接口。Clang 导入属性将符号绑定到 `cardputerzero` 模块，并且显式的 UTF-8 指针和长度匹配 WAMR 的检查过的 `*~` 参数。

Emscripten 烟雾测试将 C 和 C++ 代码单元编译为 WebAssembly 对象，并将警告视为错误。它生成的缓存仍然位于被忽略的仓库 `target/` 目录下。在完整 SDK 能够被称为发布之前，仍需打包固定版本的独立 C/C++ 工具链。
