# Phase 3E: 专注的应用输入

<!-- doc-locale: zh-CN -->
> [English](PHASE3E-FOCUSED-INPUT.md) | **简体中文**

## 输入边界

应用程序从不接收 evdev 或 libinput 描述符。可信运行时在其预连接的 Wayland 通道上绑定 `wl_seat` 和 `wl_keyboard`。Weston 只将键盘焦点给 compositor 策略激活的单个表面，因此隐藏的应用程序不会接收到键盘事件。

Runtime 只在为其自己的表面接收到一个 `wl_keyboard.enter` 后才开始接受事件。`wl_keyboard.leave` 清空整个事件队列、保持的修饰状态和焦点标志，然后控制权返回给 WASM。Home、Back、Tasks 和 Power 仍然是 compositor 所有的绑定，不依赖于应用程序的轮询。

字符转换属于平台，而不是各个应用。System Shell 和 App Runtime 编译同一份 V0.6 evdev-to-printable-ASCII 表。32 个 `Sym` 组合使用 V0.6 键盘布局的独立 evdev 标识符，因此可以直接映射到印刷字符，无需合成 Shift 状态。Runtime 只对普通字母和数字使用 XKB Shift 按下状态以及原始左右 Shift，并有意忽略锁定状态。这与首次启动的 Owner name 和 Wi-Fi password 输入一致。

## SDK ABI

Rust 暴露了 `input::poll_key_event(timeout)`，C/C++ 暴露了 `cp0_poll_key_event`. 一个关键事件是一个固定的八字节小端记录：

- Linux 输入键码（`u16`）;
- 按下和重复标志；
- 稳定的 Shift, Control, Alt 和 Super 位；
- 一个系统生成的可打印ASCII字节，或者当事件没有文本时为零；
- 两个预留的零字节。

Rust 将文本字节暴露为`KeyEvent::character: Option<u8>`；C/C++ 暴露`cp0_key_event_t.character`，其中零表示不存在。应用程序使用该字段进行文本输入，并保留`code`用于导航、快捷方式和游戏。
释放和非打印事件始终不携带字符。记录保持恰好八字节，因此之前构建的 SDK 0.1、1.0 和 1.1 应用程序保留其扁平的 ABI，并将以前保留的字节仅视为忽略的数据。
Rust 参考`Canvas`涵盖了所有可打印的 ASCII 字形，并保留大写和小写，而不是规范化显示文本。

Runtime 接受从 0 到 1000 ms 的轮询超时。它返回一个事件、一个干净的超时或一个稳定的 SDK 错误。其有界 32 事件队列从不增加应用程序控制的内存；一次溢出报告为 `ResourceLimit`，然后清除。重复元数据在 ABI 0.1 中预留，但合成键重复延迟到 SDK 包括一个键映射独立的重复策略为止。

## 验证

主机测试覆盖完整的可打印映射、按下Shift、原始Shift回退、FIFO排序、精确的线宽、字符偏移、重置和溢出行为。注释、音乐、计算器和键盘诊断消耗SDK字符字段而不是维护应用程序本地键映射。Rust和独立C/C++测试编译公共轮询API并断言记录大小。AArch64运行时构建为完全静态可执行文件，没有任何`DT_NEEDED`条目。早期聚焦输入边界是热部署的，并在绑定V0.6 `wl_keyboard`及其seccomp策略后仍然保持活动状态；新的系统字符行为仍然需要物理接受。

设备没有远程密钥注入接口。物理验证必须覆盖小写、按住Shift的大写和所有32个`Sym`组合在Notes中，加上 compositor 截获。这仍然是手动V0.6验收项。
