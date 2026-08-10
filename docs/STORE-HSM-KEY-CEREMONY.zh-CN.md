# Store HSM 密钥仪式证据

<!-- doc-locale: zh-CN -->
> [English](STORE-HSM-KEY-CEREMONY.md) | **简体中文**

生产 Store 签名密钥必须在 HSM 边界内生成、轮换、撤销和销毁。本文件定义了提供者中立的证据合同。它不授权生产仪式或选择 HSM 供应商、法定保留期或生产操作员。

## 角色和分离

每条证据记录包含两个到四个不同的不透明操作员ID，并且必须包括一个`key-custodian`和一个`security-officer`。一个`release-operator`或独立的`auditor`也可以批准，但不能替代任何一个必需的角色。门户、审核控制台和发布者服务身份不是仪式参与者。

证据中仅包含公钥ID和SHA-256承诺。私钥字节、恢复份额、PIN、HSM凭据、个人姓名、电子邮件地址和自由格式备注均在方案之外，因此被拒绝。详细的HSM日志保留在受限审计系统中，并受`hsm_attestation_sha256`约束。

## 操作

- `generate` 创建一个新的公钥ID并绑定签名的OS信任更新。它没有之前的目录序列。
- `rotate` 绑定不同的旧/新密钥ID，严格递增的目录序列过渡和建立重叠的信任更新。
- `revoke` 移除已泄露的密钥。若存在替代密钥，它必须绑定到更高的 Catalog sequence；
  若不存在，则不得记录后续 sequence，Store 必须保持不可用，直到发布新的信任更新。
- `destroy` 只允许在退休后。它绑定一个旧密钥，不绑定新密钥或信任更新，并且Catalog序列不变。

一次仪式被限制在八小时内。`approved` 表示证据通过了本地结构政策审查，但这并不意味着HSM、舰队部署、透明见证或CDN推广独立进行了审计。`aborted` 记录使用相同的严格格式，因此失败的仪式不能变成无界限的事故记录。

## 验证

```sh
./scripts/verify-store-key-ceremony.sh EVIDENCE.json
```

验证器拒绝接收符号、空或大于32-KiB的输入，未知字段，损坏的ID/摘要，重复的演员，缺少必需的角色，重复的密钥，非递增序列，无效的操作特定空值和持续时间超过八小时的仪式。方案是`schemas/store-key-ceremony-v1.schema.json`.

自动化变异覆盖率运行在`make check`。生产完成仍需经过批准的HSM设计，实际共识执行，签名OS信任根部署，离线设备组，透明/CDN验证以及保留证据的独立审核。
