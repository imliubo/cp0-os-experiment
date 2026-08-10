# Phase 3J: 受限逻辑GPIO代理服务

<!-- doc-locale: zh-CN -->
> [English](PHASE3J-GPIO-BROKER.md) | **简体中文**

## 范围

V0.6 GPIO API 暴露了四个逻辑连接器输出，并没有原始的 Linux GPIO 接口：

| SDK 行 | 固定内核属性 | 目的 |
|---|---|---|
| `grove-function` | `grove_fun/brightness` | 葡萄架连接器功能输出 |
| `external-usb-function` | `ext_usb_gpio_fun/brightness` | 外部USB功能输出 |
| `grove-5v-power` | `grove_5v_out/brightness` | 葡萄藤 5 V 电轨控制 |
| `external-5v-power` | `ext_5v_out/brightness` | 外部5 V 电轨控制 |

所有操作都是在现有`hardware.gpio`权限下的布尔读或写请求。应用程序不能命名路径、gpiochip、引脚号、方向、边缘、拉模式、驱动强度或pinmux模式。添加未来的输入引脚需要一个新的经过审核的逻辑枚举成员，而不是接受清单或RPC请求中的一个数字。

## 硬件证据

只读 V0.6 检查发现三个 gpiochips 和固定 BSP 提交 `c3b254819307c177a34100b66fe19e52059ce8c4` 的实时上游覆盖层。覆盖层
和内核所有权数据保留，以及其他内容：

- GPIO7/8/9/10/11 和 GPIO22 用于 SPI 芯片选择/数据；
- GPIO12/13 用于红外，GPIO17 以及 GPIO18-21 用于音频；
- GPIO24/25 用于扬声器/显示器和键盘中断 GPIO27；
- 键盘重置/LED控制，显示电源和电源失效控制扩展行。

这些行即使在某一时刻的 pinctrl 快照报告其为未占用时也会被排除。该 API 基于命名的板级功能，而不是偶然的驱动程序状态。这四个选定的输出已经在 V0.6 覆盖层中具有稳定的 LED 类属性，因此不需要用户空间 pinmux 或 gpiochip 的所有权。

## 信任流

```text
WASM gpio SDK call with fixed enum + bool
  -> Runtime validates enum/value and sends bounded Unix JSON
  -> appd binds SO_PEERCRED to the active app UID and systemd cgroup
  -> appd verifies root-owned manifest and hardware.gpio decision
  -> root-only cp0-gpiod socket accepts only appd
  -> cp0-gpiod maps the enum to one compiled-in sysfs attribute
  -> kernel LED/GPIO driver performs the boolean operation
```

`cp0-gpiod` 在专用的 `cp0-gpio` 没有能力的账户，
私有设备，只有 `AF_UNIX`, 8 MiB 内存限制和四个显式
`ReadWritePaths`服务没有接收到 gpiochip 设备节点。

上游 BSP 为这些属性安装了开发导向的 `0666` 模式。CardputerZero OS 用 `0660 root:cp0-gpio` 覆盖所有四个模式，因此登录用户和应用程序 Runtime 无法通过 sysfs 绕过代理。应用程序沙盒在任何情况下都不会挂载主机的 sysfs；收紧的模式为其他本地进程提供了纵深防御。

## 验证

自动化覆盖包括：

- 严格 2 KiB 协议帧，并拒绝未知行/字段；
- 固定枚举到路径的映射并模拟后端的读写行为；
- appd 请求，行，值和请求-ID 关联；
- `hardware.gpio` 表现/权限路由；
- 运行时布尔解码和不匹配行拒绝处理；
- Rust、C11、C++17 和 WIT SDK 接口；
- 服务加固, sysfs 模式, 镜像和热部署断言;
- AArch64 gpiod/appd/Runtime 和 wasm32 Hello Card 构建。

Hello CardputerZero `G` 读取并翻转仅 `grove-function`. 电源线
在这个例子中从未改变。物理读/写和拒绝接受是
通过真实身份探测器自动化实现的
`PHASE3M-DEVICE-CAPABILITY-ACCEPTANCE.md`，但执行将在活跃的24小时 compositor/Shell/appd 稳定性运行结束后延迟进行。
