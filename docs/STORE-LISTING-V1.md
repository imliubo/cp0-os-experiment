# Store Listing v1

`store-listing-v1` 是开发者提交应用时的公开元数据契约。建议 SDK 项目使用以下固定位置：

```text
my-app/
  app.json
  store/
    listing.json
    images/
      icon.png
      screen-1.png
```

`listing.json` 中的资源路径相对于 `store/` 目录。路径必须是安全的 ASCII 相对 PNG 路径，
不能包含 `..`、反斜线或空组件。开发者私钥和 Store 签名不属于这个目录。

## 示例

```json
{
  "schema_version": 1,
  "app_id": "dev.cardputerzero.notes",
  "version": "1.2.0",
  "default_locale": "zh-Hans-CN",
  "category": "productivity",
  "age_rating": "4+",
  "privacy_url": "https://example.com/privacy",
  "support_url": "https://example.com/support",
  "icon": {
    "path": "images/icon.png",
    "sha256": "1111111111111111111111111111111111111111111111111111111111111111",
    "bytes": 4096,
    "width": 48,
    "height": 48
  },
  "screenshots": [
    {
      "path": "images/screen-1.png",
      "sha256": "2222222222222222222222222222222222222222222222222222222222222222",
      "bytes": 32000,
      "width": 320,
      "height": 170
    }
  ],
  "localizations": [
    {
      "locale": "zh-Hans-CN",
      "name": "便签",
      "subtitle": "为小屏优化的快速便签",
      "description": "记录和整理短便签。完全离线工作。",
      "keywords": ["便签", "效率"],
      "release_notes": "首个公开版本。"
    }
  ]
}
```

## 冻结边界

- Listing JSON 最大 32 KiB，拒绝未知字段。
- `app_id` 和 `version` 必须与开发者签名 `.capp` 内的 manifest 完全一致。
- locale 采用有界的规范 BCP 47 子集，例如 `en`、`en-US`、`zh-Hans-CN`、`es-419`。
- 最多 8 个 locale；必须按 locale 排序且包含 `default_locale`。
- 每个 locale 最多 8 个关键词；关键词必须唯一并按字典序排序。
- 分类固定为 `developer-tools`、`education`、`entertainment`、`games`、`hardware`、
  `media`、`productivity`、`utilities`。
- 年龄分级固定为 `4+`、`9+`、`12+`、`17+`，开发者声明仍需审核确认。
- 图标固定为 48x48 PNG、最大 64 KiB；1-5 张截图固定为 320x170 PNG、每张最大
  512 KiB。
- 隐私和支持链接必须使用 HTTPS，禁止凭据、fragment、空白和控制字符。

JSON schema 位于 `schemas/store-listing-v1.schema.json`，共享严格验证器位于
`cp0-store-metadata` crate。它们负责字段、顺序和边界；后续 `cp0ctl store validate` 和
扫描 worker 还必须读取实际资源，验证 PNG 格式与像素、复算大小/SHA-256，并把 Listing
与开发者签名包的精确摘要绑定。仅通过 JSON 结构验证不能发布应用。
