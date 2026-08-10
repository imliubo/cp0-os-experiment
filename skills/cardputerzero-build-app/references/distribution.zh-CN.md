# 开发工具包和工具链分发

<!-- doc-locale: zh-CN -->
> [English](distribution.md) | **简体中文**

## 首选顺序

1. 使用发布的完整工具链 OCI 镜像以获得最可重复的设置。
2. 使用宿主机原生的 DevKit 归档以及 `devkit/toolchain.toml` 中的精确工具，当容器不可用时。
3. 仅用于OS/SDK开发使用源代码检出。

每次发布的存档都必须有一个相邻的SHA-256文件。在解压前验证它，然后验证存档内部的`SHA256SUMS`。不要在DevKit版本之间混用Skill、SDK、模拟器或`cp0ctl`。

除非发布的版本包含项目所有者批准的许可证和所需的第三方通知，否则不得公开分发内部构建的档案。一个SDK构件上的许可证声明并不涵盖所有捆绑的工具和文件。

## 本地DevKit

本地存档命名为 `cardputerzero-app-devkit-VERSION-HOST.tar.xz`。它包含：

- `bin/cp0ctl` 为命名主机；
- `sdk/{rust,c,lvgl,wit,abi}`；
- `simulator/cp0-simulator.mjs`；
- 此技能在 `skills/` 下；
- App 和 Store Listing 架构；
- 完整的八App产品示例集，附带参考截图；
- 开发者模式，照片库和图标文档，版本元数据和校验和。

示例包括 Hello Card、Calculator、Neon Snake、Camera、Gallery、Media Controls、Notes 和
Stopwatch。Camera 和 Gallery 使用受保护的内置 App ID；它们是参考源代码，不是第三方包。

将`CP0_DEVKIT_ROOT`设置为提取到的根，并将其`bin`目录添加到`PATH`中。在`devkit/toolchain.toml`中选择Rust版本，在创建项目之前运行此Skill的`scripts/doctor.sh`：

```sh
shasum -a 256 -c cardputerzero-app-devkit-VERSION-HOST.tar.xz.sha256
tar -xJf cardputerzero-app-devkit-VERSION-HOST.tar.xz
export CP0_DEVKIT_ROOT="$PWD/cardputerzero-app-devkit-VERSION-HOST"
export PATH="$CP0_DEVKIT_ROOT/bin:$PATH"
export RUSTUP_TOOLCHAIN=$(awk -F '"' '$1 ~ /^rust_version = / { print $2 }' \
  "$CP0_DEVKIT_ROOT/devkit/toolchain.toml")
(cd "$CP0_DEVKIT_ROOT" && shasum -a 256 -c SHA256SUMS)
"$CP0_DEVKIT_ROOT/skills/cardputerzero-build-app/scripts/doctor.sh" \
  "$CP0_DEVKIT_ROOT" rust
```

本地存档是主机特定的，因为`bin/cp0ctl`是本地的。使用匹配的macOS/Linux CPU存档。在Windows上或CPU不匹配时，使用OCI镜像而不是尝试运行另一个主机的二进制文件。

保持 `RUSTUP_TOOLCHAIN` 用于完整的 App 会话。仅通过医生传递不会为后续的 `cp0ctl` 子进程选择工具链。

公开发布的 macOS archive 还需要项目批准的 Developer ID 签名和 notarization。如果 Gatekeeper 拒绝内部 ad-hoc build，请使用经过验证的 OCI 镜像或源码 checkout；绝不能全局禁用 Gatekeeper，也不能从未验证的 archive 中移除 quarantine 标志。

本地存档故意不会获取或静默安装编译器。
安装 Node 20 或更新版本以及固定在 `devkit/toolchain.toml` 中的 Rust 工具链，或者切换到完整镜像：

```sh
rustup toolchain install 1.85.1 --profile minimal
rustup target add --toolchain 1.85.1 wasm32-unknown-unknown
```

仅安装固定版本的 Emscripten 以确保 C/C++ 兼容性工作；对于准备发布的 Rust 工作流来说，它不是必需的。

Rust 是可发布的应用语言。包含了与兼容性工作相关的 C/C++ 头文件、ABI 文件和 LVGL 调用适配器，但当前的 DevKit 未能提供它们的最终链接、模拟器或打包工作流程。

## 完整工具链镜像

标准镜像包括原生DevKit、Rust 1.85.1,`wasm32-unknown-unknown`, Node 20 或更新版本以及 Emscripten 5.0.4。仅在校验完校验和后加载已发布的离线镜像，或从匹配的签名源代码版本构建`devkit/Dockerfile`。

Dockerfile 复制到本地存档中是发布元数据，并需要完整的源工作空间来构建。它不能仅从提取的本地 DevKit 中启动一个 OCI 存档。本地存档消费者应该获取一个发布的 OCI 存档，而不是在提取目录中运行该 Dockerfile。

使用以下命令启动项目目录：

```sh
CP0_DEVKIT_IMAGE=cardputerzero/app-devkit:1.0.0 \
  /path/to/devkit/cp0-dev /path/to/project
```

## 将应用移动到另一台计算机

在目标设备上安装一个完整的、匹配的DevKit；不要只复制`sdk/rust`或重新使用目标设备作为编译器。克隆或复制App源代码，但不要包含`target/`、签名密钥或OAuth令牌。

`cp0ctl new` 目前将创建 DevKit 的规范 `sdk/rust` 路径
写入 `Cargo.toml`。在移动现有项目后，仅用目标的规范 `[dependencies].cp0-sdk.path` 路径替换 `$CP0_DEVKIT_ROOT/sdk/rust` 值，然后重新运行 manifest 验证、构建和捆绑的验证器。此路径绑定是当前 DevKit 的一个限制；不要将一个开发者的绝对路径提交为可移植路径。

开发人员签名密钥只能通过开发人员的安全密钥管理过程进行移动。新计算机也需要自己的 Ed25519 SSH 密钥和一个新的设备配对条目。重复使用相同的 App 签名密钥可以保存开发人员身份；配对新的 SSH 密钥不需要复制旧的 SSH 私钥。

## 失败行为

如果发布资产、校验和或锁定的工具不可用，请报告确切缺失的文件。不要替换 `latest`、使用 curl 下载任意 SDK 文件、禁用校验和验证或从目标设备复制编译器。
