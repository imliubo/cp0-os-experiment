# Store 工作人员服务器

<!-- doc-locale: zh-CN -->
> [English](STORE-WORKFORCE-SERVER.md) | **简体中文**

`cp0-store-workforce-server` 是 Review Console 和 Store 操作的浏览器身份验证 BFF。它独立于 Store 控制服务器和开发者门户运行。Review 和 操作 共享 实现 但永不共享 原生、cookies、会话、回调 URI 或 Store 控制令牌。

## HTTP边界

审核路径是 `/review/auth/login`, `/review/auth/callback`, `/review/v1/session`,
`/review/v1/token` 和 `/review/v1/session:logout`. 操作使用相同的路径后缀在 `/operations` 下。登录创建一个十分钟的 OIDC 状态/nonce/PKCE 交易。回调需要新鲜的 MFA，仅解决一个预配置的工作团队链接，并创建一个十五分钟的空闲/八小时绝对不透明会话。BFF 从不创建审核员或操作员。

会话响应包括特定观众的主要实体、角色、截止日期、资源版本和CSRF令牌。审核请求只能请求`store.review`。操作请求恰好一个授权的`store.editorial`或`store.moderation`范围。令牌在五分钟内过期，并绑定到活动会话、身份链接和主要实体。注销是幂等操作，并撤销同一数据库事务中的所有绑定令牌。

Cookie-认证的变更需要浏览器生成的确切`Origin`、`Sec-Fetch-Site: same-origin`，会话CSRF值和16-128字节的`Idempotency-Key`。审核使用`__Host-cp0_review`；操作使用`__Host-cp0_operations`。两者都是没有`Domain`属性的`Secure`、`HttpOnly`、`SameSite=Strict`、`Path=/` cookie。响应是受限的`no-store`，不会在审计记录中暴露OIDC主题、提供程序令牌、cookie、CSRF值或Store的授权令牌。

## 配置

`CP0_WORKFORCE_CONFIG` 指向严格 JSON，大小不超过 128 KiB：

```json
{
  "review": {
    "allowed_origin": "https://review.cardputerzero.dev",
    "post_login_uri": "https://review.cardputerzero.dev/queue",
    "providers": [{
      "key": "primary",
      "label": "Workforce identity",
      "issuer": "https://identity.example.com",
      "authorization_endpoint": "https://identity.example.com/authorize",
      "token_endpoint": "https://identity.example.com/token",
      "client_id": "cardputerzero-review",
      "client_secret_env": "CP0_OIDC_REVIEW_SECRET",
      "redirect_uri": "https://review.cardputerzero.dev/review/auth/callback",
      "accepted_signing_algorithms": ["RS256"],
      "accepted_mfa_acr": ["urn:example:acr:mfa"],
      "clock_skew_seconds": 60,
      "jwks": { "keys": [] }
    }]
  },
  "operations": {
    "allowed_origin": "https://operations.cardputerzero.dev",
    "post_login_uri": "https://operations.cardputerzero.dev/console",
    "providers": [{
      "key": "primary",
      "label": "Workforce identity",
      "issuer": "https://identity.example.com",
      "authorization_endpoint": "https://identity.example.com/authorize",
      "token_endpoint": "https://identity.example.com/token",
      "client_id": "cardputerzero-operations",
      "client_secret_env": "CP0_OIDC_OPERATIONS_SECRET",
      "redirect_uri": "https://operations.cardputerzero.dev/operations/auth/callback",
      "accepted_signing_algorithms": ["RS256"],
      "accepted_mfa_acr": ["urn:example:acr:mfa"],
      "clock_skew_seconds": 60,
      "jwks": { "keys": [] }
    }]
  }
}
```

上述的 inline JWKS 值是占位符；生产启动需要经过审核的公钥。
两个来源必须是不同的裸 HTTPS 来源。每个重定向 URI 必须恰好等于匹配的来源加上其固定的回调路径。OIDC 客户端密钥仅通过配置中引用的有效大写环境变量加载。

所需的环境变量是 `CP0_WORKFORCE_DATABASE_URL`, `CP0_WORKFORCE_CONFIG`, `CP0_WORKFORCE_CSRF_KEY`, `CP0_WORKFORCE_NONCE_KEY`, `CP0_WORKFORCE_PKCE_KEY`, `CP0_WORKFORCE_SUBJECT_KEY` 和 `CP0_WORKFORCE_CONTROL_TOKEN_KEY`。五个密钥是不同的32字节未填充的base64url值。监听器默认为 `127.0.0.1:8791`。需要 `CP0_WORKFORCE_ALLOW_NON_LOOPBACK=1` 和审核过的外部TLS反向代理来进行非回环绑定。

## 前端适配器和验证

审查控制台和商店操作获取 BFF 会话/令牌/注销路由使用 `credentials: include`。简短的商店授权令牌仅保留在内存中，在过期前刷新并验证精确的受众和范围。商店控制请求保持 `credentials: omit`。操作为编辑和审核范围保留单独的内存条目。

被忽略的 PostgreSQL 接受测试使用假的 OIDC 提供商和一个新的 Store 数据库：

```sh
CP0_STORE_TEST_DATABASE_URL='postgresql://...' make store-control-db-check
```

它验证了观众/cookie/callback分离，原始主题的缺失，精确的令牌重放，作用域授权，控制API使用，幂等冲突，注销，链接/主体撤销和无密钥审计证据。生产IdP/JWKS部署、受管理的秘密存储和实时访问撤销仍然是部署关卡。
