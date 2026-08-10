# 阶段6D：有界备份、恢复和工厂复位

<!-- doc-locale: zh-CN -->
> [English](PHASE6D-RECOVERY-DATA.md) | **简体中文**

## 范围

Phase 6D 为完整的 `cp0-data` 文件系统提供离线操作员工作流。它不是一个应用程序 API、云备份服务或任意路径的存档提取器。应用程序仍然只能看到其现有的配额限制存储和能力代理服务。

底层 `/usr/bin/cp0-recovery` 工具绝不会挂载、格式化或删除文件系统。它可以创建或验证
`CP0 backup v1` 文件，但只能把已验证文件恢复到仅 Owner 可访问的空目录；该目录最多可
预先包含一个空的 ext4 `lost+found`。仅 root 可用的 `device-recovery-data` wrapper 负责
块设备验证，以及显式的破坏性恢复或重置操作。

## 格式和验证

`CP0 backup v1` 是一个确定性且边界限定的二进制流。它存储一个版本化的头部，固定大小的条目头部，UTF-8 相对路径，类型，模式，UID/GID，长度，每个普通文件一个 SHA-256，以及整个负载的 SHA-256。解析器拒绝：

- 绝对路径，`.`/`..`，超过32个组件的路径和路径在固定的`cp0-data-layout-v2`顶级允许列表之外，包括本地Owner身份数据库和持久化家目录；
- 重复或未排序的路径、未知类型/标志和不一致的长度；
- 符号链接，硬链接，设备，套接字，setuid/setgid/粘滞位和世界可写条目；
- 超过65,536条条目，一个文件超过4 GiB或一个负载超过64 GiB；
- 缺失布局/配置标记、损坏以及任何多余的字节。

备份从只读挂载读取每个文件两次，并要求其设备、inode、元数据、长度和摘要保持不变。恢复在打开空的目标之前验证整个输入，然后在流式传输内容时再次验证。恢复过程中断电会导致不完整的数据文件系统，并需要重复恢复；它从不将部分数据报告为成功。

## 数据敏感性

完整备份包含本地Owner身份和密码哈希、已安装的应用程序、权限策略、私有应用存储、文档、Store信任状态、Wi-Fi凭证、SSH主机密钥、机器身份和随机种子。版本1故意离线并提供损坏检测，不提供加密或证明备份是由谁创建的。仅在操作员控制的加密介质上存储和传输它。敌对方如果可以替换备份及其哈希，可以替换持久性策略；显式的根恢复仪式是信任决策。

## 维护配置文件

设备包装器接受以下任意一种：

- 独立的 `recovery` 镜像，其 compositor 和应用程序入口点被遮罩；或
- 一个 `product` 层级较低的维护启动，缺少 `cp0.overlay_root=volatile`，因此 `cp0-data` 不会被 initramfs 挂载。

正常的 Settings “恢复启动”仍然使用 OverlayFS，并不满足这一要求。目标必须是一个未卸载的真实分区 3，ext4 格式，标签为 `cp0-data`，并且不能是当前的根文件系统。

## 操作

验证是非破坏性的，并不需要 root 权限：

```sh
/usr/libexec/cardputerzero/device-recovery-data verify \
    /media/operator/cardputerzero.cp0backup
```

备份还需要单独挂载的输出文件系统。包装器拒绝将备份写入OS根、启动文件系统或源数据分区：

```sh
sudo /usr/libexec/cardputerzero/device-recovery-data backup \
    /dev/mmcblk0p3 /media/operator/cardputerzero.cp0backup
```

恢复首先验证一个`profile=product`备份，然后需要确切的确认令牌才能重新格式化分区3：

```sh
sudo /usr/libexec/cardputerzero/device-recovery-data restore \
    /dev/mmcblk0p3 /media/operator/cardputerzero.cp0backup \
    RESTORE-CP0-DATA
```

工厂重置仅在产品低根维护启动时可用。产品镜像在安装默认应用、设备策略和可选的Store信任根后生成其工厂捆绑包。种子故意不包含机器ID、随机种子、网络配置文件或SSH主机密钥。启用的`regenerate_ssh_host_keys.service`和正常首次启动路径在重置后创建新的设备标识。

```sh
sudo /usr/libexec/cardputerzero/device-recovery-data factory-reset \
    /dev/mmcblk0p3 RESET-CP0-DATA
```

独立的恢复文件不携带部分产品的工厂种子：它不知道单独构建的产品镜像中的Store根或嵌入的策略。它可以验证、备份和恢复附加的产品数据分区，而工厂重置仍然绑定到匹配的产品较低根。

## 接受状态

本地单元测试覆盖了确定性往返、元数据和私有数据，
数据损坏/尾随字节拒绝、不安全源项和非空目标拒绝。静态发布测试绑定验证前格式化，精确确认标记，块设备不变量，配置门，镜像安装以及恢复镜像中不存在工厂种子。AArch64 二进制文件通过 ELF 和 RELRO 检查。最终挂载根文件系统的门也执行安装的二进制文件以针对产品工厂种子，并要求执行 `profile=product`。

第一个产品候选者完成了挂载根文件系统/initramfs 关卡，并进行了独立的ARM64 恢复检查：

```text
artifact:       image_2026-07-31-cardputerzero-os-phase6d-product-cp0-os-dev.img.xz
size:           244888132 bytes
sha256:         d72ce50b465788c710d4e8917b6986ecc86850eec059f9d82aad9b0606b10113
factory bundle: 10852 bytes, 29 entries, 11 files, 8255 data bytes
factory sha256: 8bb2e73e162aa3dab897fbf184b8cd028696962fcfc35f56a1dca165df683352
```

恢复的种子保留了默认的应用程序、设备策略和Store布局，而`machine-id`, `random-seed`, 网络状态、NetworkManager连接和SSH状态为空。

最终验收仍需要一个一次性SD卡：创建备份，在第二个文件系统上检查和验证它，格式化后恢复它，重新启动到产品配置文件，然后执行工厂重置并确认新设备密钥。这些操作故意不在活跃的稳定性测试设备上运行。
