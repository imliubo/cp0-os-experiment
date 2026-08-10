# 应用故障排查

<!-- doc-locale: zh-CN -->
> [English](troubleshooting.md) | **简体中文**

## 医生失败了

- 缺少 `sdk/rust` 或模拟器：`CP0_DEVKIT_ROOT` 指向了错误的级别，或者存档不完整。请验证 `SHA256SUMS`。
- 缺少 Rust 目标：安装固定版本的`wasm32-unknown-unknown`或使用工具链镜像。
- 老的 Rust/Node: 不要围绕宿主工具问题编辑应用；选择固定环境。
- Rust 目标环境看似已安装但 doctor 仍失败：安装
  `wasm32-unknown-unknown` 用于确切的 Rust 版本在
  `devkit/toolchain.toml`不仅用于当前默认工具链。
- 缺少 `emcc`：对 Rust 无关紧要；仅对 C/C++ 和 LVGL 需要。

## 构建失败

- 先运行 `cp0ctl manifest validate app.json`。
- 将 `entrypoint` 匹配到 Cargo 的下划线规范化 `cdylib` 构件。
- 保持SDK版本`1.0`、运行时`wamr`和一个规范的相对`bin`路径。
- 不要添加WASI crate、`std`、文件系统API或OS特定的构建脚本。
- 如果Cargo找不到`cp0-sdk`，请使用发布的`cp0ctl`重新生成，或者在`new`之前设置`CP0_DEVKIT_ROOT`。对于从另一台计算机迁移的项目，只需将`cp0-sdk.path`重新绑定到这个DevKit的规范`sdk/rust`目录；不要保留另一个开发者的绝对路径。

## 模拟器失败

- "没有提供有效的帧": 在阻塞等待输入之前，渲染并提交一个完整的320x150或320x170的RGB565帧。
- 未知键名：使用 `platform-contract.md` 和 `simulator/cp0-simulator.mjs` 中的封闭名称。
- 权限被拒绝：声明确切的能力并测试 `allow` 和 `deny`；不要绕过代理服务。
- 无效帧：显示模式、帧分配和Canvas高度不一致。
- App返回非零值：保留第一个SDK错误并测试该分支而不是将每个错误都转换为成功。
- 未知的 `cp0_media_*` 导入：SDK 和模拟器来自不同的 DevKit 版本。请勿移除媒体调用；恢复一个匹配的 DevKit。
- 媒体动作未被消费：注册一个非闲置会话，并使用 `--media-actions` 而不是原始的 `--keys`，并在配置文件中检查 `media_session_updates` 和 `media_actions_taken`。
- 照片计数意外为零：模拟器照片状态持续一整次运行；
保存并在同一个App执行或测试画廊的空状态中列出。
- 拍照权限被拒绝：`photos.read`、`photos.write` 和 `camera.capture` 是单独的声明。只添加使用的能力并测试允许/拒绝。
- 照片加载损坏或不完整：需要一个精确的320x170 RGB565缓冲区，
保留代理错误，并且永远不要自己重建索引/块键。
- 照片保存返回 `ResourceLimit`：保持之前的库不变，并显示可恢复的存储满状态；不要在紧循环中重试。
- 屏幕睡眠后，第一个键才显示缺失：这是 compositor 的唤醒键契约。需要一个新鲜的故意键来触发 App 动作。

## 安装失败

- 打包之前重建；验证清单标识/版本并在本地验证包签名。
- 开发者签名的包需要匹配的受信任公钥和设备上的启用开发者模式。
- `DeveloperModeOff`：在设备上启用 **Settings > Security > Developer Mode**。Owner SSH Shell 与之无关，可以保持关闭。
- `PairingClosed`: 选择 **新建计算机** 并在十分钟内重试。
- 第一次配对需要输入所有者密码；后续操作使用配对的强制命令 Ed25519 SSH 密钥。不要回退到 `scp`、sudo 或 Bash。
- 未知的 SSH 主机密钥在密码提示前：通过受信任的设备/操作员通道验证其指纹。当前产品 UI 不会暴露这一点；在不信任的网络上停止而不要盲目接受它。
- 签名/密钥不匹配意味着包是由不同的密钥签名的，而不是配对的`developer.pub`；使用配对的密钥重新签名或故意创建一个新的配对。永远不要编辑根信任文件。
- 一台新的工作站需要一个单独的配对条目。设备最多接受八个配对的计算机；当设备满时，在设备上撤销过时的条目。
- Store 包需要单独的审核签名，并且必须更新版本。
- 在稳定性或破坏性接受运行活动时拒绝安装。先获取其证据。
- 使用`cp0ctl logs APP_ID`进行有边界服务中介的日志记录；不要授予应用壳或SSH访问权限。
- SSH不可用：确认首次启动设置已完成且开发者模式已开启。
开发者模式即使在所有者SSH终端关闭时也会启动受限传输。请勿尝试固定用户名、密码或维护后门。
- macOS 阻止 `cp0ctl`：验证软件来源和授权。不要禁用 Gatekeeper 或移除未验证存档的隔离标志；在未授权发布存在之前，使用 OCI 仓库或匹配的源代码版本。

## Store提交失败

- 在每次更改包、清单或PNG后重新运行 `cp0ctl store validate`。
- 身份不符：`app_id` 和 `version` 必须与签名包完全匹配；不要编辑签名的 `.capp`。
- OAuth 挂起或过期：请按照显示的验证 URI 操作，使用已注册应用的所有者具有 2FA 的合格账户，并在稳定错误报告后重新启动。
- 切勿将私钥、OAuth令牌或Store签名放置在`store/`中。
