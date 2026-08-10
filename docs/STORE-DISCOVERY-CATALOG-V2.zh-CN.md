# Store 发现目录 v2

<!-- doc-locale: zh-CN -->
> [English](STORE-DISCOVERY-CATALOG-V2.md) | **简体中文**

Catalog v2 在不改变包安装信任链的情况下添加了签名发现元数据。这是朝着在320x170设备上实现类似应用商店的浏览和搜索体验的第一个S6切片。

## 兼容性合约

`cp0-store-protocol` 接受恰好两个目录模式：

- v1 包含原始包、摘要和权限字段，并且必须不包含`discovery`；
- v2 要求每个应用都使用 `discovery`。
- 任何其他模式、v1/v2 字段混用、未知字段或缺少v2值都会失败
  在目录可以替换验证缓存之前就关闭了。

签名信封和防回滚序列保持不变。模式版本位于签名目录中，因此CDN不能添加、删除或重写发现字段。离线 `cp0ctl store publish` fixture builder 继续发出 v1 以保持恢复兼容性。一旦包含的包 artifact 完成签名发现数据，生产投影会发出 v2。在升级期间，任何剩余的遗留 artifact 保留完整的 v1 摄影；发布者移除 v2 字段而不是发出混合或不可用的目录。

## 有符号发现字段

每个 v2 应用程序增加：

| 字段 | 来源 | 限制 |
| --- | --- | --- |
| `developer` | 发布时所有者团队显示名称 | 1-80安全字符 |
| `subtitle` | 批准的默认列表本地化 | 长度为1-48个字符；必须等于`summary` |
| `category` | 批准的列表类别 | 关闭的八值枚举 |
| `keywords` | 经批准的默认列表本地化 | 最多八个，唯一且排序 |
| `age_rating` | 批准的列表项 | `4+`, `9+`, `12+` 或 `17+` |
| `privacy_url` | 批准的列表项 | 有界的HTTPS URL |
| `support_url` | 批准的列表项 | 有界的HTTPS URL |

发布者重新构建开发者签名的包、列表和每个不可变内容寻址上传部分的资产，验证它们的摘要和独立双重审批，然后在一个不可变生成中创建仓库签名的包和Catalog v2。它从不接受编辑器覆盖或未签名请求字段的发现元数据。

## 设备行为

`cp0-stored` 验证并缓存使用相同键、有效窗口和序列保护的 v1 和 v2。现有名称排名保持稳定。对于 v2，本地搜索还会匹配签名开发人员名称、类别和关键词；精确关键词匹配优先于通用元数据包含。搜索文本保持本地，并从未发送到 Store 原点。

当前 System Shell 的 Catalog 响应仍保持有界 v1 summary 形式，因此该改动不会增加 C UI allocation 或 frame size。S6B 现在通过 Catalog v3 发布签名图标和截图资源；资源缓存与渲染、本地化选择、Today collection，以及使用兼容签名 root、category index 和有界 shard 的 S6E 现已加入；参见 `STORE-SHARDED-CATALOG-V1.zh-CN.md`。

## 验证

```sh
cargo test -p cp0-store-protocol -p cp0-stored
cargo check -p cp0-store-publisher --all-targets

CP0_STORE_TEST_DATABASE_URL=postgres://... \
  cargo test -p cp0-store-publisher --test postgres -- --ignored --nocapture
```

该协议测试涵盖v1/v2分离、缺失发现数据、非规范关键词、字幕不匹配和签名篡改。设备服务测试涵盖v1排名兼容性和v2开发者/类别/关键词搜索。PostgreSQL发布者网关证明v2值来自批准的列表和团队，而生成的包和目录保持可重复性和签名。发布者单元测试覆盖率还证明混合遗留投影保持纯v1。
