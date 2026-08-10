# 开发者模式部署

<!-- doc-locale: zh-CN -->
> [English](developer-mode.md) | **简体中文**

在更改物理设备之前，请阅读此内容。开发者模式是一个由所有者控制的、受限的部署渠道，用于签名的SDK应用。它不是应用权限、root访问、sudo、本地包渠道或通用SSH shell。

## 准备工作站

使用一个匹配的 DevKit 并先通过其 doctor 检查。生成两个独立的密钥对一次：

```sh
cp0ctl key generate /secure/developer.key ./developer.pub
ssh-keygen -t ed25519 -f ~/.ssh/cardputerzero_ed25519
```

原始的32字节`developer.pub`验证`.capp`签名。Ed25519 SSH
密钥验证工作站到强制`cp0-dev`命令。保持两个私钥远离设备，在App项目之外，并且不在源
代码控制之外。

## 在设备上授权

1. 完成首次启动设置，并从受信任设备UI中获取用户选择的用户名和当前IP。
2. 打开 **设置 > 安全 > 开发者模式**，并确认 **启用**。
3. 选择**配对新计算机**。根拥有的窗口持续十分钟。
4. 从工作站运行：

```sh
cp0ctl pair ./developer.pub ~/.ssh/cardputerzero_ed25519.pub workstation \
  --device OWNER@DEVICE_IP
```

第一次配对使用所有者的密码通过标准SSH认证。设备记录开发者的公钥、SSH公钥和主机标签。在`ssh-agent`或工作站的SSH主机条目中配置该私钥，以便用于后续命令。永远不要在App、脚本、Skill或命令行参数中放置所有者的密码或私钥。

在发送所有者密码之前，通过受信任的设备/操作员通道验证设备 SSH 主机密钥指纹。当前产品 UI 还未显示该指纹。在不信任的网络上，停止并报告此发布缺口，而不是接受未知密钥。不要禁用主机密钥检查。

对于后续命令，要么加载专用密钥 `ssh-add`，要么使用窄 SSH 配置条目：

```text
Host cardputerzero-dev
    HostName DEVICE_IP
    User OWNER
    IdentityFile ~/.ssh/cardputerzero_ed25519
    IdentitiesOnly yes
```

通过该条目，请使用`--device cardputerzero-dev`。不要添加通配符主机规则或放松主机密钥验证。

所有者可以在**配对的计算机**下查看最多八条记录，按 Enter 可撤销一条记录，或使用**全部撤销**。每个新工作站都需要单独配对；不要复制旧工作站的 SSH 私钥。

## 构建和部署

在配对期间注册的开发人员密钥上签名：

```sh
cp0ctl package ./my-app ./my-app.unsigned.capp
cp0ctl sign developer ./my-app.unsigned.capp ./my-app.developer.capp \
  /secure/developer.key
cp0ctl verify ./my-app.developer.capp
cp0ctl install ./my-app.developer.capp --device OWNER@DEVICE_IP
cp0ctl logs dev.example.my-app 100 --device OWNER@DEVICE_IP
cp0ctl app start dev.example.my-app --device OWNER@DEVICE_IP
cp0ctl app stop dev.example.my-app --device OWNER@DEVICE_IP
cp0ctl app uninstall dev.example.my-app --device OWNER@DEVICE_IP
cp0ctl device remote-status --device OWNER@DEVICE_IP
```

`cp0ctl` 通过 `ssh -T ... cp0-dev` 流式传输一个有界的协议。它不使用 `scp`, 一个通用的临时上传，sudo 或 Bash。设备在每次变更时重新检查策略、开发者模式、配对和包签名。

在安装或启动前，请确认没有活跃的稳定性验证、恢复、更新或工厂接受过程。启动应用会取消活跃的稳定性验证。设备操作需要明确授权；本地构建、模拟、签名和签名验证不需要。

## 关闭访问

测试完成后关闭开发模式。配对和应用变更立即失败，除非独立的**所有者 SSH  shell** 设置为开启状态。现有应用保持正常的运行时权限隔离；禁用开发模式不会转换或扩展这些权限。

开发者 SSH Shell 不需要用于 App 开发。即使独立启用，配对密钥也仅限于 `cp0-dev`，转发仍然关闭，并且开发者模式从不成为系统组件更新路径。
