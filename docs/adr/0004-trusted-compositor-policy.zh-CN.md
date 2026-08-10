# ADR 0004: Weston 可信 compositor 策略

<!-- doc-locale: zh-CN -->
> [English](0004-trusted-compositor-policy.md) | **简体中文**

- 状态：已接受
- 日期: 2026-07-30

## 决策

Phase 2B 保留固定版本的 Weston 14 compositor 和 kiosk shell，并在 shell 之后加载一个小型 CardputerZero policy module。该模块拥有可信系统层、全局系统按键绑定，以及仅由 System Shell 使用的私有版本化 Wayland 协议。

Weston 以 `cp0-compositor` 身份运行，System Shell 以 `cp0-shell` 身份运行。它们只共享访问
compositor socket 所需的 `cp0-wayland` 组。只有当内核提供的 Wayland peer UID 等于专用的
`cp0-shell` UID 时，策略才接受私有协议。`os.cardputerzero.shell` app-id 仅作为额外一致性规则检查，
不作为身份标识。

可信视图在可见时始终位于`WESTON_LAYER_POSITION_TOP_UI`，否则位于`WESTON_LAYER_POSITION_HIDDEN`。Home、Back、Tasks 和 Power 是 compositor 的快捷键。它们的动作通过私有协议发送，因此不依赖于当前聚焦的应用程序。

## 理由

一个普通的xdg-toplevel无法防止另一个客户端覆盖它，并且在另一个应用程序拥有焦点时无法接收键入。fork 所有的weston 或者引入第二个 compositor 会大大增加受信任代码和维护成本。一个狭窄的策略模块使用现有的libweston 层叠和对等凭证，同时保持经过验证的DRM/Pixman 路径。

分离进程账号也可以防止受攻击的 System Shell 使用相同的 UID 进程访问 Weston。第三方应用程序和 App Runtime 进程绝不能接收 `cp0-shell` UID 或 `cp0-wayland` 的成员身份；appd 只会传递显式授权的连接或文件描述符。

## 后果

私有协议是 OS 内部的一部分，而不是公共应用 SDK。它的 XML 有明确版本，并在构建镜像时生成。加载策略失败会导致 compositor 启动失败；System Shell 身份验证失败会阻止该客户端启动。两种情况都不会静默回退到不安全的仅 xdg System Shell。

这个决定确立了可信覆盖层和全局键边界，但本身并不实施应用程序启动、权限决策或24小时稳定性。第二阶段B仍然需要两客户端切换、覆盖层可见性过渡、屏幕截图回归测试和恶意客户端测试。
