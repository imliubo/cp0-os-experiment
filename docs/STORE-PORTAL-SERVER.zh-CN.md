# Store 门户服务器

<!-- doc-locale: zh-CN -->
> [English](STORE-PORTAL-SERVER.md) | **简体中文**

`cp0-store-portal-server` 是浏览器面向的认证 BFF 开发者门户。它终止门户会话，执行外部 OpenID Connect 授权码 + PKCE 交换，并读取 Store 身份表。它是一个与独立进程。 `cp0-store-control-server`浏览器不接收 Store 控制令牌、上游令牌、刷新令牌、OIDC 主体或签名材料。

该切片实现了登录、回调、会话读取、MFA 级别提升、注销、外部身份链接以及团队邀请生命周期。

## HTTP边界

已实现的路由是：

| 路由 | 合约 |
| --- | --- |
| `GET /portal/auth/login?provider=...` | 开始一个十分钟的状态/nonce/PKCE 交易并重定向到一个配置好的提供商 |
| `GET /portal/auth/callback` | 消耗一次交易，创建或旋转不透明会话，并重定向到配置的Portal URI|
| `GET /portal/v1/session` | 返回有界账户、团队、过期时间、多因素认证新鲜度、CSRF 和会话版本数据 |
| `POST /portal/v1/session:step-up` | 开始一个幂等的同提供者多因素认证挑战 |
| `POST /portal/v1/session:logout` | 终止当前会话并失效cookie|
| `GET/POST /portal/v1/identity-links` | 列出主题自由的元数据或启动一个幂等的目标提供者链接挑战 |
| `POST /portal/v1/identity-links/{link_id}:remove` | 在新鲜MFA之后移除一个非最终链接并撤销由其认证的会话 |
| `GET/POST /portal/v1/teams/{team_id}/invitations` | 列出最新的100个邀请或创建一个所有者/MFA授权的邀请 |
| `POST /portal/v1/invitations/{invitation_id}:cancel` | 终止一个待处理的邀请并清除未交付的秘密材料 |
| `POST /portal/v1/invitations:inspect` | 只返回有效的待处理令牌的团队名称、遮罩电子邮件、角色和过期日期 |
| `POST /portal/v1/invitations:accept` | 绑定验证过的会话邮箱，原子性创建会员，并旋转会话 |

会话 cookie 是 `__Host-cp0_portal`，带有 `Secure`，`HttpOnly`，
`SameSite=Strict`，`Path=/`，但没有 `Domain`. 变异路由需要精确的
配置的 `Origin`，`Sec-Fetch-Site: same-origin`，会话 CSRF 值和一个 `Idempotency-Key`。身份提升、身份链接变异、邀请创建和
取消也需要相关的强 `ETag` 在 `If-Match` 中。身份链接使用 Account 集合 ETag。身份提升、注销、身份移除和
取消只接受空体；JSON 严格限制在 1 KiB，未知字段被拒绝。

所有响应都是`no-store`，使用无来源策略并在失败时返回一个有界、封闭的问题主体。提供者令牌、原始外部主题、数据库消息、状态、nonce、PKCE 验证码、会话密钥和原始邀请令牌从未以JSON形式返回。

## 配置

`CP0_PORTAL_CONFIG` 指向一个严格格式的 JSON 文件，文件大小不超过 64 KiB。未知字段会导致启动失败。其结构为：

```json
{
  "allowed_origin": "https://developer.cardputerzero.dev",
  "post_login_uri": "https://developer.cardputerzero.dev/portal",
  "providers": [
    {
      "key": "primary",
      "label": "Primary identity",
      "issuer": "https://identity.example.com",
      "authorization_endpoint": "https://identity.example.com/authorize",
      "token_endpoint": "https://identity.example.com/token",
      "client_id": "cardputerzero-portal",
      "client_secret_env": "CP0_OIDC_PRIMARY_SECRET",
      "redirect_uri": "https://developer.cardputerzero.dev/portal/auth/callback",
      "accepted_signing_algorithms": ["RS256"],
      "accepted_mfa_acr": ["urn:example:acr:mfa"],
      "clock_skew_seconds": 60,
      "jwks": {
        "keys": [
          {
            "kty": "RSA",
            "kid": "provider-key-id",
            "use": "sig",
            "alg": "RS256",
            "n": "provider-public-modulus",
            "e": "AQAB"
          }
        ]
      }
    }
  ]
}
```

该示例展示了配置形状；生产 `jwks` 值必须从审核过的提供者密钥集复制。允许一个到八个提供者。发行者、授权、令牌和重定向 URL 必须是精确的 HTTPS URL，不包含用户信息、查询或片段。每个提供者重定向 URI 必须等于 `<allowed_origin>/portal/auth/callback`。提供者密钥和发行者必须是唯一的。签名算法仅限于 RSA PKCS#1、RSA-PSS 和 EdDSA；`none`、HMAC 算法、对称 JWK 和 JWK 私有参数会导致启动失败。静态内联 JWKS 使得密钥更改成为显式的审核配置发布和过程重启。

所需的环境变量：

| 变量 | 用途 |
| --- | --- |
| `CP0_PORTAL_DATABASE_URL` | 使用 Store 控制迁移的 PostgreSQL 连接 |
| `CP0_PORTAL_CONFIG` | 绝对路径到严格的提供者JSON文件 |
| `CP0_PORTAL_CSRF_KEY` | 32字节未填充的base64url CSRF HMAC密钥 |
| `CP0_PORTAL_NONCE_KEY` | 32字节未填充的base64url非填充nonce/state HMAC密钥 |
| `CP0_PORTAL_PKCE_KEY` | 32字节未填充的base64url XChaCha20-Poly1305密钥 |
| `CP0_PORTAL_SUBJECT_KEY` | 32字节未填充的base64url发行者/主体HMAC密钥 |
| `CP0_PORTAL_INVITATION_KEY` | 32字节未填充的base64url邀请交付加密密钥 |

