# CardputerZero Store 操作

<!-- doc-locale: zh-CN -->
> [English](README.md) | **简体中文**

Store操作控制台的内部React/Vite客户端。运行时使用Operations工作力量API前端进行会话/登录/注销，并进行受众范围内的控制令牌交换。它读取和写入实际的今日边界布局，加载带有键集分页的已发布候选版本，渲染320x170设备预览，并读取和决定实际的SLA排序的审核队列。成功的变异后跟一次新的服务器读取。

严格的API适配器使用 `/v1/editorial/releases`, `/v1/editorial/today`，和 `/v1/moderation/*`. 发布版发现验证了精确 `rel_`
身份, 当前 Catalog 序列顺序, 唯一 App, 标准光标, 并且在选择器可以消耗之前有边界响应。适配器需要一个短暂的内存操作授权令牌。Cookie 只发送给 BFF；Store Control 请求使用 `credentials: omit`客户端拒绝重定向、跨受众或范围不匹配、畸形对象以及大于64 KiB的响应，并将每项修改绑定到一个随机的幂等键和强ETag。编辑器仅接收 `store.editorial`；管理员也可能接收
`store.moderation`.

将 `VITE_OPERATIONS_WORKFORCE_ORIGIN` 设置为操作 BFF HTTPS 原始地址，并将 `VITE_OPERATIONS_CONTROL_ORIGIN` 设置为 Store 控制 HTTPS 原始地址。两者都必须是裸 HTTPS 原始地址。

```sh
npm test
npm run build
npm run dev
```

生产部署仍然停滞在实际IdP/JWKS集成、管理密钥、生产域名以及实时撤销演练上。生产审核也仍然禁用，直到政策所有权、双人审批、可逆目录抑制、通知投递、安全值班和保留得到批准并执行。
