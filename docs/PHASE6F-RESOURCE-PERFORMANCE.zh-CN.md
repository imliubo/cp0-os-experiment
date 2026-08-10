# 第6F阶段：资源和性能验收

<!-- doc-locale: zh-CN -->
> [English](PHASE6F-RESOURCE-PERFORMANCE.md) | **简体中文**

第6F阶段将CM0性能预算转换为强制运行时限制和可重复的本地设备报告。它不会放松任何应用程序隔离边界，其性能收集器永远不会上传。单独的可选Store每周计数器由`STORE-METRICS-V1.md`定义并受限。

## 强制限制

每个应用程序瞬态单元现在除了其清单内存控制组之外，还具有固定的`CPUQuota=60%`和`CPUWeight=50`，并且没有交换空间和任务限制。在单核CM0上，这可以防止一个旋转或被破坏的Runtime消耗整个处理器，同时保留足够的前台容量用于正常的SDK UI工作。CPU配额是平台安全的上限，而不是由应用程序控制的清单字段。

可信 Runtime 使用 `CLOCK_MONOTONIC`，每秒最多接受 30 次 display submit。两次提交间隔不足 33,333,334 ns 时返回已有 SDK `ResourceLimit`。双缓冲、RGB565 验证和精确 damage rectangle 保持不变。compositor 同样受 32 MiB 内存上限、零 swap 和 32 task 限制约束，与现有 24 小时 monitor contract 一致。

## 设备网关

在24小时稳定性结果完成并获取后，才运行性能门：

```sh
sudo /usr/libexec/cardputerzero/device-performance-acceptance
```

如果Phase 6F服务是通过热部署脚本安装的，重启设备一次并在运行网关之前等待Home。热部署在当前启动过程中较晚重启System Shell，所以其单调激活时间戳不是有效的启动就绪证据。部署后的重启也证明了安装单元限制能存活一个正常启动过程。

默认运行每五秒采样空闲的Home屏幕60秒。在稳定单元或任何应用程序活动时，它不会运行。每次调用会在`/run/cardputerzero-performance`下方写入一个新的根目录只读目录，包含`checks.tsv`、`samples.tsv`、`services.tsv`、`summary.env`和`status`；它从不删除早期的证据。

V0.6 发行阈值是：

- systemd 启动完成和 System Shell 激活不得晚于 35 秒；
- 在空闲采样期间使用最多180 MiB，并至少有200 MiB可用；
- compositor, Shell 和 appd 分别在 32/32/24 MiB 内；
- 所有三个核心服务保持活动状态，PID 和重启次数未发生变化；
- 将这三个服务的空闲CPU汇总起来，不超过10%；
- 短闲暇样本期间不超过 1 MiB 的 SD 写入。

1 MiB 检测捕捉即时的持续写入回退；权威的 写放大部分阈值 仍然是独立的 24 小时 稳定性运行中的 64 MiB 限制。

脚本记录了BQ27220电压、带符号的电池电流以及可用时的估计电池侧功率值。该值仅作参考：在USB为电路板供电时，电池电流并非总设备功率。产品功率声明仍需要在定义的亮度、网络和工作负载条件下使用内置校准的USB功率计。

从运行目录中检索完整路径并在主机上独立验证它：

```sh
./scripts/verify-device-acceptance-evidence.sh performance PATH_TO_RUN_DIR
```

验证器解析封闭摘要字段集，并从`samples.tsv`中重新计算持续时间、内存极值、电池样本平均值和SD字节。它还检查所有三个服务PID/重启连续性、内存上限和CPU差异从`services.tsv`。设备写入的`PASS`中如果有更改阈值或不一致的原始样本将被拒绝。

## 当前基线

在V0.6设备上的只读采样在实现前测量到systemd完成耗时27.939秒，Shell激活耗时27.790秒，大约使用了164 MiB空闲内存， compositor/Shell/appd 分别使用了7.4/1.7/1.8 MiB。这些值定义了余量而不是被复制为精确的通过标准。正式证据直到完成活跃的24小时运行并更新平台后才能确定，且不会使平台失效。
