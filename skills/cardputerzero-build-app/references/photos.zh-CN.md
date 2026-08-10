# 共享照片库

<!-- doc-locale: zh-CN -->
> [English](photos.md) | **简体中文**

当应用捕获、保存、列出、显示或删除照片时，请使用此参考。该库是一个代理服务 SDK 能力，而不是文件系统或私有存储命名空间。

## 公共 Rust API

每一帧是精确的320x170 RGB565小端序：54,400 `u16` 像素或
108,800 字节。保留一个调用者拥有的帧并仅使用这些高级调用：

- `photos::count()` 返回当前活动的`u64`照片数量；
- `photos::list_page(offset, output)` 从逻辑活动光偏移处填充一个有界调用者拥有的`Photo` 切片；
- `photos::load_rgb565(photo, pixels)` 加载一整幅画面；
- `photos::save_rgb565(pixels, suggested_id)` 原子地导入一帧并返回由能力代理服务分配的非零ID；
- `photos::delete(photo)` 明确删除了一张选定的照片，并报告该照片是否存在。

使用 `photos::LIST_PAGE_PHOTOS` （八个）作为小固定导航缓存。不要使用 SDK 的内部密钥、块、遗留导入或 ID 提示作为 App 存储格式。ID 由 appd 递增分配，删除不会重新编号剩余库中的 ID。

## 权限和隔离

声明 `photos.read` 用于计数、列表和加载。声明 `photos.write` 用于保存或删除。单独捕获一个新的摄像头帧需要 `camera.capture`。私有 `storage` 不授予共享照片访问权限，而共享照片权限也不会暴露另一个应用的私有数据。

任何应用都不会获得 SD 卡路径、Gallery 索引路径、设备节点或可变的照片库元数据。相机照片和可信的 System Shell 截图共享同一照片库，但应用只能看到代理 ID 和帧像素。所有者照片传输属于单独的 Owner 工作流，不是应用或 Developer Mode API。

没有固定的照片数量限制，也没有自动驱逐。一帧保持存在，直到所有者显式删除它。当存储无法在接收完整帧的同时保存64 MiB给系统时，save返回`ResourceLimit`，现有照片保持不变。

## 实现模式

1. 在代理工作之前渲染一个可用的加载、空状态或权限状态。
2. 调用 `count`，然后夹紧选定的逻辑偏移量，接着使用 `list_page` 获取一页八项内容。
3. 将选定的帧加载到一个固定的54,400像素缓冲区中。
4. 在执行 `delete` 之前需要明确确认，然后刷新计数和页面。
5. 处理 `Denied`, `Unavailable`, `ResourceLimit` 和格式错误/缺失的帧
   作为可见的、可恢复的状态。

删除后，计数/列表不再暴露照片及其边框的字节，但v2头部/页面元数据仍然存在。因此，模拟器 `photo_library_bytes` 通常不会返回到零；请验证可见计数和边框移除，而不是将保留的索引字节视为泄露的照片。

模拟器每次运行时都以空的确定性照片库开始。`--permissions allow`允许该运行中声明的照片调用；`deny`检查否决路径。一个App运行中的保存然后列出测试可以锻炼导入、分页和加载。检查JSON配置文件中的`capability_calls`、`photo_library_keys`和`photo_library_bytes`；绝不要单独从渲染帧中推断成功。

使用 `examples/camera` 进行捕获/导入和使用 `examples/gallery` 进行分页读取/删除。它们是生产内置函数，具有保护身份，因此将交互模式复制到新生成的项目中，并使用开发人员拥有的 App ID。
