# 可信屏幕截图代理服务

<!-- doc-locale: zh-CN -->
> [English](SCREENSHOT-BROKER.md) | **简体中文**

## 合同

`Fn+J` 由 compositor 拥有，并从未以原始 PrintScreen 键的形式到达应用程序。compositor 将受信任的操作发送给 System Shell，System Shell 捕获当前可见的帧缓冲区并将其导入生产照片库。

V0.6 合同已固定：

- 一个分辨率为320x170的XRGB8888或ARGB8888捕获；
- 一个 108,800 字节的 RGB565 小端序 Gallery 帧；
- 共享的照片库是唯一的持久目的地；
- 没有Shell私有的PNG重复文件和自动保留删除。

没有截图 API 是应用程序 SDK 的一部分。应用程序不能请求设备截图、命名目标路径或访问主机路径。画廊只能通过`photos.read`读取结果。

## 授权

Weston 的捕获全局默认被拒绝。`cardputerzero-policy.so` 允许只有当确切的信任 Shell Wayland 客户端拥有注册的`os.cardputerzero.shell` 表面，并且选择的输出是 320x170 时才进行尝试。

System Shell 将捕获转换为只读 memfd，并发送给 appd。描述符必须携带 `F_SEAL_SEAL`、`F_SEAL_SHRINK`、`F_SEAL_GROW` 和 `F_SEAL_WRITE`。appd 验证 `SO_PEERCRED`，并仅接受来自配置的 `cp0-shell` UID 的 `import-screenshot`。root、Store、Apps、缺少或多余描述符、可写文件和错误大小均按 fail-closed 原则拒绝。

## 持久性

appd 持有共享库事务锁，原子地发布一帧 blob，更新 v2 索引页面并提交 `head.v2`。失败的操作会移除其暂存数据，并保留所有之前提交的照片可见。相同的锁覆盖 Camera 导入和 Gallery 删除。没有 32 帧路径，也没有重复的截屏状态目录。

捕获开始于状态覆盖之前，因此它记录了按键时可见的屏幕。交易提交后，Shell 显示 `SCREENSHOT / SAVED` 两秒。存储/协议失败显示 `FAILED`；不支持的捕获合同显示 `UNAVAILABLE`。

## 验证

本地测试涵盖XRGB8888到RGB565转换、严格控制响应、FD传输和封印、仅Shell授权、v1迁移、v2回滚、单Blob发布、中断尾部恢复、启动阶段清理以及超过32帧的保留。物理`Fn+J`、画廊显示、SD全行为和断电后的保留是设备接受项。
