# Phase 3B: appd 生命周期服务

<!-- doc-locale: zh-CN -->
> [English](PHASE3B-APPD-LIFECYCLE.md) | **简体中文**

## 信任边界

`cp0-appd` 是根用户拥有的、基于套接字激活的守护进程。生命周期请求仅命名应用程序 ID。守护进程从其根用户拥有的注册表和验证过的规范性清单中推导出安装版本、Unix 账户、包路径、入口点、内存限制和单元名称。客户端不能提供主机路径、用户、命令或 systemd 属性。

每次启动前，`appd` 验证：

- 注册表是一个非符号链接、根用户拥有的常规文件，对组和其他用户不可访问；
- 注册表、应用程序和数据父目录由root拥有，并且不是组/世界可写的；
- 包、清单、入口点和静态运行时由根拥有，并且不是组/世界可写的；
- 每个入口点路径组件都是实际的目录而不是符号链接；
- `cp0-app-N` 解决为注册表中存储的精确稳定 UID/GID；
- 私有数据目录由该 UID 所有，并且没有组或其他的模式。

最多可以运行一个应用单元。第二次启动会被拒绝而不是隐式终止当前应用。Stop 会从注册表中获取稳定的应用单元名称而无需重新打开包内容，因此如果应用的包变得损坏，仍然可以终止运行中的应用。

## 控制协议

systemd 拥有 `/run/cardputerzero-appd/control.sock`。一个 tmpfiles 规则创建父目录为 `root:cp0-control 0750`；套接字是 `root:cp0-control 0660`。只有 `cp0-shell` 属于 `cp0-control`。在 DAC 成功后，`appd` 检查 Linux `SO_PEERCRED` 并只接受 UID 0 或解析后的 `cp0-shell` UID。

协议v1使用每个连接一个换行符终止的JSON请求。两个方向都限制在8 KiB，拒绝未知字段并使用请求ID。支持的命令是`ping`, 分页的`list`（最多8条记录）、`start`和`stop`。内部文件系统和命令错误会被记录但不暴露给客户端。

Launcher list record 还只暴露规范 manifest name 和 standard/immersive display policy。可信 Shell 请求八项一页的内容，启动选中的 stopped 应用，再等待 compositor 的临时 surface token 后激活。从 Tasks 停止应用会保留已安装 registry entry。

`cp0ctl app ping|list|start|stop` 是诊断客户端。System Shell 在权限提示和应用启动 UI 集成后使用同一 contract。

## V0.6 验证

服务和套接字在无需重启或刷写的情况下进行了热部署。

- 普通 `pi` 账户在打开控制套接字时收到了 `EACCES`。
- `cp0-shell` 完成了协议 `ping`, 分页列表，启动和停止请求。
- 运行的 Hello 单元使用 UID/GID 20000，大约 9.0 MB 内存，三个任务，
`MemoryMax=24M` 和 `MemorySwapMax=0`。
- 宿主机的 bubblewrap 监控器保持在应用程序命名空间之外；
bubblewrap PID 1 和沙盒内的 App Runtime PID 2 都没有 `/usr`，
使用了独立的 PID/网络命名空间，并报告了 `NoNewPrivs=1` 以及 seccomp 模式 2。
- 停止后，appd、compositor 和 System Shell 仍然活跃，而 app 单元则不活跃/被收集。

部署的开发制品使用 `cp0-app-20000` 并在注册表 `/var/lib/cardputerzero/registry/apps.json` 中以模式 `0600` 运行。