五个目的键必须不同。提供方客户端密钥仅从由 `client_secret_env` 指定的大写环境变量加载；如果为公共 PKCE 客户端，请省略该字段。

`CP0_PORTAL_LISTEN_ADDR` 默认为 `127.0.0.1:8790`。除非 `CP0_PORTAL_ALLOW_NON_LOOPBACK=1`，否则不允许非回环绑定；该覆盖不添加 TLS，并且仅在经过审核的 TLS 反向代理之后有效。代理必须保留原始的 `Origin`、`Sec-Fetch-Site`、cookie 和条件头，而不进行广泛的 CORS 重写。

## 提供者和数据库信任

令牌交换只能通过POST请求发送到配置的令牌端点，不会跟随重定向，具有固定的连接/请求超时，并且拒绝大于64 KiB的响应。ID令牌需要配置的公钥算法、精确的发布者和受众、签名、nonce、验证的电子邮件、限定的发行时间以及主题不超过1024字节。升级还需要配置的ACR以及不超过五分钟的`auth_time`。

数据库只存储SHA-256会话/状态/nonce/邀请摘要，发行者和主题的HMAC，以及目的分离的认证加密短寿命PKCE验证器和待处理的电子邮件交付令牌。`SERIALIZABLE`事务和数据库时钟用于会话消费、账户/链接创建、身份删除、会话旋转、邀请变更、撤销、审计和出箱工作。会话在30分钟闲置或绝对8小时后过期。

## 外部身份链接

身份链接集合由Account `resource_version`版本化，并以强`ETag`形式返回。启动过程将提供方、Account、当前会话、请求/幂等性摘要、PKCE、状态、nonce和提供方配置绑定在一个十分钟的交易中。回调仅接受一个新的精确`(issuer, subject HMAC)`。如果身份已经活跃或最终被撤销（包括另一个Account拥有的身份），则会失败。成功会更新Account版本，附加主题自由的审核/出箱证据，消耗交易，撤销旧会话，并创建一个由新链接认证的旋转会话。

移除需要提供者 MFA 证明，该证明不能超过五分钟，并且需要当前的账户 ETag。最多八个链接中至少需要保留一个。数据库将撤销所有 `current_link_id` 匹配移除身份的会话。活跃到暂停/移除的会员变更也会撤销绑定账户的所有门户会话，并使待处理的提升/链接交易失效。

## 邀请交付

创建只接受 `developer`, `release-manager`, 或 `viewer`, 标准化验证过的ASCII电子邮件格式，并要求拥有活跃的所有者会员资格，会员启用双重认证，并且提供不超过五分钟的提供商双重认证证明。一个团队最多包含100个活跃成员加上待处理的邀请。BFF 还限制每个团队每小时最多创建20个邀请，每天同一标准化团队/电子邮件对最多3个。移除的会员身份不能通过v1重新邀请。

原始的256位令牌仅存储为SHA-256接受摘要和XChaCha20-Poly1305封套在`portal_invitation_deliveries`中。封套绑定到邀请ID，并且从未进入API响应、审计事件、幂等记录或通用出站队列。`InvitationEmailWorker`租用一个任务60秒，提交前进行解密或调用提供的`InvitationMailer`，并将接受URL与令牌一起作为片段传递。交付、取消、永久失败、损坏的密文或过期后，它清除密文。瞬态失败使用有界的指数重试，并最多尝试16次。同一个工作者原子地过期七天的邀请，更新团队版本，并附加审计/出站证据。

仓库提供持久化工作者和严格的适配器特性，而不是生产邮件供应商适配器。生产启用需要经过审核的事务性提供者实现，该实现不记录请求体或URL，还需要包含退信、抑制、保留和滥用响应策略。调用者应在受监督的循环中运行`run_once`，并在它返回`false`时退避。

生产应使用专用的最小权限数据库角色、加密的数据库连接、密钥管理器、结构化的请求-ID 日志记录（不包含标头或查询字符串），以及此过程之外的健康监督。OIDC 发现、动态注册、提供方重定向和刷新令牌存储故意不支持。

## 验证

使用 Rust 1.85 或更高版本运行本地网关：

```sh
cargo +1.85.1 check -p cp0-store-portal-server --all-targets
cargo clippy -p cp0-store-portal-server --all-targets -- -D warnings
cargo test -p cp0-store-portal-server
```

PostgreSQL 接受测试需要一个隔离的一次性数据库，其模式可以重置：

```sh
CP0_STORE_TEST_DATABASE_URL='postgresql://...' \
  cargo test -p cp0-store-portal-server --test postgres -- --ignored --nocapture
```

那扇门覆盖了PKCE的保密性，主题HMAC存储，登录/会话创建，CSRF和来源拒绝，闲置刷新，幂等升级，验证MFA旋转，过期cookie拒绝，注销，两个提供商的链接/列表/删除和恢复登录，链接依赖的会话包含，会员暂停传播，邀请创建/列表/检查/取消/接受和精确重放，电子邮件成功/重试/失败，令牌保密性，接受会话旋转，过期，SQL绕过拒绝，注入交易回滚，以及提供商侧终端失败。
