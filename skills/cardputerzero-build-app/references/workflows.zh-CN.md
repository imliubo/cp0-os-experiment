# 应用程序开发流程

<!-- doc-locale: zh-CN -->
> [English](workflows.md) | **简体中文**

## Rust

从提取的DevKit中选择`cp0ctl`

```sh
export CP0_DEVKIT_ROOT=/path/to/cardputerzero-app-devkit
export PATH="$CP0_DEVKIT_ROOT/bin:$PATH"
export RUSTUP_TOOLCHAIN=$(awk -F '"' '$1 ~ /^rust_version = / { print $2 }' \
  "$CP0_DEVKIT_ROOT/devkit/toolchain.toml")
```

保持 `RUSTUP_TOOLCHAIN` 导出给 `new`、`build`、`run` 和 `package`，并托管测试。`doctor.sh` 检查可用性但不能更新其父 Shell，当前 `cp0ctl` 继承活动的 Cargo 工具链而不是覆盖它。有效的签名不能证明锁定的编译器生成了打包的 WASM。

在源代码检出中，将下面的 `cp0ctl` 替换为 `cargo run --quiet -p cp0ctl --`。

```sh
cp0ctl new ./my-app dev.example.my-app "My App"
cp0ctl manifest validate ./my-app/app.json
cp0ctl build ./my-app
cp0ctl run ./my-app --duration 1000 --permissions deny \
  --keys left,right,enter --output ./my-app/frame.ppm \
  --profile ./my-app/profile.json
```

对于媒体应用，使用单独的可信操作插件：

```sh
cp0ctl run ./my-app --duration 1000 --permissions deny \
  --media-actions play-pause,previous,next \
  --output ./my-app/frame.ppm --profile ./my-app/profile.json
```

Rust 应用程序是 `#![no_std]` `cdylib` crate 用于 `wasm32-unknown-unknown`. 保留生成的恐慌处理程序和导出 `main`. 仅使用公共模块
`cp0-sdk`；绝不能声明私有导入。

保持仅运行时导入，导出 `main`，帧存储和恐慌处理程序在 `#[cfg(not(test))]` 之后。将确定性状态转换放在普通函数中，以便 `cargo test` 可以使用宿主测试框架来执行它们。

可选的生命周期检查点导出是高级的，以模拟为主的API。
在添加它们之前，请阅读`platform-contract.md`，保持payload版本化且最多8 KiB，并保留一个干净启动路径。捆绑的模拟器尚未测试检查点/恢复功能。

对于共享照片的应用，使用 `photos::LIST_PAGE_PHOTOS` 和一个固定的相机尺寸像素缓冲区。测试两种权限模式；在一个模拟器运行中进行确定性的保存和读取可以验证完整的代理API。参见 `photos.md` 了解合同和失败状态。

## C 和 C++

这是一项高级SDK集成预览，不是准备发布的App工作流。`cp0ctl new`, `build`, `run` 和 `package` 当前仅接受Rust Cargo项目。在同等项目生成、最终链接、模拟器支持和打包支持到位之前，不要承诺分发C/C++ App。

使用 Emscripten 固定工具链仅使用 C11 或 C++17。包含 `sdk/c/include/cardputerzero.h`，以自由站立模式编译用于 WebAssembly，禁用 C++ 的异常/RTTI，并导出 `main` 及线性内存。原始生成的 `cardputerzero_imports.h` 是包装器内部的，不是应用程序 API。

DevKit 验证公共头文件和自由站立对象。它尚未提供受支持的最终链接配方。仅编译一个目标文件不足以证明一个 C/C++ 应用程序可以在 CardputerZero OS 上运行。

`sdk/lvgl` 只包含 CardputerZero LVGL 9 调用器；DevKit 不包含上游 LVGL 源代码。在 LVGL 源代码、链接器和包管道发布之前，请优先使用无分配的 Rust Canvas。

## 打包并签名

```sh
cp0ctl package ./my-app ./my-app.unsigned.capp
cp0ctl key generate /secure/developer.key ./developer.pub
cp0ctl sign developer ./my-app.unsigned.capp ./my-app.capp \
  /secure/developer.key
cp0ctl verify ./my-app.capp
```

只有在用户明确请求时才生成开发人员密钥。将密钥存放在源代码控制和应用程序包之外。分发存储独立审核签名；开发人员签名不是商店批准。

## 设备安装和日志

```sh
cp0ctl pair ./developer.pub ~/.ssh/cardputerzero_ed25519.pub workstation \
  --device OWNER@DEVICE_IP
cp0ctl install ./my-app.capp --device OWNER@DEVICE_IP
cp0ctl logs dev.example.my-app 100 --device OWNER@DEVICE_IP
cp0ctl app start dev.example.my-app --device OWNER@DEVICE_IP
cp0ctl app stop dev.example.my-app --device OWNER@DEVICE_IP
cp0ctl app uninstall dev.example.my-app --device OWNER@DEVICE_IP
```

安装前请确认首次启动 Setup 已完成，并且设备未在运行稳定性、recovery、update 或 factory acceptance。启动应用会使正在进行的稳定性测试失效。所有者必须在设备上启用 Developer Mode 并打开 **Pair New Computer**，才能执行首次 `pair` 命令。Developer Mode 会启动受限传输，因此 Owner SSH Shell 可以保持关闭。密钥生成、配对、撤销和故障行为参见 `developer-mode.zh-CN.md`。provisioning、pairing、mode 或 policy 缺失时，安装必须按 fail-closed 原则拒绝。
