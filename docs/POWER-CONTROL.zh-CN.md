# 电源控制

<!-- doc-locale: zh-CN -->
> [English](POWER-CONTROL.md) | **简体中文**

## 合同

重启和关机是受信任的全局操作。System Shell 保留现有的确认界面，然后向 root 所有的
`cp0-powerd` 服务发送一个有界请求。应用、appd、SDK、Owner 账户和开发者通道都没有电源控制端点。

版本1协议接受恰好两个命令：

- 重启
- 关机。

请求和响应是换行符分隔的严格 JSON 框，每条框限制在 1024 字节。成功的响应绑定请求 ID 和接受的动作。

## 权限边界

cardputerzero-powerd.socket 的模式为 0660，所有者为 root，组为 cp0-power-control。只有 cp0-shell 被添加到该组。守护进程仍然使用 Linux SO_PEERCRED 对每个连接进行身份验证，并仅接受 exact cp0-shell UID，因此仅组成员身份不会授予权限。

cp0-powerd 以 root 身份运行，具有空的能力边界集，NoNewPrivileges=yes，ProtectSystem=strict，并且仅使用 AF_UNIX 作为其地址家族。它没有通用命令、单元名称、参数、路径或环境字段。后台将关闭动作枚举映射到以下之一：

    /usr/bin/systemctl --no-block reboot
    /usr/bin/systemctl --no-block poweroff

该服务不授予 sudo 权限、shell 访问、System Shell 的 D-Bus 访问，也不授予一般的 systemd 控制权限。恢复镜像隐藏该服务和套接字。

## 验证

主机测试涵盖严格的协议解码、帧边界、固定命令映射，
后端失败处理、响应/动作绑定、源边界中的对等方凭证强制执行，
systemd 硬化和产品/恢复镜像集成。

V0.6 的接受需要一个新的产品镜像。重启必须断开 SSH，生成一个新的启动 ID 并返回 Home。关机必须停止内核并将设备置于物理电源恢复前的关机状态。两个操作都必须从设备上的确认 UI 中检查；主机端的 systemctl 不是等效的接受路径。
