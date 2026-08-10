# 第6E阶段：内部安全验证基线

<!-- doc-locale: zh-CN -->
> [English](PHASE6E-SECURITY-VALIDATION.md) | **简体中文**

## 交付范围

第6阶段将现有的组件级安全测试转化为明确的系统安全基线。`docs/THREAT-MODEL.md` 记录资产、攻击者、信任边界、控制映射和生产发布障碍。ADR 0006 定义了 dm-verity、RAUC A/B 和 U-Boot/FIT 在什么条件下会增加真正的安全功能，而不是仅仅增加新的启动复杂性。

这一阶段不声明验证启动、加密数据、完成的硬件故障注入或独立审核。

## fuzz 目标

单独的 `fuzz/` 工作区将 libFuzzer 和 sanitizer 的依赖项保留在产品二进制文件之外。五个目标测试了所有高风险序列化入口点。

| 目标 | 覆盖输入 |
| --- | --- |
| `manifest` | 严格的表现形式 JSON 和语义验证 |
| `package` | 原始和结构变异的`.capp` v1，标准重新编码和签名 |
| `store_protocol` | 签名目录JSON加上终止/未终止的Store IPC帧 |
| `appd_control` | 终止/未终止的 appd 请求和响应帧 |
| `recovery_backup` | 内存中的 `CP0 backup v1` 头部、条目和负载解析 |

`cp0-recovery` 只为测试或 `fuzzing` 功能暴露其字节切片验证器。产品构建继续只暴露基于文件的备份、验证和恢复操作。

在产品工作空间依赖图之外安装主机工具，类型检查每个目标，然后运行有界局部战役：

```sh
cargo +nightly install cargo-fuzz --locked --root target/fuzz-tools
make fuzz-check
./scripts/fuzz-smoke.sh 30
```

烟雾运行程序应用64 KiB输入限制，每个输入五秒超时，并使用AddressSanitizer和1536 MiB主机RSS限制。它是回归门，而不是长期模糊测试的替代品。计划的CI工作流程每运行一次目标30秒，并保留崩溃 artifacts。

## 接受边界

第6E阶段内部验收需要所有目标构建，本地除错器无崩溃/超时/OOM，`make check`，工作区Clippy和干净的差异。任何崩溃将在修复前被最小化为永久回退输入。

外部环节仍未关闭：独立审核方必须获得威胁模型、固定构建输入、fuzz 语料、镜像门禁和
硬件测试结果。审核发现必须按严重程度处理，并由产品负责人明确解决或接受；本仓库不能
自行认证该步骤。
