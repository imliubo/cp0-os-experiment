# Store 审核 v1

<!-- doc-locale: zh-CN -->
> [English](STORE-MODERATION-V1.md) | **简体中文**

状态：工程预览；生产强制执行已禁用。

本合同定义了内容报告、开发者通知、申诉和安全响应截止日期的隐私边界和事务状态机。它不授权自动删除。产品、法律、安全和安全所有者必须在启用生产入口之前批准政策词汇、截止日期、保留期、申诉窗口和双人执行程序。

## 隐私边界

`POST /reports/v1/content` 接受恰好：

- `release_id`, `app_id`, 和精确语义 `version`;
- 一个固定的理由：`malware`, `privacy`, `fraud`, `harmful-content`, `age-rating`, 或 `other`.

该服务拒绝未知字段，并不接受自由文本、联系信息、账户标识符、设备标识符、事件时间戳、IP地址、User-Agent、日志、截屏或附件。应用程序不读取或持久化除`Content-Type`和一个随机`Idempotency-Key`之外的网络头部信息，后者在24小时后SHA-256摘要失效。生产级入口不得向应用程序日志或转发头部添加客户端IP或指纹数据。

报告仅接受与批准的提交和已发布的不可变 Store 包 artifact 的 App ID 和版本匹配的发布。响应为 `202 Accepted`，仅暴露随机报告 ID、状态、SLA 类别、截止日期和资源版本。精确重试返回相同的报告；使用另一个主体重用密钥会失败。

## 暂定SLA

这些常量存在是为了使队列排序和超时行为可测试。它们不是生产承诺：

| 类别 | 原因 | 承认 | 解决或上报 |
| --- | --- | ---: | ---: |
| `security` | `malware`, `privacy` | 4小时 | 72小时 |
| `standard` | 其他所有原因 | 72小时 | 14天 |

未处理的报告按确认截止日期和稳定报告ID排序。
数据库只存储服务器接收时间。此参考服务最多接受10,000个未处理的报告；外部速率限制必须在无需持续用户指纹识别的情况下运行。

## 角色和转换

审核操作需要一个带有精确的`store.moderation`范围、激活的`admin`角色和2FA的实时操作员令牌。开发者读取和在`SERIALIZABLE`交易中重新审核实时团队所有权、角色、令牌范围、撤销和2FA。

```text
report: submitted -> closed-no-action
                  -> notice-issued -> closed-after-appeal
                  -> security-escalated

notice: open -> appealed -> resolved-accepted | resolved-upheld
appeal: pending -> accepted | upheld
```

操作决定只包含一个固定处置和一到四个固定原因代码。`developer-notice` 创建不可变的
Team 范围通知；`security-escalation` 向独立的运营事件系统发出 outbox 请求。两种转换
都不会改变 Release 状态或 Catalog 发布结果。

一个团队的所有者或开发者可以在初步的14天窗口内创建一个申诉，并使用一个固定的理由：`identity-mismatch`, `policy-misapplied`，`remediated`，或`other`。独立的操作员决定解决申诉。每次写操作都有一个强ETag，精确的幂等重放，只读修订，审计事件和出箱事件在一个事务中。

## API 接口

- `POST /reports/v1/content` - 隐私最小化的公共摄入；
- `GET /v1/moderation/reports` - 有界操作符SLA队列；
- `POST /v1/moderation/reports/{report_id}:decide` - 结构化分诊;
- `GET /v1/apps/{app_id}/moderation-notices` - 团队范围的通知；
- `POST /v1/moderation/notices/{notice_id}:appeal` - 一个开发者申诉；
- `POST /v1/moderation/appeals/{appeal_id}:decide` - 操作符解析。

量产完成仍需要批准的政策文本、滥用控制、保留擦除、通知交付所有权、安全值班集成、双人执行、可逆目录抑制，以及涵盖申诉逆转和紧急移除的演练。
