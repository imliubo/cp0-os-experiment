# CardputerZero 应用程序开发指南

<!-- doc-locale: zh-CN -->
> [English](DEVELOPER-GUIDE.md) | **简体中文**

## 应用模型

CardputerZero 应用程序是基于平台 SDK 构建的 WebAssembly 模块。它们不接收 Linux 路径、套接字、设备节点或 shell 访问权限。每个安装的应用程序都有一个稳定的 UID、一个私有的存储配额和一个明确的权限集。每次只有一个应用程序处于前台，只有该应用程序接收键盘输入或拥有应用程序表面。

支持的 Rust 目标是 `wasm32-unknown-unknown`。C 和 C++ 应用程序使用 Clang 兼容的 freestanding wasm32 工具链和 `sdk/c/include/cardputerzero.h`。传统的 Raspberry Pi 桌面或 Linux 应用程序故意不兼容。

## 开发套件和AI技能

推荐的分发是带版本号的宿主机原生DevKit存档或在`docs/APP-DEVKIT-DISTRIBUTION.md`中描述的完整工具链镜像。提取前请验证发布校验和，然后设置根并运行其医生工具：

```sh
export CP0_DEVKIT_ROOT=/path/to/cardputerzero-app-devkit-1.1.0-HOST
export PATH="$CP0_DEVKIT_ROOT/bin:$PATH"
export RUSTUP_TOOLCHAIN=$(awk -F '"' '$1 ~ /^rust_version = / { print $2 }' \
  "$CP0_DEVKIT_ROOT/devkit/toolchain.toml")
"$CP0_DEVKIT_ROOT/skills/cardputerzero-build-app/scripts/doctor.sh" \
  "$CP0_DEVKIT_ROOT" rust
```

保持那个变量在每个 `cp0ctl build`、`run` 和 `package` 命令中导出。医生验证了锁定的编译器，但不能修改其父 Shell；`cp0ctl` 当前继承了活动的 Cargo 工具链。

捆绑的 `$cardputerzero-build-app` 技能为AI代理提供了平台合同、项目工作流、权限边界、确定性验证器和故障路由，使其能够在没有私有Runtime或Linux API的情况下完成应用程序。保留来自同一DevKit的技能、SDK、模拟器和`cp0ctl`。

在另一台计算机上，使用与该计算机的操作系统和CPU匹配的原生存档，或者使用发布的OCI工具链镜像。在使用前，验证相邻存档的校验码和提取的`SHA256SUMS`。原生`cp0ctl`二进制文件在macOS/Linux或CPU架构之间不具有可移植性。Windows开发使用OCI镜像或受支持的Linux环境，而不是原生Unix DevKit。

公共的 macOS 原生发布也需要 Developer ID 签名和 不ar化。不要禁用 Gatekeeper 或移除 隔离以运行 未验证的 内部存档；而是使用 验证过的 OCI 仓库镜像或匹配的源代码检出。

对于原生Rust开发，安装Node 20或更高版本以及固定工具链：

```sh
rustup toolchain install 1.85.1 --profile minimal
rustup target add --toolchain 1.85.1 wasm32-unknown-unknown
```

生成的 Rust 项目当前会在 `Cargo.toml` 中记录创建该项目的
DevKit 的规范绝对 `sdk/rust` 路径。将应用克隆到另一台计算机后，只需把
`[dependencies].cp0-sdk.path` 替换为该计算机上规范的
`$CP0_DEVKIT_ROOT/sdk/rust`，然后重新执行 manifest 验证、构建和模拟。不要把其他
开发者的绝对 SDK 路径作为可移植依赖提交。完整的迁移契约见
`APP-DEVKIT-DISTRIBUTION.md`。

## 创建并构建

使用发布的DevKit，可以直接使用`cp0ctl`创建和构建：

```sh
cp0ctl new /tmp/my-clock dev.example.clock "Clock"
cp0ctl build /tmp/my-clock
```

在OS源代码检出中，安装锁定的Rust WAMR目标，并使用workspace工具：

```sh
rustup target add --toolchain 1.85.1 wasm32-unknown-unknown
cargo run -p cp0ctl -- new /tmp/my-clock dev.example.clock "Clock"
cargo run -p cp0ctl -- build /tmp/my-clock
```

生成的`app.json`声明应用程序身份、SDK 要求、显示模式、内存和私有存储限制、权限和意图。`cp0ctl build`验证此清单并在`target/cardputerzero/<app-id>/<version>`下构建一个确定性的包树。

在受信任的状态栏下方使用标准显示模式为一个 320x150 的应用程序表面。沉浸模式使用 320x170，但 System Shell 的权限提示、通知和全局操作仍为受信任的 compositor 重叠层。

## 事件循环和UI

