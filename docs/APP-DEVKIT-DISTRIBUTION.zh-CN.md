# CardputerZero App DevKit 发行版

<!-- doc-locale: zh-CN -->
> [English](APP-DEVKIT-DISTRIBUTION.md) | **简体中文**

## 发布合同

每次 App SDK 发布都包含一个版本号，覆盖了 manifest 合同、Rust/C/C++ SDKs、模拟器、`cp0ctl`、技能和 DevKit。`devkit/toolchain.toml` 固定用于验收的编译环境。发布时不能混用不同提交或 SDK 版本的文件。

公共分发被阻止，直到项目所有者为 `cp0ctl` 和 DevKit 包含的仓库内容选择并记录了许可证，然后发布了所需的许可证和第三方通知。Rust `cp0-sdk` 箱声明了 Apache-2.0，但仅此声明不足以许可其他捆绑文件或可执行文件。在做出此决定之前可以构建内部接受存档；不要将它们作为公共发布资产发布。

Rust 是 DevKit 1.1 中唯一可以发布的端到端 App 工作流。C/C++ 头文件、ABI 合同和 LVGL 调用器已提供以支持高级集成，但 `cp0ctl` 仍无法生成、最终链接、模拟或打包这些项目。上游 LVGL 源代码未捆绑。发布说明必须保留这一区别。

运行 `make devkit` 以生成主机原生的存档文件，路径为 `target/app-devkit`。
存档包含：

- 一个可移动的 `bin/cp0ctl` 用于其命名的目标宿主；
- Rust、C/C++、WIT、ABI和LVGL SDK 源代码；
- 具有集中键和全局媒体固定装置的确定性PC模拟器；
- `cardputerzero-build-app` Skill，以及 App 和 Store Listing Schema；
- Hello Card、Calculator、Neon Snake、Camera、Gallery、Music、Media Controls、Notes 和
  Stopwatch 源代码及参考截图；
- 开发者模式、照片库、图标和开发者工作流文档；
- 一种机器可读的 `devkit.json`，按文件 `SHA256SUMS` 和存档校验和。

为每个支持的宿主机发布一个单独的本地存档。本地存档不会默默地下载编译器。开发人员必须使用`devkit/toolchain.toml`中的版本，或者使用完整的工具链镜像。

## 在另一台计算机上使用

获取目标计算机的精确操作系统和CPU对应的存档及其相邻校验码。然后验证该存档及其解压后的内容：

```sh
shasum -a 256 -c cardputerzero-app-devkit-1.1.0-HOST.tar.xz.sha256
tar -xJf cardputerzero-app-devkit-1.1.0-HOST.tar.xz
export CP0_DEVKIT_ROOT="$PWD/cardputerzero-app-devkit-1.1.0-HOST"
export PATH="$CP0_DEVKIT_ROOT/bin:$PATH"
export RUSTUP_TOOLCHAIN=$(awk -F '"' '$1 ~ /^rust_version = / { print $2 }' \
  "$CP0_DEVKIT_ROOT/devkit/toolchain.toml")
(cd "$CP0_DEVKIT_ROOT" && shasum -a 256 -c SHA256SUMS)
"$CP0_DEVKIT_ROOT/skills/cardputerzero-build-app/scripts/doctor.sh" \
  "$CP0_DEVKIT_ROOT" rust
```

安装`wasm32-unknown-unknown`工具链和`devkit/toolchain.toml`中指定的Rust目标；Rust应用开发还需要Node 20或更新版本。本地存档包含`cp0ctl`、SDK源代码、模拟器、Skill、模式、示例和开发人员文档，但故意不包含编译器。在安装匹配的主机工具不可取或工作站上没有本地存档时，请使用完整的OCI镜像。

在整个会话中保持 `RUSTUP_TOOLCHAIN` 导出。`doctor.sh` 检查固定编译器是否存在，但无法为后续 `cp0ctl` 子进程选择它。

公共的 macOS 原生存档需要项目批准的 Developer ID 签名和 notarization 以及校验和。不要指示用户禁用 Gatekeeper 或从未验证的内部构建中移除隔离状态。

```sh
rustup toolchain install 1.85.1 --profile minimal
rustup target add --toolchain 1.85.1 wasm32-unknown-unknown
```

一个应用仓库可以在没有`target/`、私有签名密钥或OAuth令牌的情况下被复制。`cp0ctl new`当前将源DevKit的规范`sdk/rust`路径写入生成的`Cargo.toml`中；在构建前将该单一依赖路径重新绑定到目标DevKit。一个新配对的工作站使用自己的Ed25519 SSH密钥。仅在应用签名身份必须保持不变时，通过开发者的安全密钥管理过程转移开发者的签名私钥。

## 完整工具链镜像

`devkit/Dockerfile` 从锁定的 Emscripten SDK 多平台摘要和 Rust 工具链构建标准环境。它包括 Rust
`wasm32-unknown-unknown`, 节点, Emscripten, `cp0ctl`, 每个 SDK 和模拟器。构建并导出它：

```sh
docker build -f devkit/Dockerfile -t cardputerzero/app-devkit:1.1.0 .
docker save cardputerzero/app-devkit:1.1.0 | xz -T0 > cardputerzero-app-devkit-1.1.0.oci.tar.xz
shasum -a 256 cardputerzero-app-devkit-1.1.0.oci.tar.xz > cardputerzero-app-devkit-1.1.0.oci.tar.xz.sha256
```

发布OCI存档和校验和供离线使用。开发人员使用`docker load`加载它，并通过`devkit/cp0-dev`启动它。不要指示AI代理获取未版本化的SDK、编译器安装程序或容器标签。

`devkit/Dockerfile` 需要完整的源工作区。本地 DevKit 中复制的副本仅包含发布元数据，无法仅从提取的存档构建完整的镜像；本地消费者必须获取一个发布的 OCI 存档。

## 发布接受

在发布前，在每个宿主机 artifact 上验证以下内容：

1. 归档的校验和和内部 `SHA256SUMS` 通过。
2. `bin/cp0ctl new` 在提取的 DevKit 内创建一个项目的 SDK 路径。
3. 生成的项目可以在没有源仓库的情况下构建。
4. 所有九个打包示例都能编译；摄像头/照片和媒体模拟器配件生成帧和配置文件，并消耗其脚本动作。
5. 该技能通过了结构验证器的验证，其`doctor.sh`报告了预期的工具链。
6. `make check` 在发布提交时通过。
7. 该版本包含所有者批准的许可证和第三方通知。
8. macOS 归档在公开发布前由开发者 ID 签名并经过核实。

签署和发布发布制品属于发布管道。技能可以验证发布的校验和，但必须不绕过缺失的校验和或用另一个版本的文件替代。
