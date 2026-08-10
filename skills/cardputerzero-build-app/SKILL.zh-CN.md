---
name: cardputerzero-build-app
description: Build, modify, debug, simulate, package, sign, pair, deploy, and submit applications for CardputerZero OS with its isolated WebAssembly SDK. Use for CardputerZero app ideas, app.json manifests, 320x170 or 320x150 UI, keyboard wake behavior, global media input, shared photos, Rust/C/C++ SDK code, lifecycle checkpoints, cp0ctl workflows, permissions, cross-computer DevKit setup, Developer Mode pairing and deployment, simulator failures, .capp signing, Store icons/screenshots, OAuth submission, and provisioned-device installation.
---

# 构建CardputerZero应用

<!-- doc-locale: zh-CN -->
> [English](SKILL.md) | **简体中文**

使用受支持的 SDK 和 `cp0ctl`；绝不能以 Raspberry Pi Linux API 为目标，也不能使用
桌面框架、设备节点、套接字、Shell 命令、DRM、evdev 或 WASI。
应用程序是隔离的 WebAssembly 模块，并仅接收声明的能力代理服务。

## 解决开发套件问题

1. 将 `SKILL_DIR` 设置为包含此 `SKILL.md` 的目录。在以下顺序中查找 `ROOT`：`$CP0_DEVKIT_ROOT`，包含此技能的提取 DevKit，或 CardputerZero-OS 仓库根目录。
2. 从 `ROOT/devkit/toolchain.toml` 读取 `rust_version`，在整个构建、运行和打包会话中
   将其导出为 `RUSTUP_TOOLCHAIN`，然后运行
   `"$SKILL_DIR/scripts/doctor.sh" "$ROOT" rust`。诊断脚本会验证固定的工具链，但不会
   修改其父 Shell。若验证失败，请阅读[分发说明](references/distribution.zh-CN.md)，并使用
   固定的工具链镜像，或只安装诊断结果指出的缺失组件。
3. 在发布的DevKit中，请使用`ROOT/bin/cp0ctl`。在源代码检出中，请使用`cargo run --quiet --manifest-path ROOT/Cargo.toml -p cp0ctl --`。
4. 在更改清单、输入模型、显示模式或权限集之前，请阅读 [references/platform-contract.md](references/platform-contract.zh-CN.md)。

不要下载未版本化的SDK文件，也不要从示例中复制私有主机导入，或在未报告的情况下替换编译器/工具链版本。
不要假设通过医生检查会改变Shell：在每次手动组装构建或包的工作流之前，确认`rustc --version`匹配固定版本。
在将App移动到另一台计算机时，请阅读[references/distribution.md](references/distribution.zh-CN.md)中的迁移部分；生成的Rust项目当前将`cp0-sdk`绑定到创建DevKit的绝对路径。

## 选择实现方式

- 默认使用Rust `no_std`。它包含了完整的支持的高级SDK，
项目生成器，构建，模拟器和打包工作流程。
- 可发布的项目工作流仅支持 Rust。C11/C++17 和 LVGL 属于高级 SDK 集成预览：请阅读
  [工作流说明](references/workflows.zh-CN.md)中的 C/C++ 部分，报告缺失的项目或打包
  自动化，不要声称尚未支持的流程已经完成。
- 除非应用程序真正需要受信任的状态栏区域，否则请使用标准320x150显示。谨慎使用沉浸式320x170；受信任的叠加窗口仍然优先。
- 保留一个由调用者拥有的RGB565帧。状态变化时重绘或以每秒不超过30帧的速度重绘。带有限超时时间地轮询输入。
- 为键盘操作设计，高对比度，稳定几何结构和物理320像素屏幕。永远不要依赖悬停、触摸或微小文本。
- 对于媒体应用，请使用无目标应用 ID 的 `media` SDK，并阅读[平台契约](references/platform-contract.zh-CN.md)
  中的媒体部分。绝不要将全局媒体动作视为原始焦点按键事件。
- 对于照片应用，请参阅[照片说明](references/photos.zh-CN.md)。
  只使用`photos` SDK 对象和调用者拥有的RGB565帧；没有应用接收图库路径、索引文件或原始SD卡访问权限。
- 将生命周期检查点导出视为向前兼容的模拟预览，直到目标镜像确认Runtime支持为止。不要仅仅因为SDK声明存在就声称当前硬件会调用它们。

## 创建或修改

创建新的 Rust 应用时，运行：

```sh
cp0ctl new APP_DIR dev.example.app "App Name"
```

使用开发者自有的反向域名 App ID。检查并编辑生成的三个文件：`app.json`、
`Cargo.toml` 和 `src/lib.rs`。保留生成的 `cdylib`、release profile、SDK 路径和导出的
`main` 契约。

