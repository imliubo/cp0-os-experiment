# V0.6 设备性能和架构审核

<!-- doc-locale: zh-CN -->
> [English](DEVICE-PERFORMANCE-ARCHITECTURE-AUDIT-20260806.md) | **简体中文**

## 测试身份

- 日期: 2026-08-06, 亚洲/上海
- 设备: CardputerZero V0.6, Raspberry Pi CM0, 512 MiB RAM
- 显示: 320x170 ST7789 LCD
- Camera: Raspberry Pi IMX219
- 源基线：`5f4ea63`（摄像头调度修复`d603bdc`）
- 镜像 profile：量产布局，使用隔离的 hardware-debug access profile
- 根布局：只读 `mmcblk0p2` 下根加上一个 64 MiB 易失性覆盖层
- 持久布局：`mmcblk0p3`，独立安装在
`/var/lib/cardputerzero` 和其他批准的状态路径上

在本次审计之前，核心二进制文件和八个内置应用程序已热部署到易失性覆盖层中。因此，正常重启将恢复较旧的底层根。以下结果验证了当前实现功能，但在实际CM0上，这不是一个干净启动的生产发布接受。

## 结果

当前的相机、画廊应用生命周期和闲置资源设计在单核CM0上可行。可见的相机UI每秒维持30.14个公共预览帧，以58毫秒的温暖速度完成1280x720的捕获，并在离开前台后释放相机管道。所有八个内置应用启动，发布一个表面并在测量范围内停止。120秒的闲置监控在没有前台应用、没有服务重启、没有SD写入和内存增长受限的情况下完成，并且其完整证据在主机上独立验证。

摄像头仍然是主导的工作负载。前台运行时，App Runtime 使用了 34.512% 的 CPU，camerad 使用了 52.565%，appd 使用了 4.051%，而 compositor 使用了 3.293%。这接近一个完整的 CM0 核心的总和，但测量的 UI 仍然达到了 30 FPS 的公开展示目标。架构应保留固定分辨率、单生产者管道，并且必须不添加并发高分辨率预览、第二个摄像头进程或后台预览工作。

后续审计发现隐藏 Camera 继续工作的两条路径。Camera 应用在 appd 拒绝后台请求后保留先前的 `Live` 状态，因此继续执行 33 毫秒预览重试和 1 毫秒输入轮询。另外，重启后的 System Shell 在显示 Home 时没有清除 appd 先前的前台身份。应用现在会进入显式 unavailable 状态，每 750 毫秒重试一次，inactive 时最多等待 250 毫秒，并保留 2 毫秒前台调度余量。System Shell 现在会失败关闭，除非能在分发 UI 事件前清除任何陈旧的前台身份。

## 摄像头管道

部署的流水线使用一个连续的 `rpicam-vid` 过程，并具有固定的产品配置：

- 传感器输出：1280x720 YUV420;
- 选定的传感器模式：1920x1080，10位打包；
- 纠正方法：180度旋转；
- 内部目标：40 FPS;
- 公开预览：320x170 RGB565，目标帧率为30 FPS；
- 照片：1280x720 JPEG，以及320x170 的画廊缩略图；
- 后台行为：在两秒内没有前台请求时停止摄像头进程。

测量设备结果：

| 操作 | 结果 |
| --- | ---: |
| 可见 Camera UI 的预览吞吐量 | 30.14 FPS |
| 冷启动管道到第一帧完成 | 1.345 s |
| 热管线 1280x720 拍照 | 58 ms |
| Camera 拍照并导入 Gallery | 312 ms |
| 返回 Home 后的管线 | 约 2 秒内停止 |

预览结果显示了相机运行时在其 compositor 表面活动期间读取的精确 320x170 RGB565 载荷字节：15.029 秒内共 453 帧。相同的激活生成的受信任截图显示了沉浸式的 Camera 表面处于 `LIVE` 状态。

本次运行的相机和JPEG证据几乎全是黑色的，因为镜头指向了一个黑暗的测试环境。协议大小、帧速率、捕获延迟、JPEG有效性以及UI渲染都得到了验证；曝光和主题质量没有作为通过的标准。

## 画廊管道

画廊保持在WASM应用边界内。它只接收密封的RGB565视图描述符，从未接收JPEG路径、存储键或原始文件系统访问权限。在照片ID46上，第一次Fit解码耗时115.0 ms。缓存的半部分、实际中心、实际左和实际右视图耗时4.7-4.8 ms，所有五个帧哈希值都不同。

物理键通过 compositor 座位注入并通过 `Z`、`X`、`C` 和 `F` 输入验证未修改。`Z` 和 `F` 选择上一个图像；`X` 和 `C` 选择下一个图像。两端循环。进入打开原始分辨率视图，缩放和平移改变渲染视窗，返回保留所选照片。

## 内置应用性能

每一行都完成并添加了`PASS`。表面就绪时间从控制请求开始测量到 compositor 观察到 App 表面为止。CPU 值是在活跃样本期间 CM0 核心的百分比。

