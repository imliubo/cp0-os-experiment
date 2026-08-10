# 第6A阶段：不可变根文件系统和持久化数据

<!-- doc-locale: zh-CN -->
> [English](PHASE6A-IMMUTABLE-ROOT.md) | **简体中文**

## 图像布局

量产镜像使用 MBR 分区表，并包含三个对齐分区：

| 分区 | 初始大小 | 文件系统 | 用途 |
|---|---:|---|---|
| bootfs | 512 MiB | FAT32 | 固件、内核、DTB和`initramfs8` |
| 根文件系统 | 计算出的 | ext4 | 不可变的操作系统底层文件系统 |
| cp0-data | 256 MiB | ext4 | 可变身份、策略和应用程序状态 |

`cp0-data` 在烧录之前，它在镜像中是存在且有效的。每次启动时，
initramfs 验证它是分区 3，是最后一个分区，并属于 `/dev/mmcblk0`它将分区扩展到SD卡的末尾并运行离线模式 `e2fsck` 跟随 `resize2fs`. 这是幂等的。如果在更新MBR之后中断了分区表增长，下次启动时会观察到更大的分区并完成文件系统增长。如果无法尝试增长，则原来的256 MiB文件系统仍然可用，并在下次启动时重试增长。

上游 `resize` 内核参数和 `rpi-resize.service` 被禁用，因此它们不能扩展分区 2 至分区 3。

## 启动序列

镜像包含默认的内核参数 `cp0.overlay_root=volatile`。initramfs 在 PID 1 启动之前执行以下操作：

1. 在卸载状态下验证并扩展 `cp0-data`。
2. 将挂载的 ext4 根文件系统移动到一个私有的较低挂载点，并以只读方式重新挂载。
3. 创建一个由 root 拥有的 64 MiB tmpfs 上层目录，包含 16,384 个 inode。
4. 将 OverlayFS 挂载为 `/`.
5. 将 `cp0-data` 以读写方式挂载与 `nodev,nosuid,noexec,noatime`。
6. 在`cp0-data`上生成设备机器码，如果不存在的话。
7. 将仅批准的持久目录和文件绑定到新的根。
8. 将所有诊断挂载点移动到最终的根文件系统 `/run` 中。

任何缺失标签、重复标签、无效布局标记、挂载失败或绑定失败都会进入initramfs恐慌路径。它绝不会以可写根或没有其持久安全状态的方式静默启动产品配置文件。

切换根目录后，仅根用户可用的诊断工具位于：

```text
/run/cardputerzero-root/lower     ext4, read-only
/run/cardputerzero-root/volatile  tmpfs, nodev, 64 MiB maximum
/run/cardputerzero-data           ext4, rw,nodev,nosuid,noexec
```

从 `cmdline.txt` 中移除 `cp0.overlay_root=volatile` 是显式的下层服务恢复路径。它启动下层根的读写模式，并不挂载或绑定 `cp0-data`。这与设置中的“恢复启动”不同，“恢复启动”保持不变的/持久化的布局，仅选择 tty1 控制台而不是 compositor。

第6C阶段将下根路径正式化为单独命名的`recovery`镜像配置文件。它还遮盖了 compositor 和应用激活，而不仅仅是依赖编辑后的命令行；请参见`docs/PHASE6C-RECOVERY-IMAGE.md`。

## 持久允许列表

导出的镜像种子了一个版本化的`cp0-data-layout-v2`布局。现有v1媒体在initramfs中通过在v2标记提交前添加所有者身份/home根路径来迁移。只有这些路径在重启后存活：

| 持久化源 | 运行时路径 |
|---|---|
| `cardputerzero/` | `/var/lib/cardputerzero` |
| `etc-cardputerzero/` | `/etc/cardputerzero` |
| `extrausers/` | `/var/lib/extrausers` |
| `home/` | `/home` |
| `ssh/` | `/etc/ssh` |
| `network-connections/` | `/etc/NetworkManager/system-connections` |
| `network-state/` | `/var/lib/NetworkManager` |
| `machine-id` | `/etc/machine-id`（只读绑定） |
| `random-seed` | `/var/lib/systemd/random-seed` |