对于现有应用，检查那些文件以及它导入的精确SDK模块。遵循附近当前示例的本地模式。DevKit 包含固定八应用产品示例集：Hello Card 作为最小循环，Calculator 用于键映射，Neon Snake 用于状态游戏和私有存储，Camera/Gallery 用于共享照片，Media Controls 用于全局操作，Notes 用于文本和私有存储，以及 Stopwatch 用于单调时间。Camera 和 Gallery 使用受保护的产品标识，因此使用自有应用ID创建新项目，而不是部署那些表单作为第三方替代。

实现最小完整的交互循环：

1. 初始化有界状态和静态帧缓冲区。
2. 立即渲染一个有效的初始帧。
3. 提交通过`display::present_rgb565`。
4. 关注关键事件；仅对按下事件作出反应。
5. 仅在状态或时间驱动的动画发生变化时重新绘制。
6. 将能力拒绝和临时不可用视为正常状态。

## 声明能力

从没有权限开始。只有在代码使用其匹配的公共SDK模块后，才添加一个manifest权限。给出一个简短的面向用户的理由。私有存储是配额控制的，不需要权限。永远不要请求更广泛的权限来绕过实现失败。

媒体会话注册不需要权限且不授予音频访问权限。
当应用实际提交PCM音频时，单独声明`audio.playback`。
共享照片访问与私人存储分开：声明`photos.read`用于计数/列表/加载，声明`photos.write`用于导入/删除。摄像头捕获也需要`camera.capture`。

阅读 [references/platform-contract.md](references/platform-contract.zh-CN.md) 以了解封闭权限词汇表、限制和输入代码。确认导入和清单声明在打包前一致。

## 在打包之前验证

使用代表性的逗号分隔密钥序列运行捆绑验证器：

```sh
"$SKILL_DIR/scripts/verify-app.sh" APP_DIR left,right,enter deny 1000
```

对于媒体应用，将全局动作作为第五个可选参数传递：

```sh
"$SKILL_DIR/scripts/verify-app.sh" APP_DIR "" deny 1000 \
  play-pause,previous,next
```

然后检查渲染得到的 PPM 和 JSON profile。至少测试初始状态、主要成功路径、边界输入、
重启/返回行为，以及应用使用的每种权限允许/拒绝状态。对于游戏或有状态工具，增加
确定性逻辑测试。

设备在唤醒睡眠中的显示时消耗第一个物理键。应用程序逻辑必须在该键从未送达的情况下保持正确；不要要求单次唤醒触发按压来执行破坏性或一次性操作。

不接受单独的构建。完成需要有效的清单文件、成功的WASM构建、至少一个模拟帧、边界内存、预期的输入数量以及没有未声明的能力调用。

## 打包并安装

签名或安装到设备前，请阅读
[工作流说明](references/workflows.zh-CN.md)。使用 `cp0ctl package`
可复现地打包，使用保存在项目之外的开发者密钥签名，并在安装前验证已签名的 `.capp`。

对于产品设备，在配对或远程部署之前，请阅读[references/developer-mode.md](references/developer-mode.zh-CN.md)。设备所有者必须物理上启用开发者模式并打开十分钟的配对窗口。将工作站的开发者签名公钥和Ed25519 SSH公钥与`cp0ctl pair`配对；不要编辑信任文件或使用`scp`、sudo、远程shell或未签名的包。

当请求Store分发时，读取
[Store 提交说明](references/store-submission.zh-CN.md)。准备并
本地验证 Listing 之后再开始 OAuth 设备流程。不要运行内部 `store publish` 操作工作流或自己添加 Store 签名。

安装更改会影响真实设备。确认请求的设备，并确保没有正在进行的稳定性、恢复或工厂验收运行。量产设备必须已完成首次启动配置。Developer Mode 本身启动受限 SSH 传输；独立的 Owner SSH Shell 可以保持关闭。使用所有者选择的账户和当前设备 IP；绝不能假设账户为 `pi`、密码固定或地址不变。绝不能削弱设备策略、把私钥复制到设备或绕过签名验证。

## 诊断故障

当环境诊断、构建、模拟器或设备安装失败时，请参阅[故障排查说明](references/troubleshooting.zh-CN.md)。
保留首个具体错误，判断它属于宿主工具链、Manifest、WASM ABI、应用逻辑、权限策略还是
设备状态，然后在继续之前从最小相关范围重新测试。

## 完成检查清单

- `doctor.sh` 适用于所选语言。
- Manifest标识、入口点、SDK版本、显示、资源和权限需与实现匹配。
- 共享照片应用覆盖空闲、允许、拒绝、缺失/删除和满资源状态，而不假设主机路径或固定库大小。
- 逻辑测试和`verify-app.sh`使用代表输入通过。
- 最终帧在其真实的320像素尺寸下进行视觉检查。
- 当请求分发时，包签名验证通过。
- 请求设备部署时，开发者模式配对使用准确的包签名公钥和单独的 Ed25519 SSH 公钥。
- 请求 Store 分发时，Store Listing 必须在提交前通过验证。
- 设备安装和物理键仅在授权和安全时进行测试。
- 向用户报告源代码、命令、制品路径以及所有尚未完成的硬件检查。