| 应用 | 表面就绪 | 停止 | 应用CPU | 峰值应用内存 |
| --- | ---: | ---: | ---: | ---: |
| Hello Card | 480 ms | 200 ms | 0.172% | 10.05 MiB |
| Calculator | 483 ms | 201 ms | 0.028% | 9.88 MiB |
| Neon Snake | 480 ms | 223 ms | 5.615% | 10.49 MiB |
| Camera | 477 ms | 212 ms | 34.512% | 10.80 MiB |
| Gallery | 505 ms | 202 ms | 0.068% | 10.24 MiB |
| Media Controls | 454 ms | 203 ms | 0.655% | 9.87 MiB |
| Notes | 515 ms | 199 ms | 0.071% | 9.74 MiB |
| Stopwatch | 457 ms | 209 ms | 0.122% | 9.73 MiB |

在测量的应用启动/活跃/停止窗口期间，没有应用程序向SD卡写入数据。相机样本短暂使用了12 KiB的交换空间；在应用停止后，这部分交换空间仍然被分配，并且在随后的应用程序中没有增长。

## 空闲稳定性

主机独立验证了`target/device-evidence/stability/20260806T031828Z-21323/20260806T031828Z-21323`与`scripts/verify-stability-evidence.sh`.

- 请求时长：120秒
- 采样间隔：5秒
- 完整的timeline：25个块-I/O行，25个前台行和24个样本周期；
- 前台应用数量：每个周期为零；
- 服务重启次数：零；
- SD 写：0 字节；
- compositor 内存：7,622,656 至 7,884,800 字节；
- System Shell 内存：2,572,288 到 2,625,536 字节；
- appd 内存：1,024,000 至 1,269,760 字节。

一个早期的30秒Home样本测量了appd为0.059%，System Shell为0.676%，camerad为0.004% CPU。移除空闲App-catalog轮询将从前一个4.787%的观察值减少约80倍。

Camera 更新后又复现了一个独立的驻留后台忙循环。
在修复前，隐藏的Camera大约使用了3.4-4.1%的CPU，并使appd达到约2.9-3.6%；停止Camera使appd减少到约0.4%。中间的有界重试构建测量到Camera为1.03%，appd为0.72%。最终的30.41秒Home样本，包含Camera、Gallery和Calculator仍然驻留，测量结果为：

| 本地服务 | CPU |
| --- | ---: |
| Camera App Runtime | 0.791% |
| appd | 0.635% |
| System Shell | 0.669% |
| compositor | 0.648% |
| Gallery App Runtime | 0.070% |
| Calculator App Runtime | 0.029% |
| camerad | 0.003% |

正常F1 Home过渡和故意重启System Shell都撤销了Camera前台访问权限。在每种情况下，连续的`rpicam-vid`进程在大约两秒内退出，而Camera任务仍然驻留。通过Apps重新打开Camera恢复了一个可见的`LIVE`表面和30.14 FPS的节律。

## 隔离，访问和持久化数据

运行后只读审计未发现失败的systemd单元和服务。 compositor、System Shell、appd、camerad、storaged、stored 和 provisiond 均处于活动状态且未重启。App Runtime 保持临时状态，而 devd 在空闲时通过套接字激活。

前台摄像头权限现在在 Shell 进程重启时与可见 Shell 状态耦合。一个无法清除之前前台 appd 令牌的 Shell 在绘制 Home 时退出，而隐藏的 App 保留仅前台的能力。

应用程序包树由根拥有。私有应用程序数据是`0700 cp0-storage`，注册表是`0700 root:root`，`permissions.json`是`0600 root:root`。相机有持久的`camera.capture`和`photos.write`授权；相册有一个持久的`photos.read`授权。没有被拒绝的决定。共享的照片库大约占用了9.0 MiB，而持久分区大约有25.4 GiB可用。照片和受信任的截屏不受自动项数驱逐策略的影响。

此媒体是一个故意用于硬件调试的制品。它包含
`/etc/cardputerzero/hardware-debug-access` 和临时操作员账户的密码-sudo 策略。根账户本身仍然被锁定；sshd 报告了 `PermitRootLogin no`，并且有效转发被禁用。最终审计时开发者模式为 Off，而独立控制的拥有者 SSH  shell 为 On。生产镜像中不得出现任何硬件调试标记、sudo 策略、操作员账户或凭证。

## 视觉证据

受信任的屏幕截图和相机/相册证据保留在以下位置
`target/device-evidence/ui/20260806T025529Z`，
`target/device-evidence/camera-gallery-20260806`，
和
`target/device-evidence/ui/20260806T033051Z`。后续可见的相机和
重启Shell后的Home截图位于
`target/device-evidence/ui/20260806-camera-background-fix`。受信任的帧是精确的320x170截图，没有任何泄露的应用或任务内容。

## 释放门仍敞开

以下结果需要一个新的并刚刚烧录的生产镜像。它们不能通过易失性热部署关闭：

1. 验证挂载的生产根文件系统包含当前的System Shell、appd、Runtime、camerad、brokers、八个内置App和QA独立的udev规则。
2. 证明镜像不包含硬件调试标记、sudo策略、共享凭证、root访问或默认SSH监听器。
3. 执行一次正常重启，验证新的启动ID和LCD上的Home，并确认所有当前二进制文件和持久介质在重启后仍然存在。
4. 从清洁启动运行并独立验证工厂和官方Phase 6F的性能验收
5. 运行能力 `--full`，然后重启并运行 `--persistence-only`.
6. 完成六步 Store 刷新/恢复/升级/离线/过期接受序列，并独立验证所有证据。

关机、首次启动中断、量产 Developer Mode/Owner Shell 隔离、恢复介质、A/B/verity 和外部
电源测量仍是相互独立的硬件发布门禁。
