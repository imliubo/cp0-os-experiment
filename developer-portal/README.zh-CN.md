# CardputerZero 开发者门户

<!-- doc-locale: zh-CN -->
> [English](README.md) | **简体中文**

本地 S4 前端 MVP 用于 Store 控制平面。门户是一个单独的 Web 应用程序，并且永远不会安装在 CardputerZero 设备镜像中。

Store内容工作流仍然使用有界内存演示数据。账户安全
在配置了`VITE_PORTAL_BFF_ORIGIN`时使用真实的Portal BFF：会话、MFA提升步骤、身份列表/链接/移除、注销和邀请方法由cookie/CSRF `PortalApi`实现。刷新页面重置演示Store内容但从BFF重新加载身份状态。

门户从不请求或存储开发者的私钥。生产客户端必须在内存中提供短期的OAuth令牌到`StoreApi`。API客户端需要一个裸的HTTPS来源，省略浏览器凭证，拒绝重定向，限制JSON响应大小为64 KiB，向写操作添加幂等键，并要求现有资源的ETag进行修改。

## 开发

```sh
npm install
npm run check
npm run dev
```

UI 支持桌面和移动宽度，物理键盘导航，可见焦点，减少运动效果，以及原生表单控件。`npm run check` 在创建生产包之前运行状态模型/API 边界测试。

对于同站点的生产构建：

```sh
VITE_PORTAL_BFF_ORIGIN=https://developer.cardputerzero.dev \
VITE_PORTAL_PROVIDERS=primary,secondary npm run build
```

BFF 原始必须是裸 HTTPS 原始。`PortalApi` 只向 `/portal/v1/*` 发送浏览器凭据，将 CSRF 保留在内存中，拒绝重定向和过大响应，并从应用代码中永不接受 OIDC 或 Store  bearer token。
