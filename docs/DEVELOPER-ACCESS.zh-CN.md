# 开发者访问

<!-- doc-locale: zh-CN -->
> [English](DEVELOPER-ACCESS.md) | **简体中文**

## 产品合同

个人拥有的量产设备可以在不成为通用目的Linux开发机器的情况下运行开发应用程序。所有者控制两个独立的设置：

| 设置 | 默认 | 授权授予 |
| --- | --- | --- |
| 开发者模式 | 关闭 | 配对，安装，检查日志，启动，停止和卸载签名的SDK应用程序 |
| 主账号 SSH 壳 | 关闭 | 以主账号身份打开交互式 shell；仍然没有 root 或 sudo 权限 |

Developer Mode 不会启用 root 登录、sudo、任意远程命令、原生应用或未签名包。正常 SDK 工作流不需要 Owner SSH Shell，启用 Developer Mode 也不会意外启用它。

恢复和不受限制的维护仍然是一个单独的可移除恢复
图像仪式。受管理的父级或组织策略可能会锁定开发模式。个人生产策略允许所有者更改它。

## 设备端工作流

1. 完成首次启动设置，并且除非故意需要完整的 Shell，否则保持 **Owner SSH Shell** 关闭。
2. 打开 **设置 > 安全 > 开发者模式**，并确认 **启用**。
3. 选择“配对新计算机”选项。设备在接受新的配对注册请求的持续时间为十分钟。
4. 使用**Paired Computers**来审查最多八个主机标签。按下主机上的 Enter 键以撤销，或选择**全部撤销**。
5. 测试完成后关闭开发模式。这会关闭受限远程通道，并且除非所有者 SSH  shell 独立开启，否则 sshd 会停止。

配对窗口是易失的，由根用户拥有，并绑定在十分钟内。其过期使用 Linux `CLOCK_BOOTTIME`，所以休眠时间会被计算在内，墙钟或 NTP 调整不能延长它。客户端只接收守护进程计算出的剩余时间，并不决定窗口是否打开。一个打开的窗口不会覆盖开发者模式。禁用开发者模式会立即阻止配对和所有应用程序的变更，即使窗口还未过期。

## 开发工作站工作流程

生成一个开发者签名密钥和一个Ed25519 SSH密钥。在打开设备配对窗口后，使用所有者名称和设备IP注册两个密钥。

```sh
cp0ctl key generate developer.key developer.pub
ssh-keygen -t ed25519 -f ~/.ssh/cardputerzero_ed25519

cp0ctl pair developer.pub ~/.ssh/cardputerzero_ed25519.pub workstation \
  --device OWNER@DEVICE_IP
```

第一个配对使用所有者的密码通过标准 SSH 认证。将配对的 SSH 密钥添加到工作站的 SSH 代理或主机配置中，然后使用受限命令：

在输入密码之前，通过受信任的设备/操作员通道验证设备的 SSH 主机密钥指纹。当前受信任的产品 UI 还不显示它，因此配对仍被此发布缺口阻止。不要通过禁用主机密钥检查来接受未知密钥。

```sh
cp0ctl install app.developer.capp --device OWNER@DEVICE_IP
cp0ctl logs dev.example.app 100 --device OWNER@DEVICE_IP
cp0ctl app start dev.example.app --device OWNER@DEVICE_IP
cp0ctl app stop dev.example.app --device OWNER@DEVICE_IP
cp0ctl app uninstall dev.example.app --device OWNER@DEVICE_IP
cp0ctl device remote-status --device OWNER@DEVICE_IP
```

没有ADB服务。`cp0ctl`通过`ssh -T ... cp0-dev`流式传输一个有界的协议；它不上传到一个通用的临时目录，也不调用`scp`、sudo或远程shell。

## 执行边界

`sshd` 只接受预分配的所有者，从不接受 root。所有者的登录 shell 是 `/usr/libexec/cardputerzero/owner-shell`：

- 一个密码认证的`cp0-dev`命令被路由到`cp0ctl dev-session`，并在`cp0-devd`在每条特权请求前检查开发者模式；
- 每对 SSH 密钥由 root 写入一个所有者可读文件中；`restrict,command="/usr/bin/cp0ctl dev-session"`
- 一个交互式或任意命令只有在独立的 `cp0-ssh` 登录组存在时才会到达 Bash；
- 配对的强制命令密钥在 Owner SSH Shell 开启时仍然受约束。SSH 转发，包括 Unix 套接字转发，全局禁用。

`cp0-devd` 使用 `SO_PEERCRED` 验证每个 Unix 连接。UID 1000 只能使用远程开发操作。受信任的 `cp0-shell` UID 只能使用本地配对窗口和撤销操作。每次请求都会重新加载根拥有的设备策略和开发者模式标记。

安装需要以下所有项：

- 开发者模式当前为开启状态；
- `.capp` 结构有效，并带有有效的开发者签名；
- 签名密钥由根所有并配对的主机注册表引用；
- 匹配的信任文件包含精确的32字节密钥；
- appd 独立地通过其现有的根控制路径接受包。

撤销操作会从配对主机注册表中重写所有者授权密钥文件。
开发人员信任密钥在没有剩余配对主机引用它时会被移除。
守护进程启动时，授权密钥文件会从严格的、根拥有的注册表中进行校正。

## 设备余量接受

发布前，V0.6 硬件验收必须验证密码配对、已配对密钥复用、十分钟过期、单个和批量撤销、
Developer Mode 关闭后的拒绝、独立 Owner SSH Shell 行为、正常重启持久性，以及 Recovery
镜像的屏蔽行为。登录 Shell 的 `$2`/`SSH_ORIGINAL_COMMAND` 行为必须在镜像实际使用的
OpenSSH 构建上观察，不能只根据 Shell 单元测试推断。
