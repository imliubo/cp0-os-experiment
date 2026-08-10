# Phase 3D: 通知能力代理服务

<!-- doc-locale: zh-CN -->
> [English](PHASE3D-NOTIFICATION-BROKER.md) | **简体中文**

## 请求路径

通知代理是第一个类型化的SDK能力服务。`appd` 拥有
两个socket激活的监听器：

- `/run/cardputerzero-appd/control.sock` 是模式 `0660`, 属于控制组并且只接受根或受信任的Shell UID `SO_PEERCRED`;
- `/run/cardputerzero-broker/runtime.sock` 是模式 `0666` ，所以不同的应用 UID 可以连接，但每个请求都由 `SO_PEERCRED` 验证，并映射回根拥有者注册表中已安装且当前运行的一个应用程序。对方 PID 还必须是该应用程序精确的 systemd cgroup 的成员；使用相同 UID 的宿主进程将被拒绝。

代理套接字是唯一挂载到应用程序空根目录下的主机IPC端点，位于`/run/cardputerzero/broker.sock`处。请求不能命名应用程序、权限、文件、设备或主机命令。它仅包含一个有界的通知标题和正文。

## 授权和资源限制

`notifications.post` 必须存在于标准安装清单中。然后，共享权限协调器返回允许、拒绝、未声明或一个待处理的提示 ID。新授予的应用程序会重新尝试已输入的请求；提示解析永远不会自动重放不可信的负载。

通知标题最多32个字符，正文最多160个字符。
控制字符和超过4 KiB的帧被拒绝。内存中的FIFO最多保存八个通知，并返回`resource-exhausted`而不是无限制地增长。

可信的Shell通过认证的控制套接字获取通知。`cp0ctl`暴露了启动时的诊断命令：

```sh
cp0ctl broker notify <title> <body>
cp0ctl permission pending
cp0ctl permission resolve <prompt-id> once|always|deny
cp0ctl notification take
```

`cp0ctl broker notify` 不进行身份检查绕过。只有在作为前台测试应用注册的 UID 运行时才有用。

System Shell 协议 v4 将每个出队项呈现为 compositor 强制的可信横幅，持续四秒。应用保持键盘焦点，而 Shell 占据顶部 88 像素。权限提示优先并切换到完整的可信表面；Home、Tasks、Power 和应用退出会清除可见横幅。应用不能控制此策略或绘制到可信层。

## 部署不变量

appd 服务没有任何能力，只有 `AF_UNIX`，并且 cgroup 限制为 24 MB。
`ProtectSystem=strict` 保持启用；只有权限注册表目录可写，所以 `allow-always` 和 `deny` 可以原子地提交。代理套接字目录由 root 拥有且应用程序不可写。启动时主机验证在进入 bubblewrap 之前会拒绝缺少的、符号链接的、非套接字或非 root 拥有的代理端点。

## V0.6 验证

Phase 3D 在 V0.6 设备上无需重启或刷机进行了热部署。

- 最终的 aarch64 `cp0-appd` SHA-256 是
  `e2ad7cb396a19ff2163f45930fdc1f030db6056cfbf25ac37c110ab2b50eb0b1`；
  `cp0ctl` 是
  `0dc07fb09643ef0902b421ed06e0d006ee7c6a4d8d1019db21071cbae3a71b66`。
- 控制套接字是`root:cp0-control 0660`；代理套接字在`root:root 0711`目录内的`root:root 0666`。
- Bubblewrap 挂载了代理端点，Hello app 保持活跃在
  `/system.slice/cardputerzero-app-20000.service`.
- 来自`pi`的一个请求被拒绝，因为其UID未注册。一个使用UID 20000的宿主机进程也被单独拒绝，因为其PID超出了精确的应用cgroup范围。
- 一个cgroup绑定的请求返回了提示1，并带有标准应用名称、权限和表现原因。`allow-always`持续存在`/var/lib/cardputerzero/registry/permissions.json`作为`root:root 0600`。
- 重试返回的通知 ID 1，并且 Shell 通道检索到了确切的信任应用身份、标题和正文。测试应用随后干净地停止，而 appd、compositor 和 System Shell 仍然保持活动状态。
