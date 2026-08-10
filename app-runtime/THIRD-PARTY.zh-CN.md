# 第三方软件

<!-- doc-locale: zh-CN -->
> [English](THIRD-PARTY.md) | **简体中文**

CardputerZero App Runtime 静态链接 Bytecode Alliance 的 WebAssembly Micro Runtime
(WAMR) 2.4.5，版本固定在 `wamr.env` 中。

WAMR 采用 Apache 许可证 2.0 并附带 LLVM 例外。权威的许可文本在固定版本的 WAMR 源代码检出中分发，路径为 `LICENSE`，并且可以在以下链接获取：

<https://github.com/bytecodealliance/wasm-micro-runtime/blob/25bd7eb63e828e4bd242cc9b38d260b4b31c6605/LICENSE>

WAMR源代码中没有进行本地修改。CardputerZero 提供其自身的嵌入式可执行文件、构建配置和初始化后的 seccomp 策略。

Runtime 还静态链接了 Wayland 1.23.1 和 libffi 3.5.2。xdg-shell 客户端协议是从 wayland-protocols 1.44 生成的。确切的仓库和提交被固定在 `wayland.env` 中；CardputerZero 源补丁没有修改这三个源树中的任何一个。
