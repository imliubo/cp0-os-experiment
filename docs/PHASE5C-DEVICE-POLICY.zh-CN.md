# 第5C阶段：设备策略和用户控制模式

<!-- doc-locale: zh-CN -->
> [English](PHASE5C-DEVICE-POLICY.md) | **简体中文**

## 策略边界

`/etc/cardputerzero/device-policy.json` 是 root 管理的设备行为上限。应用、System Shell 和 `cp0-stored` 都不能写入该文件。appd 启动时加载并严格验证最多 16 KiB 的内容，拒绝未知字段，并要求应用和权限列表有序且唯一。

政策权限是`personal`, `parent`或`organization`. 权限
是一个面向用户的拥有标签，而不是一个身份验证凭据。父级和
组织的配置当前是本地和根控制的；远程车队管理和服务注册尚未实现。

Policy v1 可以：

- 锁定开发者模式和恢复模式；
- 禁用所有Store安装；
- 独立禁用自动Store更新；
- 将应用程序启动从 `allow-all` 更改为精确允许列表；
- 在显示用户权限提示之前，全局拒绝任何 SDK 能力。

允许列表也会阻止不在列表中的应用程序在 Store 中安装。全局能力拒绝覆盖之前的应用级 `allow-always` 决定。当策略更改时，不会删除已安装的字节和应用数据。

当 appd 加载一个锁定某种模式的策略时，它会在接受请求之前移除任何过时的模式标记。即使模式被锁定，禁用该模式仍然允许；但启用该模式则不允许。

在配置策略后，重启 appd 以应用启动、Store 和能力规则：

```sh
sudo install -o root -g root -m 0644 device-policy.json \
  /etc/cardputerzero/device-policy.json
sudo systemctl restart cardputerzero-appd.service
```

## 用户控制

Home 的第五项打开 320x170 设置屏幕。它显示开发者模式，
恢复启动，当前权限以及 Store、应用程序启动或能力是否受限。锁定模式不能启用。启用任一模式需要二次确认，默认选择为取消；禁用则立即生效。

从恢复控制台可以访问相同的受限制 appd 协议：

```sh
sudo cp0ctl device status
sudo cp0ctl device developer on
sudo cp0ctl device developer off
sudo cp0ctl device recovery on
sudo cp0ctl device recovery off
```

开发者模式不是一个未签名代码的开关。开发包仍然需要一个有效的开发者签名和一个匹配的根预置公钥，位于`/etc/cardputerzero/trust/developers`之下。appd 会为每个开发安装检查当前策略和持久化的开发者模式标记。

## 恢复启动

恢复启动创建根用户拥有的持久标记 `/var/lib/cardputerzero/registry/recovery-mode`。在接下来的启动和后续启动中， compositor 拒绝启动，并且 `cardputerzero-display-generator` 选择 `cardputerzero-recovery-console.service`，这会激活 `getty@tty1`。因此，LCD 显示本地 Linux 登录控制台，键盘可以输入命令。

恢复功能在未明确禁用前保持启用。要返回 System Shell：

```sh
sudo cp0ctl device recovery off
sudo systemctl reboot
```

模式标记包含一个确切值，使用模式`0600`创建，同步并原子重命名到持久根拥有的注册表中。appd 在读取时拒绝符号链接、可写文件和替换竞态。

## 执行点

```text
root device-policy.json
        |
        +-- Settings mode locks -> appd atomic markers
        +-- developer install -> policy + marker + developer signature/key
        +-- StoreInstall -> Store switch + automatic mode switch + application allowlist
        +-- Start -> application allowlist
        +-- capability request -> global deny before user decision
        +-- next boot -> compositor gate + tty1 recovery service
```

Store UID 只被授权使用与目录绑定的 `StoreInstall` 命令
以及一个专用分页快照，包含已安装的应用 ID、版本和权限。它不能使用启动器列表、读取或更改设置、启动应用、检查日志或使用根开发人员安装路径。

自动化覆盖包括有界严格的策略解码、原子模式状态、锁定模式、allowlist 和能力决策、
开发者安装限制、Store UID 命令隔离、严格 Shell 响应解析、Settings 导航和 320x170 截图
回归。Recovery 启动本身在新镜像或 unit 部署后仍需真机验收。

Phase 5C 已于 2026-07-31 热部署到 V0.6。设备端验收确认默认 personal 设置、Developer
Mode 开/关、无需重启即可切换 Recovery marker、准确清理 marker、Store UID 拒绝，以及
compositor/Shell/appd 均保持 active 且部署后重启次数为零。Weston 还确认 320x170 输出和
物理 `tca8418c` 键盘。开始替代的 24 小时稳定性测试前已关闭 Recovery mode。下一次启动
选择 tty1 的行为有意暂不测试，直到该测试结束或烧录新镜像。
