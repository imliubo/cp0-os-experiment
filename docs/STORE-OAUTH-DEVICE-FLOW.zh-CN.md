# Store OAuth 设备授权流程

<!-- doc-locale: zh-CN -->
> [English](STORE-OAUTH-DEVICE-FLOW.md) | **简体中文**

S5I 实现了由 `cp0ctl store submit` 使用的开发者授权路径。它遵循 OAuth 设备授权授予的轮询模型，同时保持控制平面现有 PostgreSQL 身份、RBAC、2FA、审计和出箱边界。它不是 CardputerZero 设备镜像的一部分。

## 协议

1. `cp0ctl` 向 `POST /oauth/device/code` 发送固定客户端 `cp0ctl` 和范围 `store.submit`。
2. 服务返回一个10分钟设备码、一个格式为`XXXX-XXXX-XXXX`的48位用户码、验证URI和一个5秒的轮询间隔。
3. 已认证的开发者将用户代码提交到`POST /oauth/device/authorize`，并附带`approve`或`deny`以及一个幂等键。审批需要一个有效的持有者身份、角色`owner`或`developer`、确切的`store.submit`范围以及当前启用的2FA。
4. `cp0ctl` 投票 `POST /oauth/token`. 待授权返回
   `authorization-pending`；早期投票结果 `slow-down` 并将间隔增加5秒，最多增加到30秒。
5. 一次成功的交换返回一个仅针对`store.submit`的15分钟授权令牌。拒绝、过期、已被使用过的代码或不再符合条件的成员无法颁发令牌。

所有OAuth响应设置`Cache-Control: no-store`和`Pragma: no-cache`。
稳定的故障代码是`authorization-pending`, `slow-down`, `access-denied`,
和`expired-token`，匹配已经由`cp0ctl`处理的情况。

## 存储和并发

- 设备代码和颁发的访问令牌仅以SHA-256摘要形式存储。它们的明文值仅存在于单个成功的HTTP响应中。
- 短用户代码被存储，因为认证批准端点必须找到其待处理请求。它是唯一的、大写的、十分钟内有效，并且没有活跃开发会话无法授权任何内容。
- 交易级别下的活跃待处理记录创建上限为10,000条。生产入站请求在转发前必须根据可信客户端和网络信号进行速率限制。
- 审批和拒绝使用SERIALIZABLE事务、行级锁、精确的幂等重放和原子审计/出箱写入。
- Exchange 锁定授权行。唯一的成功过渡是
`approved -> consumed`, 在同一事务中插入访问令牌及其审核/出箱记录。因此并发的交换仅发出一个令牌。
- PostgreSQL 触发器拒绝删除、身份变更、反向轮询时钟、状态回滚、令牌替换和令牌未撤销。

审计和出箱载荷包含设备码摘要、成员 ID、决策和范围。它们从不包含设备码或访问令牌。

## 验证

PostgreSQL 17 接收套件涵盖了损坏客户端、待处理和早期轮询、批准和拒绝、精确重放、幂等冲突、缺失、过期和被撤销的审批人令牌、角色/范围/双因素拒绝、授权过期、并发一次性交换、使用发布的令牌针对提交 API、后期令牌撤销、实时成员角色和双因素认证更改、明文秘密排除以及直接 SQL 状态机绕过尝试。

```sh
CP0_STORE_TEST_DATABASE_URL=postgres://... make store-control-db-check
```

S5K 现实施行有界 Team 读取和 Owner 角色变更，需要五分钟的 MFA 新鲜度要求。账户注册/绑定、密码辅助登录、双因素认证注册和恢复、邀请、成员移除、门户会话端点、审阅者 SSO、滥用控制以及生产身份恢复仍作为独立的 Identity/Teams 工作。当前的审批流程假设该系统已经发放了用于授权设备的短期有效开发者凭据。
