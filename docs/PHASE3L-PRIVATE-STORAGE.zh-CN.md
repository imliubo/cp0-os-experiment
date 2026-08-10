# Phase 3L: 配额限制的私有存储

<!-- doc-locale: zh-CN -->
> [English](PHASE3L-PRIVATE-STORAGE.md) | **简体中文**

私有应用数据仅通过CardputerZero SDK暴露。Runtime不再接收可写的绑定挂载主机应用数据目录。其`/data`目录是一个空的命名空间本地目录，且seccomp策略继续拒绝`open`, `openat`和文件系统 mutation 系统调用。

## API和配额

SDK 提供 `put`、`get` 和 `delete` 操作以验证密钥：

- 密钥包含1到64个ASCII字母数字字符，`.`、`_` 或 `-` 字节，并且不能以 `.` 开始；
- 值包含1到8192字节；
- 每个应用最多可以存储 256 个密钥；
- 一个缺失的键与服务错误不同；
- 硬字节配额是安装的清单的 `resources.storage_mb`。

存储是一个基础应用设施，而不是用户授予的权限。
appd 仍然会每次调用都验证安装的应用 UID，
激活的 systemd cgroup 和根拥有的清单，然后在特权服务请求中添加应用 ID 和配额。

## 隔离路径

```text
WASM storage SDK call
  -> Runtime validates linear-memory ranges, key and value bounds
  -> appd authenticates UID, PID, active cgroup and installed manifest
  -> root-only cp0-storaged socket
  -> cp0-storaged derives one fixed application directory
  -> quota check, owner/mode/type checks and atomic filesystem operation
```

`cp0-storaged` 是唯一可以访问的账号
`/var/lib/cardputerzero/data`. 目录和每个应用程序子目录 使用模式 `0700`; 值使用模式 `0600`该服务没有设备或网络访问权限，systemd 为其授予一个可写路径。应用程序的 Unix 账户不拥有或挂载这些目录。

## 原子性和计费

一个 put 检查每个直接入口，拒绝符号链接和损坏或过大文件，减去替换值，并将预估总量与清单配额进行比对。它写一个唯一命名的同目录文件，调用 `fsync`，原子重命名它覆盖目标并同步目录。由于断电会留下一个临时文件，所以配额会被计入，因此中断的操作不能绕过计费。

逻辑值字节是文档化的配额单位。文件系统元数据和SD分配开销故意不暴露给应用程序。

## 验证

测试涵盖标准协议封装、最大值、无效键、原子替换、删除、精确配额耗尽、Runtime JSON 解码、Rust 和 C/C++ SDK 验证、可写主机绑定的移除、systemd 所有权和套接字限制，以及 AArch64 交叉编译。实际身份配额、进程重启、重启和跨应用探测在 `PHASE3M-DEVICE-CAPABILITY-ACCEPTANCE.md` 中有记录。物理执行和断电接受仍待当前 24 小时核心稳定性运行完成。
