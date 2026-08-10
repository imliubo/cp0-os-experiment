# CardputerZero 审查控制台

<!-- doc-locale: zh-CN -->
> [English](README.md) | **简体中文**

内部 React/Vite 客户端用于与 Store 审核 API 的边界内审核。运行时使用 Review 工作force BFF 进行会话/登录/注销和短暂的控制令牌交换，然后从 Store 控制中读取真实的 Review 队列和提交详情。队列分页、主要和独立的次要声明、追加消息、结构化决策、不可变哈希、扫描发现、权限、导入、提交资产元数据和边界内审计历史都来自权威 API 响应。成功的变更后跟随着一次新的服务器读取，而不是乐观的本地状态。

Cookies 只发送给受众特定的工作force BFF。控制凭据令牌保存在内存中，从不持久化，并且 Store Control 请求使用 `credentials: omit`。严格的适配器拒绝重定向、跨受众会话、范围不匹配、畸形对象以及大于 64 KiB 的响应。

将 `VITE_REVIEW_WORKFORCE_ORIGIN` 设置为 Review BFF HTTPS 原始地址，并将 `VITE_REVIEW_CONTROL_ORIGIN` 设置为 Store Control HTTPS 原始地址。两者都必须是裸 HTTPS 原始地址。生产部署仍然受到 IdP/JWKS、管理密钥、域名和撤销练习门的约束，这些约束在 `docs/STORE-WORKFORCE-SERVER.md` 中文档化。

```sh
npm test
npm run build
npm run dev
```
