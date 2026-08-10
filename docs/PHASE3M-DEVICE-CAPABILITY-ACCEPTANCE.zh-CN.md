# Phase 3M: 设备能力接受

<!-- doc-locale: zh-CN -->
> [English](PHASE3M-DEVICE-CAPABILITY-ACCEPTANCE.md) | **简体中文**

Phase 3 设备门使用真实的SDK应用程序而不是特权测试请求。`dev.cardputerzero.acceptance`和`dev.cardputerzero.isolation`是开发者签名的接受包，仅安装在专用测试设备上。它们不包含在量产镜像中。

## 信任路径

每次测试的操作都遵循生产路径：

```text
acceptance WASM
  -> public Rust SDK host call
  -> sandboxed App Runtime
  -> appd SO_PEERCRED, active-cgroup and manifest authentication
  -> permission decision
  -> restricted capability service
  -> V0.6 hardware or app-private storage
```

设备测试框架绝不会打开 Runtime capability broker socket。它只重置两个专用 App 的
权限，每次启动一个前台 App，并处理可信 appd prompt。每个 probe 通过自身的 SDK 私有
storage identity 保存有界查询结果；root 测试框架在 inode 发生变化后读取由专属
`cp0-storage` 身份拥有的结果文件。通知仍对操作员可见，但不会作为自动化通道，因为
System Shell 也是通知消费者。因此，原始 root broker 请求无法伪造通过的 capability 结果。

## 覆盖率

主探针执行以下操作：

- 填充其1 MiB私有配额的128个最大值，验证下一个写入被拒绝，清理临时值并留下一个标记；
- 验证标记在进程停止/启动后仍存在；
- 请求独立地播放和捕获，并报告捕获是否返回了非零样本；
- 读取、反转、重新读取、恢复并验证仅逻辑 `grove-function` GPIO；
- 在连续拒绝决策后报告拒绝结果，然后在明确允许决策后重新执行。

隔离 probe 以自身身份请求主 App 的 marker key，并且必须收到 `not found`。测试框架还会
检查准确的 sysfs 和私有存储 Owner/mode，并验证登录用户不能绕过 `cp0-gpiod`。验收 App
运行时，它还读取真实 transient unit 属性，并要求固定的 60% CPU quota、CPU weight 50、
Manifest 规定的 16 MiB 内存上限、零 swap 和 32 task 上限。

## 构建和配置

使用仅接受开发人员密钥构建可重复构建的包：将开发人员密钥存储在忽略的 `target/` 树下：

```sh
./scripts/build-device-capability-apps.sh
```

该命令打印开发者密钥 ID 并生成公钥以及两个 `.capp` 文件在 `target/device-capability-acceptance` 中。在专用设备上，将公钥放置在精确打印的根所有者信任路径中，启用开发者模式，使用 `sudo cp0ctl install` 安装两个包，然后再次禁用开发者模式。包安装记录稳定的应用程序 UID；关闭开发者模式不会扩展它们的运行时权限。

## 运行并保留证据

在核心稳定性运行活动时，请勿运行能力接受。框架强制执行这一点，并仅写入RAM支持的证据：

```sh
sudo CP0_AUDIO_OBSERVED=yes \
  /usr/libexec/cardputerzero/device-capability-acceptance --full
```

仅当操作员实际听到限定的测试音时设置 `CP0_AUDIO_OBSERVED=yes`。没有该观察的成功的PCM写入仍然是一种警告。即使代理和服务音频捕获完成，全零捕获也仍然是一种警告。

在重新启动之前，从报告的目录中检索 `status`, `checks.tsv` 和 `summary.env`. 为了证明SD卡支持的持久性，请正常重新启动并运行：

```sh
sudo /usr/libexec/cardputerzero/device-capability-acceptance \
  --persistence-only
```

第二次运行必须报告`storage=persist-ok`, 保留权限决定而无需新的提示, 并记录不同的内核`boot_id`. 在任何进一步重启之前, 获取那个RAM支持的结果。

验证两个检索到的目录一起：

```sh
./scripts/verify-device-acceptance-evidence.sh capability \
  PATH_TO_FULL_RUN PATH_TO_PERSISTENCE_RUN
```

主机验证器需要完整的能力/资源/sysfs/存储检查集，验证有界私有存储结果文件，并证明持续运行有更晚的结束时间和不同的内核启动 ID。

完整的运行故意保留专用标记、结果和权限决策，以便重启检查。它从不重启平台服务、重启、挂载、格式化或读取另一个应用程序的结果或标记文件。
