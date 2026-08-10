# 显示设置代理服务 v1

<!-- doc-locale: zh-CN -->
> [English](DISPLAY-SETTINGS-BROKER.md) | **简体中文**

`cp0-displayd` 是受信任的 System Shell 首先使用的特权设置提供者。它不是一个应用程序能力，也没有 SDK 或 Runtime 主调用。应用程序不能请求、探测或继承显示控制。

## 硬件合同

V0.6 设备树 overlay 将背光节点命名为 `m5stack,pwm-backlight`
`backlight`，这映射到这些固定的sysfs属性：

- `/sys/class/backlight/backlight/max_brightness` 是只读的；
- `/sys/class/backlight/backlight/brightness` 是唯一可写属性。

代理将原始级别转换为观察到的百分比，使用有符号整数算术。请求可以设置5%到100%。调整请求使用固定的10%步长，并在安全边界处进行限制，因此全局快捷键不能关闭唯一的本地显示。每次成功写入都会在Shell更新前读回。

亮度属性必须在一次 `write(2)` 调用中接收完整的十进制值和尾随换行符。V0.6 sysfs 实现应用第一个片段但在高级文件偏移处拒绝第二次写入。因此，代理在写入前编码完整的属性值并将短写视为设备错误。

固定路径由固定BSP源建立。V0.6检查确认了路径、其100级范围以及由代理控制的65/75%写回点。路径缺失、意外值、权限失败或写回失败会使控制不可用。生产UI从不启用模拟回退。

## 信任边界

systemd socket 的权限是 `0660 root:cp0-display-control`；只有 `cp0-shell` 属于该控制组。
服务会独立解析 `cp0-shell` UID，并检查每条已接受连接的 `SO_PEERCRED` 是否匹配。仅拥有
另一个 `cp0-control` 组身份或 App 身份不足以获得访问权限。

`cp0-displayd` 作为专用的 `cp0-display` 账户运行，具有空的能力集，私有设备，仅 Unix 网络和严格的文件系统。服务沙盒仅授予对亮度属性的写访问权限，而 tmpfiles 将底层 sysfs 模式限制为 `0660 root:cp0-display`。修改请求发出有界对等 UID 和观察到的百分比审核行到 RAM 支持的日志。

## 协议和Shell行为

每行JSON协议有2 KiB的帧上限，具体版本为1，
严格拒绝未知字段，并有三个命令：`get-state`，
`set-brightness` 和 `adjust-brightness`。响应要么返回观察状态，
要么返回有界错误。不可用状态需要明确表示，并不能携带过时百分比。

Fn+U/Fn+I 仍然是 compositor 所有的全局操作。受信任的 Shell 将它们转换为一个代理调整，并在它的临时覆盖层中渲染返回的值。设置亮度行使用相同的路径。当套接字或硬件不可用时，行和覆盖层显示 `UNAVAILABLE`；不呈现任何仅本地值作为硬件状态。

## 验证状态

本地覆盖包括协议封装和验证、安全边界裁剪、不可用硬件处理、严格的C响应解析、Shell事件路由、未改变的320x170像素快照、systemd限制和镜像/部署集成。在设备候选者准备之前，需要进行Linux AArch64构建和完整仓库检查。

保留的24小时稳定性证据，物理sysfs身份，
65/75%写/读回，非Shell拒绝和重启检查均已通过。Fn+U/Fn+I LCD重叠显示、设置导航和输入延迟仍需操作员观察；短性能运行记录零SD写入。
