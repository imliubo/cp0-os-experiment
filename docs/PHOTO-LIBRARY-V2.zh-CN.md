# CardputerZero 图库 v2

<!-- doc-locale: zh-CN -->
> [English](PHOTO-LIBRARY-V2.md) | **简体中文**

Camera 和 Gallery 是生产内置的 WASM 应用程序。它们保持在沙盒中，并不能访问 `/dev/video*`，一个 SD 卡路径，另一个应用的数据，或照片库文件。appd 拒绝卸载请求 `dev.cardputerzero.camera` 和 `dev.cardputerzero.gallery`。开发包不能替换这些身份；签名的 Store 升级仍然允许。

## 用户合约

- 相机照片和受信任系统截图共享一个相册库。
- 没有照片数量保留限制，也没有自动驱逐。
- 在相册中，一张照片会一直可见，直到所有者显式地删除它。
- 当SD卡无法接受另一张完整的照片同时保留64 MiB用于系统时，新的保存会失败并显示`ResourceLimit`；现有照片和索引不变。
- 画廊缓存八个ID和一个RGB565帧。appd 只缓存当前解码的相机原始图像，与库大小无关。

物理介质容量是唯一的实际库绑定。系统照片
身份使用一个故意无法达到的1 PiB 逻辑配额，因此 storaged 的正常每应用配额不能成为一种人为的保留策略。

## 权限

| 权限 | 访问 |
| --- | --- |
| `camera.capture` | 读取一个 320x170 预览或请求一个固定大小的 1280x720 照片。 |
| `photos.read` | 读取版本化的索引和选定的帧片段。 |
| `photos.write` | 添加一帧或删除一张选定的照片。 |

Camera 声明 `camera.capture` 和 `photos.write`. Gallery 声明 `photos.read` 和 `photos.write`. 私有应用存储保持身份绑定。
当前 SDK 暴露帧导入和 ID 基础的移除；页面/头部变异和单调递增的照片 ID 由 broker 所有。ABI 保留 SDK 1.0 ID 提示，但 appd 不信任它进行分配。遗留的低级 ABI 符号仍然可加载，而 appd 拒绝直接帧或元数据变异。

## 格式

每个Gallery显示框是一个320x170 RGB565小端格式缩略图，恰好108,800字节。截屏和兼容性`photos.import-rgb565`调用只存储这种表示。Camera`capture-photo`交易还会存储一个固定大小的1280x720 JPEG原始图像作为`p<16-hex-id>.jpg`和一个56字节的`p<16-hex-id>.meta`记录，包含种类、尺寸、JPEG大小、捕获时间和SHA-256摘要。Camera App只接收代理拥有的照片ID；JPEG图像从不复制到WASM内存中。

appd 验证调用的应用，并拥有完整的导入交易。它的私有 storaged 客户端将有界 8 KiB 块写入模式 `0600` 临时 blob。中间块仍处于不可达的暂存名称下；在最终块之后，storaged 刷新整个文件，原子重命名它并刷新包含目录。缩略图、可选的原始文件和元数据在索引页面和权威头部之前写入。失败的交易会移除所有未提交的组件，并从不列出部分照片。

画廊加载一个提交的帧，并使用一个 `photos.load-rgb565` 主程序调用。appd
首先验证请求的ID在提交索引中仍然有效，
然后请求storaged以只读方式打开相应的blob。storaged仅在描述符操作针对系统照片库身份并且blob是一个具有精确RGB565帧大小的常规文件时接受此描述符操作。描述符跨两个Unix套接字边界使用 `SCM_RIGHTS`；appd和Runtime独立重新验证其类型、大小和访问模式，然后再映射或复制任何像素。这取代了旧的连续14个base64块读取，同时保持相同的App隔离边界。

相机原图通过`photos.load-view-rgb565`查看。唯一的输入是一个活动的照片ID、适合/一半/实际缩放以及有界的`-1000..1000`平移坐标。appd 验证元数据，打开精确的只读JPEG块，解码并缓存当前1280x720图像，然后渲染一个固定的320x170 RGB565视窗到一个密封描述符中。相册从未接收JPEG字节、一个存储键、一个文件系统路径或一个全分辨率RGB分配。

`head.v2` 是一个固定的32字节记录，包含：

- 魔法 `CP0H`, 版本 2 和保留的零字节；
- 活动照片数量
- 只读槽计数;
- 最后一次分配的单调递增照片ID。

`index.v2.<8-hex-page>` 包含 256 个有序的 ID 槽。显式删除会将一个槽变成零墓碑，并减少页面/头部的活跃计数；它不会压缩或重新编号后续的照片。画廊的 `list_page(offset, out)` 使用逻辑活跃照片偏移量并跳过墓碑。

保存会发布缩略图和可选的JPEG/元数据，更新其索引页面，然后提交 `head.v2`。页面/头部失败会恢复旧页面并移除每个未提交的组件。删除会提交墓碑和头部，然后回收缩略图、JPEG和元数据。
相机导入、Shell 截图导入和画廊删除共享一个 appd 事务锁，因此它们的页面/头部更新不能相互覆盖。

已提交的 header slot count 是恢复边界。Gallery 从已提交的 page slot 推导可见计数，不信任缓存计数。下一次修改前，appd 会校准 page count，并清除在丢失的 header commit 前写入的 page tail；storaged 启动时只安全删除 daemon 中断或掉电留下且经过验证的 `.cp0-blob-*` staging file。符号链接和无效条目按 fail-closed 原则拒绝。

## v1迁移

如果 `head.v2` 缺失，写入器会读取 `index.v1`，写入第零页，并在提交等效的 v2 头之前追加。遗留索引和旧的十四键框架仍然可读。新框架使用一个数据块。失败的迁移会移除未提交的第零页并使 v1 成为主导版本。

## 备份

产品数据分区将 `/var/lib/cardputerzero` 映射到 `cardputerzero` 根目录下的 `CP0 backup v1`，因此完整的恢复备份/恢复已经包括了共享库。每日导出到计算机是一个单独的只读所有者照片传输工作流，描述在 `PHOTO-TRANSFER-V1.md` 中。
