# CardputerZero LVGL 9 调用适配器

<!-- doc-locale: zh-CN -->
> [English](README.md) | **简体中文**

这个可选的C适配器将LVGL 9连接到支持的CardputerZero SDK ABI。它不访问DRM、evdev、Wayland或Linux设备。显示刷新使用`cp0_present_rgb565`，键盘输入使用`cp0_poll_key_event`，并且调度使用有界事件等待宿主调用。

应用程序必须编译 LVGL 并编译此目录。 `wasm32` 与独立 C SDK 包含目录。为所选显示模式分配两个完整的 RGB565 缓冲区：

```c
#include <cardputerzero_lvgl.h>

static uint8_t first[CP0_DISPLAY_WIDTH * CP0_STANDARD_DISPLAY_HEIGHT * 2U];
static uint8_t second[CP0_DISPLAY_WIDTH * CP0_STANDARD_DISPLAY_HEIGHT * 2U];
static cp0_lvgl_context_t context;

int main(void) {
    if (cp0_lvgl_init(&context, first, second, sizeof(first), 0U) != CP0_OK)
        return 1;
    for (;;)
        if (cp0_lvgl_run_once(250U) < 0)
            return 1;
}
```

适配器使用 `LV_DISPLAY_RENDER_MODE_FULL`, RGB565颜色格式和一个键盘输入设备。可打印输入来自Runtime生成的 `cp0_key_event_t.character`，因此LVGL文本小部件继承与首次启动相同的Shift和 `Sym` 行为。当事件没有字符时，箭头、Enter、Escape和Backspace的evdev代码映射到相应的LVGL键。由于一个OS应用拥有一个前台表面，因此只能存在一个LVGL上下文。标准模式在受信任的20像素状态栏下方为320x150；沉浸模式为320x170。

构建测试使用最小化的 LVGL 9 声明示例来验证 WebAssembly ABI，而不使用 LVGL。发布的 SDK 工具链将单独锁定并打包测试的上游 LVGL 源代码。
