# Store 提交

<!-- doc-locale: zh-CN -->
> [English](store-submission.md) | **简体中文**

只有在用户请求 Store 分发时阅读。开发人员提交结束于不可变上传版本；审核、Store 签名和发布属于独立操作。

## 准备不可变输入

使用开发人员签名的 `.capp`，但不包含Store签名。在上传前将其稳定的App ID注册到开发人员门户中。将Store资源保留在项目旁边：

```text
store/
  listing.json
  images/icon.png
  images/screen-1.png
```

清单最多 32 KiB，拒绝未知字段，并且必须完全匹配应用程序的包 ID 和版本。使用一个大小不超过 64 KiB 的 48x48 PNG 图标，以及一个到五个大小不超过 512 KiB 的 320x170 PNG 截图。路径相对于 `store/` 是 ASCII 相对路径，绝不能是符号链接或 `..`。声明按顺序排列的本地化资源、关键词、支持的类别、年龄分级、HTTPS 隐私 URL 和 HTTPS 支持 URL，根据 `schemas/store-listing-v1.schema.json` 在源代码检出中声明。
相同的模式在发布的 DevKit 中位于 `ROOT/schemas/store-listing-v1.schema.json`。

为了获取物理截图，请使用受信任的System Shell `Fn+J` 捕获。应用程序不能调用或读取截图服务；获取生成的PNG文件是设备所有者/运营商的明确操作。不要发明一个受信任的状态栏或将320x150模拟器表面拉伸成Store截图。

## 验证并提交

在每次输入更改后运行本地验证：

```sh
cp0ctl store validate APP.developer.capp store/listing.json
```

它验证开发者的签名、身份、路径、PNG结构和像素，大小、SHA-256值以及缺少Store签名。只有这样之后才能运行：

```sh
cp0ctl store submit APP.developer.capp store/listing.json
```

CLI 将 OAuth 设备授权流程说明打印到 stderr。使用显示的开发人员门户中的有效所有者/开发人员帐户和当前 2FA 进行批准。令牌保存在进程内存中，仅具有 `store.submit` 范围，并且永远不能替代开发人员签名密钥。成功的 stdout 包含 JSON 格式的 `submission_id`, `state`, `content_sha256` 和 `portal_url`.

生产源固定。仅在明确请求的HTTPS开发控制平面中使用`CP0_STORE_API`。绝不要上传私钥、保存OAuth令牌、盲目重试致命的4xx响应、运行`store publish`或使用Store密钥签署包。
