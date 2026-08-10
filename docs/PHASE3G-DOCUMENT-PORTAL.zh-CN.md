# 阶段3G：受限文档门户

<!-- doc-locale: zh-CN -->
> [English](PHASE3G-DOCUMENT-PORTAL.md) | **简体中文**

## 范围

第一个文档门户实现允许WASM应用程序请求一个用户选择的只读文档，而不暴露主机路径、目录API或WASI文件系统。应用程序必须声明`documents.open`，并且只接收一个不透明的Runtime句柄以及文件长度限制。

初始合同故意很小：

- 文档存放在`/var/lib/cardputerzero/documents`中；
- 可信的 System Shell 最多显示 16 个直接普通文件；
- 一次只能有一个文档在Runtime中激活；
- 一份文档最多为 256 MiB；
- 每次 SDK 读取最多为 4096 字节，并使用显式的 64 位偏移量；
- 应用程序不能选择或提交路径或文档ID。

## 信任流

```text
WASM cp0_document_open
  -> Runtime sends open-document with no path or identity
  -> appd verifies peer UID, systemd cgroup, manifest and permission
  -> cp0-documentd returns a bounded opaque-ID/name snapshot
  -> trusted System Shell renders the single foreground file picker
  -> Shell resolves only an ID present in that snapshot
  -> cp0-documentd opens the direct child with openat(O_NOFOLLOW)
  -> cp0-documentd verifies the opened device/inode, type and size
  -> descriptor crosses documentd -> appd -> Runtime with SCM_RIGHTS
  -> Runtime verifies O_RDONLY, regular-file type and exact bounded size
  -> WASM reads through a validated pointer/length host call
```

`cp0-documentd` 以专用的 `cp0-document` 账户运行。它的 systemd 单元
具有空的能力集、空的设备视图、严格的只读系统视图和仅 `AF_UNIX`。它的套接字是 `0600 root:root`，所以只有 appd 才能调用它。相反，appd 没有 DAC 能力，文档根目录是 `0750`，由 `cp0-document` 所拥有；appd 接收到一个描述符但没有被授予目录遍历权限。

## 赛跑和逃脱阻力

文档ID是固定宽度的小写十六进制设备/节点标识符，不是文件名。服务仅枚举直接的UTF-8名称，并拒绝斜杠、控制字符、目录、符号链接、过大文件和重复的硬链接标识符。打开使用非跟随目录FD加上`openat(O_RDONLY|O_CLOEXEC|O_NOFOLLOW)`，接着是`fstat`；打开的设备/节点仍需匹配选定的ID。因此，重命名、替换或符号链接交换会失败。

Runtime 保持接收到的 FD 私有。WASM 只能看到一个生成句柄，并且只能调用 `open`，受限的 `read` 和 `close`；它不能调用 `read(2)`，复制描述符或发现宿主路径。第二次成功的打开会关闭之前的描述符，过时的句柄会被拒绝。

## 验证

自动化覆盖包括：

- 严格的 4 KiB 协议和精确的一个-FD `SCM_RIGHTS` 转移；
- 符号链接, 伪造-ID 和 后选替换拒绝;
- 只读描述符和设备/节点检查；
- 受信任提示快照选择和取消
- Shell JSON 解析，键盘状态，滚动和像素快照回归测试；
- 运行时 stale-handle, EOF, 偏移量和 4096 字节读取边界；
- Rust、C11、C++17 和 WIT SDK 接口；
- 加固的服务和镜像阶段断言。

该实现已在本地验证并通过，并包含在未来的镜像构建中。在进行Phase 2 24小时稳定性运行期间，故意不进行热部署。最终的物理选择和文档内容接受属于下一个集成镜像硬件过程。