保留一个由调用者拥有的RGB565帧，并在状态变化后重新绘制。使用带有限超时的轮询来处理焦点键盘事件，以便生命周期和代理服务的工作可以继续。`cp0_sdk::ui::Canvas` 是最小的无分配渲染器。`sdk/lvgl` 中的可选LVGL 9适配器通过相同的公共SDK ABI 提供更大的控件工具包。

两种 UI 路径都不会授予 framebuffer、DRM、Wayland 或 evdev 的直接访问权限。每次
frame 提交和每个输入事件都必须经过 App Runtime。

对于文本字段，消耗 Rust `KeyEvent::character` 或 C/C++ `cp0_key_event_t.character`；零/`None` 表示键没有可打印文本。Runtime 使用与首次启动和 System Shell 相同的 V0.6 可打印布局，包括按住 Shift 的大写和每个打印的 `Sym` 符号。不要在 App 中翻译 Linux 键码。仅将 `code` 用于导航、快捷键和游戏控制，并仅在按下/重复时处理 `character`。

当物理按键唤醒休眠的显示器时， compositor 消费该按键的完整按压/重复/释放序列。前台应用接收下一个有意的按键。唤醒触发键缺失时，确认、保存、发送和删除流程必须保持正确。

## 权限

仅声明应用程序使用的功能，并在受信任的权限提示中提供简短的原因。示例包括 `network.client`、`audio.playback`、`audio.capture`、`camera.capture`、`hardware.gpio`、`radio.lora`、`documents.open`、`notifications.post`、`photos.read`、`photos.write` 和意图声明。共享照片库在 `docs/PHOTO-LIBRARY-V2.md` 中有文档说明：它没有固定的照片计数淘汰策略，使用代理分页，并不暴露文件系统路径或可变索引。私有键值存储在清单的 `resources.storage_mb` 配额内自动可用，并没有单独的功能名称。被拒绝的功能返回 `Error::Denied`；待定决定或暂时不可用的服务返回 `Error::Unavailable`，并在返回事件循环后可以重试。

PC模拟器从不授予宿主机访问权限。`--permissions allow`使用确定性能力固定装置，而`--permissions deny`验证应用程序的拒绝路径。

Photo Apps 使用 `photos::count`, `photos::list_page`, `photos::load_rgb565`, `photos::save_rgb565` 和 `photos::delete`。Camera Apps 可以使用 `camera::capture_rgb565` 获取固定 320x170 预览，并使用 `camera::capture_photo` 获取系统管理的 1280x720 JPEG 捕获，仅返回照片 ID。保留八张 ID 和一张缩略图，而不是按库的比例分配，并处理 `Denied`, `Unavailable` 和 `ResourceLimit` 作为正常 UI 状态。

## 模拟和分析

```sh
cargo run -p cp0ctl -- run examples/calculator \
  --keys 1,2,plus,3,equal --output /tmp/calculator.ppm \
  --profile /tmp/calculator.json

cargo run -p cp0ctl -- run examples/camera \
  --permissions allow --keys enter --output /tmp/camera.ppm
```

模拟器记录提交的帧、能力调用、输入计数、私有存储字节/键、线性内存和时间戳在JSON配置文件中。其私有存储配置项强制执行清单字节配额、256个键限制和缺失键语义。它是一个确定性的SDK测试框架，不是设备命名空间、seccomp和cgroup测试的安全替代品。

DevKit 包含九个示例。八个生产内置示例是 Hello Card、Calculator、Neon Snake、Camera、Gallery、Music、Notes 和 Stopwatch；Media Controls 仍然是补充的 SDK 示例。它们的 README 和 320 像素屏幕截图记录了预期的交互方式。Camera 和 Gallery 展示了照片代理，但使用了受保护的产品标识符，因此第三方开发需要从 `cp0ctl new` 和拥有 App ID 开始。Music 展示了 SDK 1.1 本地文档和 HTTPS 范围音频流媒体。Media Controls 隔离了目标无关的全局操作 API，但不声明音频播放。

Manifest v1 不包含启动器图标。当前应用网格为第三方应用在 40x40 格口中渲染 System Shell 徽标。Store 提交需要一个 48x48 PNG 图标和一个到五个 320x170 截图；这些资产属于 `store/listing.json`，不属于 `app.json`。

## 打包、签名和安装

生成一个开发密钥，然后创建并签署一个可重复的`.capp`：

```sh
cargo run -p cp0ctl -- key generate developer.key developer.pub
cargo run -p cp0ctl -- package /tmp/my-clock /tmp/my-clock.capp
cargo run -p cp0ctl -- sign developer /tmp/my-clock.capp \
  /tmp/my-clock.developer.capp developer.key
cargo run -p cp0ctl -- verify /tmp/my-clock.developer.capp
cargo run -p cp0ctl -- install /tmp/my-clock.developer.capp \
  --device OWNER@DEVICE_IP
```

