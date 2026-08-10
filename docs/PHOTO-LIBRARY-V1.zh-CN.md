# CardputerZero 图库 v1（legacy）

<!-- doc-locale: zh-CN -->
> [English](PHOTO-LIBRARY-V1.md) | **简体中文**

Photo Library v1 使用了一个 `index.v1` 记录，最多包含 32 个照片 ID，并将每个 320x170 RGB565 帧存储为 fourteen 8 KiB 存储值。它仅作为设备上的迁移源保留。

Photo Library v2 读取现有的 v1 索引，写入等效的 v2 页和头部，然后追加新的照片。v2 头是可见性边界，迁移过程中不会移除遗留索引和帧值。请参见`PHOTO-LIBRARY-V2.md`了解活跃合同。
