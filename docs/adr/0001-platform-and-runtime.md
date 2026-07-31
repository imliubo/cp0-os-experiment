# ADR 0001：平台、图形栈与应用运行时

- 状态：Accepted
- 日期：2026-07-30

## 决策

1. 基础系统从 Debian arm64 minimal 开始，不在首版切换 Yocto。
2. 图形栈采用 DRM/KMS + Wayland，首个 compositor 使用 Weston kiosk shell。
3. 系统只支持单前台应用，不提供重叠桌面窗口。
4. 第三方应用必须使用新 SDK 并运行在 WASM 中，不兼容传统 Linux GUI 应用。
5. 设备运行时采用 WAMR interpreter/AOT；WIT 描述公共类型接口，独立的机器可读
   flat ABI 契约作为 Runtime 与语言导入的唯一生成源。
6. 原生进程只用于可信系统组件，不开放原生第三方应用安装。

## 理由

CM0 只有 512 MB RAM。Debian 能最大化复用已有驱动和镜像工作，Weston 能先验证
Wayland 路径而不立即承担 compositor 维护成本。WAMR 的嵌入式资源占用比完整 JIT
运行时更适合该设备，WASM 加进程沙箱则提供清晰、可审计的第三方能力边界。

## 后果

现有 LVGL framebuffer 应用需要迁移或重写。SDK 必须提供足够完整的 UI、媒体和硬件
能力，否则开发者会要求原生逃生通道。若 Weston 后续无法实现产品交互，再以真实
测试结果决定是否维护 wlroots compositor。
