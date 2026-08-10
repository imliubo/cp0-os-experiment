# 第二阶段G：核心恢复和稳定性验收

<!-- doc-locale: zh-CN -->
> [English](PHASE2G-RECOVERY-STABILITY.md) | **简体中文**

## 恢复合约

`device-core-recovery.sh` 在真实设备上测试三个受信任的长期运行服务。它在应用程序活跃时拒绝运行，只解析固定的systemd单元名称，并仅将`SIGKILL`发送给每个单元的主要进程。它验证一个新的PID，一个增加的`NRestarts`计数器（systemd执行失败重启），保留无关服务的PID，所有三个Unix套接字以及实时appd ping/list请求。

compositor 失败的情况也需要替换 `BindsTo` 系统 Shell 进程，并重新连接到新的私有 Wayland 套接字。恢复仅因为 systemd 报告 `active` 并不被接受。

## 24小时监控

`device-stability-monitor.sh` 默认值为86,400秒，采样间隔为60秒。它仅限root用户使用，并仅允许输出低于
`/run/cardputerzero-stability`，该版本是基于RAM的，在V0.6镜像中。每次运行都会创建一个唯一的目录，并且不会删除之前的結果。

每项样本记录：

- 墙钟纪元和单调运行时间
- `ActiveState`、`SubState`、`MainPID`、`NRestarts`和`MemoryCurrent`用于
compositor、System Shell和appd；
- 一个认证过的appd ping;
- 分页、方案检查的应用列表和正在运行的前台应用数量；
- 私有的 Wayland、appd 控制和运行时代理服务套接字。
- 从`/sys/block/mmcblk0/stat`累计写入的SD卡扇区数，采样到RAM中，而不造成测量SD写入。

意外重启、缺失进程/套接字、失败的ping和 compositor/Shell 的内存使用超过32 MiB 或 appd 的内存使用超过24 MiB 是失败。应用程序列表查询失败、无效分页或任何运行中的应用程序也会使空闲运行失败。最终样本可能分别从空闲基线增长最多4 MiB、2 MiB 和4 MiB。结果包含 `samples.tsv`、`summary.env`、`status`，仅在失败时包含 `failures.log`。`block-io.tsv` 包含原始写入计数器，`foreground.tsv` 包含每次样本中的运行应用程序数量。默认空闲接受允许在整个运行过程中最多64 MiB的SD写入；可以通过第四个参数设置更严格的字节限制。

应用程序暂态单元还声明与命名稳定性接受服务的systemd冲突。一旦该平台版本部署，启动应用程序会停止监控，并且其退出陷阱会写入`FAILED`，即使应用程序在两个60秒样本之间完全运行也是如此。这种硬互锁补充而不是替代独立验证的`foreground.tsv`时间线。

两个工具都作为显式诊断项复制到镜像的
`/usr/libexec/cardputerzero/` 下；均未启用为开机服务。

已完成的证据不单单因为设备写入了`PASS`而被接受。在检索后，`verify-stability-evidence.sh`独立解析文件而不引用`summary.env`。它需要一个精确的字段集，一个块-I/O 行和每个周期恰好三个不同的核心服务行，请求持续时间的单调墙时间和运行时间覆盖，常数服务 PIDs/重启次数，内存限制/增长，摘要到原始数据的一致性以及 SD 写入限制。前台、块-I/O 和服务时间线必须完全一致，每个前台计数必须为零。当可选存储服务存在时，它必须在每个周期中出现，并且具有稳定的 PID/重启次数，并且必须保持在其内存限制以下。未知/重复字段、非空失败日志、缺少样本、过大的间隙或伪造的摘要会失败。其变异测试在`make check`中运行。

## V0.6 验证

恢复测试于2026年7月31日通过，无需重启或刷新镜像：

- appd 进程 ID `8249 -> 9628`, `NRestarts 0 -> 1`;
- System Shell PID `8351 -> 9651`, `NRestarts 0 -> 1`;
- compositor 进程 ID `8334 -> 9679`, `NRestarts 0 -> 1`;
- compositor 替换导致 Shell 重新绑定为 PID `9695`，而 appd 保持不变；
- 所有控制路径通过了测试，4K Camera2 在恢复后显示了 Home。

一个15秒、间隔3秒的监控烟雾运行完成，没有任何失败。Compositor 内存保持在 7,487,488 字节，Shell 从 1,073,152 变到 1,323,008 字节，而 appd 从 1,081,344 变到 1,073,152 字节。

第一次正式的24小时接受运行始于大约2026年7月31日05:25 CST，但其RAM支持的结果在设备重启时丢失。后来在14:19进行的运行也在LCD冷启动诊断过程中被无效化。当前运行始于2026年7月31日20:42:15 CST，作为临时单元`cardputerzero-stability-acceptance.service`；在21:58只读检查时，它和 compositor、System Shell 和 appd 都是`active/running`状态，没有重启。该运行在2026年8月1日00:26 CST被所有者的请求开发者模式密钥和Neon Snake包安装所显式无效化。其最终状态不能被接受为闲置证据。它在00:43 CST被停止，并且其`FAILED`存档被保留到主机`target/device-evidence`目录下，SHA-256哈希值为`88ecc5d2414710d3dc60ce63dbbd046e7bc0e010bccc29f609409edaba23c2bf`.

Neon Snake 然后被停止并且开发者模式被禁用。禁用该模式前，重启了 appd。在 `/run/cardputerzero-stability/acceptance/20260731T164319Z-9619` 重启了一个替换基线。替换运行于 2026-08-01 00:43:19 CST 开始。其第一个样本记录了 compositor 进程 ID `909`，System Shell 进程 ID `926`，appd 进程 ID `9465` 和存储进程 ID `8480`，所有进程均处于活动状态且未重启；4K Camera2 显示 Home。然而，在 00:49 进行只读应用列表查询时发现 Neon Snake 仍然标记为运行状态，因此该运行也是无效空闲证据。替换运行未重启核心服务并保留为 `target/device-evidence/invalid-20260731T164319Z-9619.tar.gz`，SHA-256 `9608197a5520cea281054a3ede9d0047362c0cb70135fbe915d643a93120d8fd`.

然后使用SHA-256 `5219e5b33982c598914378c127694fc491f2186b0bca0df952b639dbb3b42797` 热部署了前台感知监控器。硬件烟雾测试运行通过了三次零前台样本，零失败，零SD写入，并且核心服务PID未改变。正式替换于2026-08-01 01:02:28 CST在 `/run/cardputerzero-stability/acceptance/20260731T170228Z-10620` 开始。其第一个样本记录了零运行中的应用程序和 compositor PID `909`，System Shell PID `926`，appd PID `9465` 和 stored PID `8480`，所有这些都处于活动状态且没有重启；4K Camera2 在启动后显示空闲的Home。路线图保持开放状态直至大约2026-08-02 01:02 CST，那时必须完整且不间断地恢复目录并独立验证，之后才能进行任何平台部署、应用程序启动或重启。