这涵盖了已安装的应用程序、注册表和权限决策，私有应用数据，所有者身份/家庭目录，信任/撤销策略，LoRa策略，Wi-Fi凭据，NetworkManager状态，SSH主机密钥，机器身份和随机种子。
initramfs 在systemd启动前创建持久的机器身份，因此SSH主机密钥配置不能使用`ConditionFirstBoot=yes`。`cardputerzero-ssh-prepare.service`则在`ssh.service`之前运行，直接在持久的`/etc/ssh`绑定挂载中创建任何缺失的密钥，并在可以监听之前验证服务器配置。
相同的早期身份设置意味着产品不需要systemd之后的`systemd-machine-id-commit.service`；它仅在产品配置文件中被屏蔽，以避免对只读的持久绑定挂载进行强制提交的保证失败。一切写入`/etc`或`/var`的内容在重启时都会被丢弃。

数据文件系统根目录的模式是`0700`。应用程序不会收到其挂载路径。它们唯一的可写持久接口仍然是配额限制存储代理；appd和代理保留其现有的systemd路径允许列表。

## 验证

`tests/test-built-rootfs-profile.sh` 注入到 pi-gen 的 finalise 阶段，
在最终生成 initramfs 之后并且在镜像卸载和压缩之前。它验证实际挂载的启动、根和数据文件系统，
启用的单元、持久种子、默认内核参数、代理删除和 initramfs 具体条目。它还提取生成的 initramfs，并验证两个 CardputerZero 启动脚本是否从其阶段 `ORDER` 文件调用；
存档中存在可执行文件并不足够。

集成开发候选版本是：

```text
deploy/image_2026-07-31-cardputerzero-os-d19d1ca-cp0-os-dev.img.xz
SHA-256 e965d4dc6b9d42bb03a37e70ea700c7e128b5a10c15ddb54f8a91cb20e448c05
```

它压缩后是244,050,632字节，未压缩是2,097,152,000字节。
独立只读检查确认了一个包含512 MiB的MBR。
`bootfs`, 1,283,457,024 字节 `rootfs` 和 256 MiB `cp0-data` 分区。
根文件系统使用大约 770 MiB，bootfs 使用大约 49 MiB，而预置数据文件系统使用不到 1 MiB。压缩流、包配置文件、文件系统标签、默认命令行、持久布局和所需的 initramfs 文件都通过了发布检查。

分区扩展脚本还针对具有特权的回环MBR进行了测试：

- 分区 3 和其 ext4 文件系统从 64 MiB 增长到 344 MiB；
- 结果中的块设备和文件系统大小是相同的；
- 第二次调用是一个 no-op；
- 分区 2 在文件系统访问前被拒绝。

最终验收仍需要刷入集成镜像，验证V0.6首次启动，重启持续性，中断写入恢复以及完整的24小时SD写入预算。直到这些硬件检查通过，对应的路线图验收项仍然保持开放。

2026年7月31日的硬件跟进发现，`d19d1ca`镜像包含了自定义脚本，但省略了initramfs-tools生成的`ORDER`文件，因此根文件系统保持可写状态，`cp0-data`未挂载。还发现状态单元隐藏了`/proc/cmdline`并错误地返回成功，且 compositor 可以在 udev 冷插拔完成前被跳过。该制品和后续继承相同 initramfs 注册的图像不是候选发布版本。修正后的图像必须通过新的`ORDER`门和一次新的V0.6启动测试，才能替换上述候选图像。

## 编写和服务控制

journald, `/tmp`, `/var/tmp` 和稳定性报告保留在RAM中。zram没有写回，apt定时器被禁用，24小时监控默认拒绝超过64 MiB的SD写入。内核sysctls限制了不安全链接行为，核心转储，dmesg，内核指针和非特权BPF。

Compositor、System Shell 和 appd 使用显式的权限边界、原生系统调用架构、命名空间/进程/内核保护、不可写执行内存和零交换。硬件能力代理服务仅保留其固定的设备访问权限。
