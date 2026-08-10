# 存储媒体资源 v1

<!-- doc-locale: zh-CN -->
> [English](STORE-MEDIA-RESOURCES-V1.md) | **简体中文**

S6B 定义并发布 Store 浏览和应用详情使用的不可变媒体对象。它在 CM0 的 512 MB CM0 中保持签名根目录的边界，同时允许按需获取和验证屏幕截图。

## 有符号层次

Catalog v3 需要 v2 `discovery` 对象和一个 `resources` 对象：

```text
Store-signed Catalog v3
  -> icon URL + SHA-256 + bytes + dimensions
  -> details URL + SHA-256 + bytes
       -> description and release notes
       -> screenshot URL + SHA-256 + bytes + dimensions
```

该详细文档使用 `StoreAppDetails` 架构 v1，拒绝未知字段，并且大小最多为 16 KiB。它重复了 `app_id` 和 `version`；设备消费者必须在验证详细文档的 SHA-256 后，将两者与 Catalog 条目匹配。

目录v1、v2和v3保持独立。v1条目不能包含发现或资源，v2需要发现但不包含资源，而v3需要两者。发布者选择每个预览 artifact 支持的最高 schema，并在遗留 artifact 需要较低 schema 时移除较新字段。混合 schema 条目从不签名。

## 图像合约

所有镜像都是PNG文件，在隔离的扫描器在审查前检查了其结构、CRC、尺寸和批准的摘要：

| 资源 | 尺寸 | 单文件最大值 | 数量 |
| --- | --- | --- | --- |
| 应用图标 | 列表v1为48x48；协议还预留32x32 | 64 KiB | 一个 |
| 截图 | 320x170 | 512 KiB | 一到五 |

每个描述符包含一个有边界限制的HTTPS URL、小写SHA-256、确切的字节长度、宽度和高度。重定向或替换的CDN内容不能满足签名的描述符。

## 不可变原始布局

发布者从未从开发者资产路径，而是从服务器ID和数组索引派生每一个路径：

```text
generations/<sequence>/assets/<release-id>/icon.png
generations/<sequence>/assets/<release-id>/details.json
generations/<sequence>/assets/<release-id>/screenshots/<index>.png
```

发布者重新读取所有经过审批的内容寻址上传片段。包、图标、屏幕截图、详情、目录、透明度叶子/检查点和公钥写入一个临时生成文件，同步，重命名并验证，在数据库提交和`current`切换之前。后续目录快照保留指向原始不可变生成文件的URL。

## CM0 缓存预算

设备缓存实现必须强制执行这些独立的磁盘预算：

- 验证过的目录：一个活动文件，最多 48 KiB；
- 急切图标缓存：最多 4 MiB，足以容纳 64 个最大尺寸的图标；
- 细节缓存：最多 1 MiB，足以存储 64 个最大尺寸的清单；
- 按需截图缓存：最多 8 MiB 总大小，并经过验证的 LRU 回收机制；
- 一次临时资源下载；临时字节和最终字节都计入相关预算。

资源通过SHA-256存储，以所有者只读权限写入，并在经过精确长度和摘要验证后重命名。缺少或损坏的资源可能会从Store中移除媒体，但绝不能阻止已安装应用的启动。

S6C 在 `cp0-stored` 下实现此合同：

```text
/var/lib/cardputerzero/store/media/icons/<sha256>.png
/var/lib/cardputerzero/store/media/details/<sha256>.json
/var/lib/cardputerzero/store/media/screenshots/<sha256>.png
```

目录刷新提交验证后的目录，然后再进行最佳努力顺序图标预取，因此 CDN 媒体故障无法回滚发现或阻止包安装。详细信息再次进行解码并必须匹配目录 `app_id` 和版本。截图按需获取并使用文件修改时间进行稳定 LRU。启动时移除未引用的图标/详细信息对象，拒绝符号链接和无效文件模式，重新检查保留的图标/详细信息对象，清理未完成的临时文件而不跟随它们，并在访问时重新检查每个截图。S6D 通过绑定到严格响应元数据的一个只读描述符将验证后的媒体暴露给系统 shell；私有缓存路径从不离开 `cp0-stored`。参见 `STORE-DEVICE-RICH-DETAILS-V1.md`。

## 验证

```sh
cargo test -p cp0-store-protocol -p cp0-store-publisher --lib
cargo test -p cp0-stored --lib

CP0_STORE_TEST_DATABASE_URL=postgres://... \
  cargo test -p cp0-store-publisher --test postgres -- --ignored --nocapture
```

协议覆盖范围拒绝了方案混用、缺失资源描述符、不安全的URL、不良校验和、错误维度、重复截图、无界细节和不安全的说明。发布者单元测试覆盖了v1/v2/v3投影迁移。设备测试覆盖了精确媒体缓存、仅所有者模式、篡改CDN字节、目录/安装独立性和有界截图LRU。PostgreSQL网关从不可变源头读回每个生成对象，重新计算哈希值，解码细节，并在数据库回滚和所有者Team重命名后证明字节级生成重用。
