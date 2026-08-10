# 第4阶段：小屏UI和参考应用

<!-- doc-locale: zh-CN -->
> [English](PHASE4D-SDK-UI-EXAMPLES.md) | **简体中文**

## SDK UI 表面

Rust SDK 现在提供无需动态分配的 RGB565 `Canvas`，支持 320x150 标准 App surface 和
320x170 沉浸式 surface。它提供裁剪绘制、紧凑 ASCII 字体、按钮和进度条，同时仍由
应用持有 framebuffer。

可选的LVGL 9 C适配器将一个完整的渲染RGB565显示和按键输入绑定到公共CardputerZero主机调用。它故意没有Linux后端。两种完整的缓冲区使刷新语义确定，并在标准模式下消耗192,000字节，在沉浸模式下消耗217,600字节。

## 参考应用

`examples/calculator` 直接行使集中键输入和小屏渲染而无需权限。`examples/camera` 行使受信的`camera.capture`权限，处理拒绝，并使用固定320x170捕获帧。两个应用程序都是`no_std`，仅使用`cp0-sdk`，编译为WebAssembly并在PC模拟器下运行。

模拟器的关键词汇包括数字和计算器运算符。CI 运行 `12 + 3 =`，通过帧提交次数验证显示结果，并在 Enter 事件后捕获一个确定性的相机固定点。PPM 帧和 JSON 配置文件保持在被忽略的 `target/` 输出下。

## 验证

- Rust SDK 单元测试覆盖画布大小检查和裁剪绘制。
- LVGL适配器编译为独立的wasm32 C11，错误视为警告。
- 计算器和相机通过`cp0ctl`构建并在模拟器中运行。
- 能力分析验证了仅调用了 Camera 接口 `camera.capture`。
- 视觉检查确认两个参考布局都适合320x150的表面。

第四阶段冻结扁平的WAMR ABI，从一个机器可读的合约生成 Runtime/C/Rust 接口绑定，并验证 WIT 映射和签名兼容性。
