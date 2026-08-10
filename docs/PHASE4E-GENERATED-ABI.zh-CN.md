# 第4E阶段：生成的宿主机ABI

<!-- doc-locale: zh-CN -->
> [English](PHASE4E-GENERATED-ABI.md) | **简体中文**

## 单一扁平契约

`sdk/abi/cardputerzero-hostcalls-v1.json` 是 WAMR 标准导入合同。
每个条目固定了模块/名称对、实现函数、C 符号，
参数所有权类型、结果类型、WAMR 指针绑定签名、本地测试回退以及相应的公共 WIT 操作。

`scripts/generate-sdk-bindings.mjs` 验证合约并产生：

- `app-runtime/src/hostcall_symbols.inc`，由Runtime注册表包含
- `sdk/c/include/cardputerzero_imports.h`，包含在公共C类型之后；
- `sdk/rust/src/host_imports.rs`, 唯一一个允许声明原始 WebAssembly 导入的 Rust 模块。

生成的输出会被提交，因此构建镜像不需要Node。CI会在`--check`模式下运行生成器，并拒绝过时的输出、重复的映射、无效的指针边界或WAMR签名/类型不匹配。

## WIT 关系

WIT 仍然是类型化、语言中立的公共 SDK 合约。它现在与实现的 API 相匹配：有界事件等待和聚焦键盘输入存在；未实现的日志记录和生命周期回调草稿已被移除。每个公共 WIT 主函数恰好有一个平坦的 ABI 映射。纯粹的 SDK UI 辅助函数不需要 WIT 操作。

CM0 继续通过 WAMR 执行核心 WebAssembly，而不是组件模型，以保持内存和启动成本处于可控范围。生成的 SDK 面具将 WIT 值，如字符串、列表、结果和选项降低为调用者拥有的扁平缓冲区和打包标量结果。应用程序从不直接调用扁平导入。

生成器的离线结构检查验证WIT包版本、接口/功能映射和平衡的接口块。此外，SDK 测试套件解析并解决完整的合同，使用标准Bytecode Alliance `wit-parser` 由 `wasm-tools`；它验证1.1.0 包身份、接口数量和导出的应用世界。这只是一个构建时测试依赖项，而不是运行时或产品镜像依赖项。

## 兼容性

`sdk/abi/compat/cardputerzero-hostcalls-0.1.json` 和
`sdk/abi/compat/cardputerzero-hostcalls-1.0.json` 记录每个发布的名称和 WAMR 签名。测试需要两个快照中的所有条目都保持存在且不变。兼容的次要版本可以添加导入；删除导入或更改签名需要一个新的 SDK 主版本。

冻结的1.0 兼容基线包含22个有界宿主调用，并且字节对字节ABI与遗留的0.1版本兼容。当前SDK 1.1增加的合同包含35个宿主调用，包括HTTPS范围和48 kHz立体声PCM；所有22个基线名称和签名保持不变。C11、C++17和Rust WASM构建使用生成的声明，而Runtime构建使用相同的签名源。应用程序安装接受SDK 1.1、兼容的1.0版本和确切的遗留0.1版本，但不接受其他预1.0版本。

使用生成后的注册表重新构建固定版本 WAMR 2.4.5 AArch64 静态 Runtime 成功。Phase 4E
制品的 SHA-256 为
`1fc27bf80953f16a0840ea82a2fcfc17590b58c09e9ff6aa879a154d9e05130a`。
