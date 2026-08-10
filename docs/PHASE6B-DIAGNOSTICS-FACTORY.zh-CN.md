# 第6B阶段：隐私保护诊断和工厂验收

<!-- doc-locale: zh-CN -->
> [English](PHASE6B-DIAGNOSTICS-FACTORY.md) | **简体中文**

## 策略

CardputerZero诊断和支持工具不会上传遥测数据。
诊断收集是一个本地的、显式的根操作，所有生成的文件都保持在`/run`之下，因此它们是RAM支持的，并在重启后消失。
支持工具不会联系网络端点，创建持久标识符或修改应用程序状态。单独同意的Store每周计数合同在`STORE-METRICS-V1.md`中不能读取或包含这些捆绑包。

默认支持包不包含：

- 应用程序ID、包、私人存储和文档门户内容；
- Wi-Fi 配置文件和 SSIDs，IP 地址和 MAC 地址；
- SSH密钥、主机名、机器ID、启动ID和硬件序列号；
- 原始内核日志和服务日志。

它包括OS/内核版本、非标识的V0.6硬件存在情况、允许列表中的服务状态和退出状态、内存计数器、受保护的挂载属性以及SD写入计数器的聚合值。这些信息足以区分缺失设备、失败单元、内存压力、可写根文件系统回退或SD写入异常，而无需收集用户内容。

从恢复控制台或SSH会话生成默认捆绑包：

```sh
sudo /usr/libexec/cardputerzero/device-support-bundle
```

该命令打印出唯一的根目录 `tar.gz` 路径以下
`/run/cardputerzero-support`. 它还保留了未打包的源代码以供本地检查，并不会自动上传。

原始日志可能具有诊断必要性，但服务消息可能包含应用ID、URL、路径或用户输入的文本。因此它们需要一个单独的显式操作：

```sh
sudo /usr/libexec/cardputerzero/device-support-bundle --include-journal
```

那个捆绑包记录了`journal_included=1`, 命名了文件`sensitive-journal.txt`并带有检查警告。操作员必须在传输前获取用户同意并检查它。图像中故意没有上传命令。

## 工厂大门

`device-factory-acceptance` 是未配置的 V0.6 发行版门。它是只读的：它不捕获摄像头帧，不播放或录制音频，不驱动 GPIO，不传输 LoRa，不挂载文件系统，不重启服务或更改设备模式。在三分区产品镜像首次启动后运行它，并在启用开发人员/恢复模式或安装用户应用程序之前运行。

```sh
sudo /usr/libexec/cardputerzero/device-factory-acceptance
```

每次调用会在 `/run/cardputerzero-factory` 下创建一个隔离的目录，包含：

- `hardware-smoke.txt`, 现有的CM0/LCD/键盘/音频/电池基线；
- `checks.tsv`, 每个工厂不变量的一行PASS/FAIL;
- `summary.env`, 架构, 失败次数, 可用内存和当前SD写入计数器;
- `status`，包含`PASS`或失败次数。

在重启前检索完整的报告目录，然后从匹配的源修订版中验证它，而不仅仅是信任`status`：

```sh
./scripts/verify-device-acceptance-evidence.sh factory PATH_TO_RUN_DIR
```

主机验证器需要完整的固定出厂检查集，拒绝符号化或格式错误的证据，并交叉检查总结失败次数。嵌套硬件烟雾发出的警告仍然可见，但不会替代任何所需的出厂不变量。

门需要不可变的OverlayFS配置文件，一个标记为分区3的分区，该分区已扩展到SD卡的最终1 MiB，一个ext4文件系统已扩展到该分区，v1持久布局，干净的标准设备模式，活跃的核心服务无重启，所有代理套接字，精确的套接字所有者和模式，一个实时认证的应用程序ping和没有失败的systemd单元。外部可选的摄像头和LoRa外设仍保持在V0.6工厂门之外。

自动化仓库测试强制执行RAM-only输出根目录，不允许删除和只读出厂行为，支持包数据排除，明确的日志许可，缺少上传命令和两个工具的图像安装。最终接受仍需要在刷写的Phase 6A候选版上运行出厂门，并完成物理键盘/显示检查。
