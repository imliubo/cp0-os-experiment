# CardputerZero 应用平台合约

<!-- doc-locale: zh-CN -->
> [English](platform-contract.md) | **简体中文**

## 显示和执行

- Runtime: 在WAMR下隔离的WebAssembly；没有Linux/WASI兼容层。
- 标准表面：在可信赖的20像素状态栏下方为320x150 RGB565。
- 沉浸式表面：320x170 RGB565；系统叠加保持受信任且可见。
- 帧字节大小：`width * height * 2`, 小端序 RGB565.
- 最大帧率：30 FPS。优先使用改变重绘对于静态应用。
- 前台：恰好一个应用；只有它接收键盘事件。
- 默认应用内存请求：16-24 MiB。CM0有512 MiB由OS共享。

Rust SDK 渲染器是 `cp0_sdk::ui::Canvas`。即使开发机器提供原生 API，也禁止直接访问
framebuffer、DRM、Wayland 或 evdev。

## 任务和生命周期预览

产品始终保持单前景状态，即使任务模型发生变化：只显示并聚焦一个应用。F3 属于受信任的系统 shell。后台任务从未接收焦点键，并在 CM0 内存压力下可能会被冻结或销毁；独占的摄像头、麦克风和 GPIO 输出租约不是后台应用的权限。

SDK 1.0 声明可选的 `cp0_app_checkpoint` 和 `cp0_app_restore` 导出。
检查点模式版本是 App 所有的非零 `u32` 值，且载荷限制在 8 KiB。没有这些导出的 App 仍然有效并可以干净地重启。
当前的多任务实现是模拟优先：PC App 模拟器不驱动检查点/恢复和生产 Runtime/compositor 的集成仍然取决于设备工作。保持状态恢复独立正确，并且除非目标镜像的发布说明明确启用，否则不要承诺检查点持久性。

## 聚焦输入

`input::poll_key_event` 通过一个受限的 SDK 事件返回 Linux evdev 兼容的数值代码，而不是通过 evdev 文件。处理 `pressed`；仅当交互故意支持键重复时才使用 `repeated`。

| 密钥 | 代码 | 模拟器名称 |
| --- | ---: | --- |
| 退出 | 1 | `esc` |
| 删除键 | 14 | `backspace` |
| 进入 | 28 | `enter` |
| 空间 | 57 | `space` |
| 上 | 103 | `up` |
| 左 | 105 | `left` |
| 右 | 106 | `right` |
| 下载 | 108 | `down` |
| F1-F4 | 59-62 | `f1`-`f4` |

F1-F4 可能在设备上作为全局 System Shell 动作被拦截。不要让它们成为访问 app 功能的唯一途径。字母和数字代码遵循 `simulator/cp0-simulator.mjs` 中的封闭模拟器地图。

当显示器处于休眠状态时，compositor 消耗第一个物理键的按下/重复/释放序列以唤醒面板。该键故意不传递给前台应用。让每一流程都能容忍缺失的唤醒触发键，特别是确认、保存、发送和删除操作。

## 全球媒体动作

媒体播放/暂停、上一首和下一首是可信的 System Shell 操作（V0.6 上为 `Fn+Q`、`Fn+W` 和 `Fn+E`），不是发给焦点应用的按键事件。使用 `media::update_session` 注册播放状态和支持的操作掩码，再使用 `media::take_action` 消费每个路由操作。该调用不包含应用 ID、标题、封面、路径或目标；appd 将其绑定到经过身份验证的前台 Runtime。注册 inactive 会话需要空掩码；暂停或播放需要至少支持一项操作。

模拟器接受 `--media-actions play-pause,previous,next`。此测试装置在不授予 `audio.playback` 的情况下测试注册和动作处理。

## 清单

`app.json` 架构版本 1 要求稳定的反向域名 ID，显示名称，语义版本，SDK `1.0`，WAMR 运行时，规范的 `bin/*.wasm` 入口点，显示模式，内存/存储限制，权限和意图。通过 `cp0ctl manifest validate` 验证；不要手动编写等效验证。

使用应用程序使用的每个能力恰好一个权限条目：

| 能力 | 公共SDK | 限制或边界 |
| --- | --- | --- |
| `notifications.post` | `system` | 32个字符的标题，160个字符的正文 |
| `network.client` | `network` | HTTPS GET，公共目的地，2048字节正文 |
| `documents.open` | `documents` | 门户选择的只读描述符 |
| `audio.playback` | `audio` | 16 kHz单声道S16_LE，每帧1024帧 |
| `audio.capture` | `audio` | 单独捕获授权，相同固定格式 |
| `camera.capture` | `camera` | 一个固定的320x170 RGB565 帧 |
| `hardware.gpio` | `gpio` | 只有四个逻辑V0.6连接线 |
| `radio.lora` | `radio` | 固定外部SX1276策略；默认禁用 |
| `photos.read` | `photos` | 计数，页面和加载共享的320x170帧 |
| `photos.write` | `photos` | 原子地导入或显式删除一帧 |

私有 `storage` 始终是身份绑定并且受限于 `resources.storage_mb`；它没有权限名称。意图必须显式声明并使用反向域名动作。应用不能通过ID选择另一个应用。

`media` 模块提供不指定目标 App 的协调状态，没有对应的权限名称，也绝不替代
`audio.playback`。

共享的照片库没有固定的数量或自动淘汰。使用有界的八项SDK页面和一个108,800字节的帧，并在实现照片访问前阅读`photos.md`。没有应用接收路径或可能 mutation 索引。

Manifest v1 没有打包的 Launcher 图标字段。第三方应用目前在 40x40 网格槽位中使用 System Shell glyph。独立的 Store Listing 使用 48x48 PNG 图标和 320x170 截图；不要把这些字段添加到 `app.json`。

## 隔离规则

不要将主机路径、网络地址、设备节点、shell 文本、UID、套接字路径或秘密添加到应用代码中作为逃生机制。能力代理服务将请求绑定到调用应用的身份和权限决策。拒绝是产品行为的一部分，并且必须有可用的 UI 状态。

## 设备就绪

一个产品设备不暴露固定的用户账号或密码。首次启动设置创建所有者并选择网络连接。设置阻止第三方应用激活，直到其持久提交完成。开发者模式和所有者 SSH Shell 是独立的设置：开发者模式仅启动受限的签名应用传输，而所有者 SSH Shell 为所有者提供一个没有 sudo 权限的 Bash 会话。将可见的 IP 地址和所有者选择的用户名视为设备状态，而不是嵌入源代码、脚本或技能输出中的常量。
