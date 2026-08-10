# 第6C阶段：独立恢复镜像配置文件

<!-- doc-locale: zh-CN -->
> [English](PHASE6C-RECOVERY-IMAGE.md) | **简体中文**

## 目的

恢复镜像是一个单独的维护制品，不是替代的桌面环境，也不是一种较不安全的产品模式。它使用与产品镜像相同的锁定BSP和经过审计的二进制文件，但故意启动一个可写的较低层级根文件系统到`tty1`。这给操作员提供一个可预测的本地键盘和SSH环境，当产品 compositor 或不可变根路径无法启动时。

每个构建好的根文件系统包含恰好一个根拥有的标记：

```text
/etc/cardputerzero/image-profile
```

它的值是`product`或`recovery`. 配置文件控制内核命令行、启用的单元、artifact后缀、LCD横幅以及最终挂载的根文件系统释放门。仅凭文件名从不被信任为配置文件权威。

## 构建

`product` 仍然是默认值。显式构建一个恢复补丁，使用不同的容器名称，以便中断的产品构建不会被误认为是可恢复的恢复构建：

```sh
CP0_IMAGE_PROFILE=recovery \
CP0_BUILD_CONTAINER=cardputerzero-pigen-recovery \
CP0_FIRST_USER_PASSWORD='one-time-maintenance-password' \
CP0_SSH_PUBLIC_KEY='ssh-ed25519 ...' \
./image/build-image.sh
```

结果使用后缀 `-cp0-os-recovery.img.xz`。恢复构建拒绝 `CP0_STORE_PUBLIC_KEY`；维护介质不得成为Store信任根。构建仍然需要显式的本地登录密码。提供SSH密钥也会使远程维护使用操作员控制的密钥而不是共享凭证。

## 启动合约

恢复配置文件与量产镜像相比有这些 fail-closed 的差异：

- `cp0.overlay_root=volatile` 缺失，因此根文件系统可写。
- `getty@tty1`, LCD 加载摘要仍然可用；NetworkManager 和 SSH 仍然可用；
- LCD 标题是 `CardputerZero OS RECOVERY`；
- compositor 和 System Shell 被遮盖；
- appd 和每一个能力/Store 激活套接字都被屏蔽了；
- initramfs 既不会扩展 `cp0-data`，也不会挂载它。

最后一项可防止 recovery boot 静默地把可变应用、身份或网络状态绑定到 maintenance root。root 只能通过显式且经过审核的 recovery procedure 检查或挂载目标。保留三分区布局可让镜像 exporter 和 partition parser 保持一致，但该 profile 中的数据分区处于 inactive 状态。

## 发布门禁

最终的 pi-gen 验证器从挂载的 ext4 根文件系统中读取配置标记。对于量产镜像，它需要 immutable-root 参数、compositor、seatd、恢复选择器和所有代理套接字。对于恢复镜像，它需要该参数不存在、`tty1` 启用，并且每个应用程序执行入口点链接到 `/dev/null`。两个配置保留相同的包、initramfs、世界可写文件和三分区检查。种子数据分区中复制的标记必须与较低的根文件系统匹配。

仓库测试也在任何克隆、包构建或Docker操作之前拒绝未知的`CP0_IMAGE_PROFILE`值，验证配置文件特定的artifact名称，并扫描恢复分支以防止意外的compositor/appd激活。

第一个可重复的恢复候选者完成了挂载根文件系统和initramfs门：`PASS built rootfs and initramfs profile: recovery`

```text
artifact: image_2026-07-31-cardputerzero-os-cp0-os-recovery.img.xz
size:     243974280 bytes
sha256:   2895e90f592c4e9c892873eb328f097e6d45598d15cff95b6f7c4b1c59746d92
```

## 操作限制

将此镜像刷入会覆盖所选SD卡。它无法备份被其替换的相同卡。因此，在重新刷入之前，必须从运行中的产品恢复控制台导出用户数据，或者必须在单独的受信任计算机上读取原始卡。经过审计的受限备份/恢复格式和出厂重置工作流程在`docs/PHASE6D-RECOVERY-DATA.md`中定义；它们不将不受限制的`tar`命令视为恢复协议。

最终验收需要构建压缩的制品，通过用户协助刷入单独的SD卡，确认LCD恢复横幅和键盘，并证明重启后 compositor/appd 套接字仍然不存在。