设备安装只有在开发人员密钥配对且设备已明确启用开发者模式时才成功。在个人生产设备上，打开**设置 > 安全 > 开发者模式**，选择**配对新计算机**，然后在十分钟窗口内注册工作站的开发人员和SSH公钥：

```sh
cp0ctl pair developer.pub ~/.ssh/cardputerzero_ed25519.pub workstation \
  --device OWNER@DEVICE_IP
```

开发者模式仅暴露有界 `cp0ctl` 部署通道。它不启用交互式 SSH 壳、root、sudo、本地包或未签名的应用。父级或组织策略可能会锁定该设置。Store 分发添加独立的 Store 审核签名。请参见 `DEVELOPER-ACCESS.md` 了解完整的信任和撤销边界。

量产镜像不包含固定的 `pi` 账户、密码或地址。首次启动 Setup 必须完成。Developer Mode 启动受限 SSH 传输；独立的 **Owner SSH Shell** 设置可以保持关闭。使用所有者选择的用户名和可信 Setup/网络 UI 显示的 IP；切勿在应用或分发脚本中嵌入测试设备凭据。

关闭开发模式会阻止新的配对和每个远程应用的任何变化。
每个额外的工作站都需要有自己的 Ed25519 SSH 密钥和设备上的配对条目；当开发者身份必须保持不变时，它可以重用安全传输的开发者签名密钥。设备最多存储八台配对的计算机，所有者可以从“配对的计算机”中撤销一个或全部。

Store提交使用开发者签名的包，而不是未签名的构建或已有Store签名的包。审核元数据必须绑定提交的精确SHA-256、声明的权限和检查的WASM导入。开发者首先在本地验证包和Store资源：

```sh
cargo run -p cp0ctl -- store validate \
  dev.cardputerzero.example-1.0.0.signed.capp store/listing.json
```

这拒绝身份不符、无效开发者签名、预存的Store签名、不安全或符号链接资产路径、大小/校验和不符以及畸形PNG尺寸。在开发者门户注册App ID后，使用以下内容提交相同的不可变输入：

```sh
cargo run -p cp0ctl -- store submit \
  dev.cardputerzero.example-1.0.0.signed.capp store/listing.json
```

CLI 使用 OAuth 设备授权流程，分段可续传数据和内存中的令牌。其最终 stdout 值是包含提交 ID 和门户 URL 的机器可读 JSON。开发人员到此为止。一个独立的手动审核/发布操作员可以运行：

```sh
cargo run -p cp0ctl -- store publish \
  submissions reviews public-store https://store.example.invalid \
  42 1800000000 1800600000 store.key
```

这创建了一个新的静态HTTPS树，包含商店签名的包、一个签名的目录和`store.pub`。输出目录必须不存在。开发人员不能通过添加商店签名来自行批准一个包；设备信任密钥和审查签名密钥独立控制。请参阅`docs/PHASE5B-APPLICATION-STORE.md`中的审查方案和设备信任边界。

在预配置的测试设备上，操作员可以使用固定的 Store 控制命令：
`sudo cp0ctl store list`、`sudo cp0ctl store search <query> [offset limit]`、
`sudo cp0ctl store refresh` 和
`sudo cp0ctl store install <app-id> --approve-permissions`。搜索在本地进行，每页最多返回
八个结果，不会把查询发送到源站。Catalog URL、包 URL、预期身份、大小和哈希均来自
root 拥有的配置及已验证 Catalog；这些命令不允许调用者提供上述信息。

应用程序日志是受限的，并由根调解：

```sh
cargo run -p cp0ctl -- logs dev.example.clock --device OWNER@DEVICE_IP
```

媒体应用仅注册播放状态和支持的全局操作 `media::update_session` 用 Rust 或 `cp0_media_session_update` 用C/C++。
它们消耗Play/ Pause，Previous和Next。 `media::take_action` 或者
`cp0_media_take_action`API 没有目标 ID 或应用程序提供的元数据；appd 将其绑定到经过身份验证的前台 Runtime。注册不授予音频访问权限，因此实际播放仍然需要
`audio.playback`. 查看 [媒体会话代理服务](MEDIA-SESSION-BROKER.zh-CN.md).

## 兼容性

当前的清单SDK要求是`1.1`，由WIT包`cardputerzero:sdk@1.1.0`支持。设备拒绝未知的主要版本，并接受同一主要版本内的应用次要版本不新于自身的版本。它还接受Exactly legacy SDK`0.1`；任意`0.x`版本都不兼容。公共WIT描述了类型化的源代码契约；`sdk/abi/cardputerzero-hostcalls-v1.json`是标准的扁平WAMR导入契约，生成的SDK绑定必须完全匹配它。
