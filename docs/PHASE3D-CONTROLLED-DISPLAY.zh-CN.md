# Phase 3D：受控的应用显示

<!-- doc-locale: zh-CN -->
> [English](PHASE3D-CONTROLLED-DISPLAY.md) | **简体中文**

## 安全边界

应用程序账户不是`cp0-wayland`的持久成员。每个临时服务在其受信任的bubblewrap命令启动时才接收该辅助组。bubblewrap只读绑定由 compositor 所拥有的套接字到沙盒内的一个空的`/run/cardputerzero`目录中。主机 compositor 运行时目录中的其他文件均不可见。

Runtime 要求固定的 `XDG_RUNTIME_DIR=/run/cardputerzero` 和
`WAYLAND_DISPLAY=wayland-0` 启动契约，并拒绝继承 `WAYLAND_SOCKET`。只有在 systemd
应用专属 App UID 后 Runtime 才会连接，因此 compositor 看到的 `SO_PEERCRED` 身份与
appd 任务元数据一致。App 身份和显示模式从可信安装 Manifest 复制到清空后的 bubblewrap
环境。WASM 模块不能建立第二条 Wayland 连接、访问宿主路径或自行提供 xdg app-id。

## 渲染路径

Wayland 1.23.1 和 libffi 3.5.2 是静态链接到 Runtime 中的。xdg-shell 绑定是从 wayland-protocols 1.44 生成的。所有仓库和精确提交都被固定在 `app-runtime/wayland.env` 中。

公共 host ABI 接受一帧完整的 little-endian RGB565 内容和最多 32 个 damage rectangle。Runtime 在更新可信 XRGB8888 shadow frame 前验证完整 WASM 内存范围、帧长度和每个矩形。两个 `wl_shm` buffer 可防止 compositor ownership 与下一次应用更新竞争。

标准应用接收 320x150 尺寸。它们的内容从物理 y=20 开始，因此 WASM 无法访问保留的状态区域。沉浸式应用接收完整的 320x170 帧。 compositor 负责最终的表面放置，并隐藏不活跃的应用程序。

Runtime 的事件等待会驱动 Wayland 连接，包括 buffer release、configure 和 close 事件。
双缓冲繁忙时返回稳定的 SDK `ResourceLimit`，应用可重试而不会阻塞 compositor 进程。
宿主侧单调节流器也会拒绝距上次提交不足 33,333,334 ns 的提交，因此即使 WebAssembly
应用忙循环而不等待输入，架构规定的 30 FPS 上限仍会生效。

## 构建和测试

`make app-runtime` 构建宿主机 `wayland-scanner`, 交叉编译静态 AArch64 libffi，生成核心和 xdg-shell 协议并链接最终的静态 Runtime。`tests/test-runtime-display.sh` 独立于硬件验证 RGB565 转换、标准模式偏移和恶意损害边界。

硬件验收还需要通过 appd/systemd/bubblewrap 路径渲染仅包含 SDK 的 Hello 应用程序， compositor 发现和单个前台激活。应用沙箱必须只暴露绑定的 Wayland 端点，而不是其宿主运行时目录。

## V0.6 验证

最初的V0.6接受使用了systemd `OpenFile=` 并继承了FD 3。这证明了渲染隔离，但systemd PID 1 创建了Wayland连接，所以 compositor 对端凭据是root而不是App UID。任务恢复和可信缩略图关联正确地拒绝了该身份。上面的直接Runtime连接替代了那个无效的启动合同。

Weston 在 SDK 仅有的 Hello WASM 提交其第一个缓冲区后宣布了 `app token=1`。一个受信任的 30 秒激活客户端暴露了表面，4K 摄像头检查显示了预期的白色边框和红色、绿色和蓝色 RGB565 带。重启生产 Shell 恢复了 Home，而隐藏的应用程序仍然活着。应用程序单元在峰值时使用了 9.8 MiB 的内存，没有交换，并且有三个任务；compositor、Shell 和 appd 保持活跃。然后通过 appd 停止了应用程序，使得所有三个核心服务都保持活跃。
