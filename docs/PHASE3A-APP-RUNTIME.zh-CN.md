# Phase 3A: App 运行时和 Linux 沠板

<!-- doc-locale: zh-CN -->
> [English](PHASE3A-APP-RUNTIME.md) | **简体中文**

## 范围

第3A阶段建立了SDK下方的可执行隔离边界。后续工作增加了通知代理和服务描述中提到的受控显示通道。`PHASE3D-CONTROLLED-DISPLAY.md`

可信的 App 运行时嵌入了 WAMR 2.4.5 版本在提交
`25bd7eb63e828e4bd242cc9b38d260b4b31c6605`目标构建是一个静态的 aarch64 可执行文件，启用了解释器和 AOT 加载。JIT、WASI、本地 libc 导入、线程、共享内存、SIMD 和多模块加载被禁用。因此，第三方模块没有环境文件系统、套接字或进程 API。

在读取已验证的模块文件后，Runtime 在 WAMR 解析或实例化不受信任的字节之前安装一个 aarch64 seccomp 允许列表。过滤器允许内存管理、时钟、信号和通过预打开描述符进行的通信。它拒绝 `open`, `openat`, `socket`, `connect`, mount, 命名空间、进程创建和带有 `EPERM` 的 ptrace 系统调用。

## 构建

WAMR 源代码检出和所有构建输出保持在仓库的忽略 `target/` 目录下：

```sh
make app-runtime
make example-app
make malicious-apps
```

`scripts/build-app-runtime.sh` 拒绝 WAMR、Wayland、wayland-protocols 或 libffi 的检出，除非其 HEAD 与固定提交一致。它验证生成的 ELF 是 aarch64，并且没有动态库依赖。

## 沙盒合约

`cp0-appd plan` 将静态运行时与以下内容结合：

- 一个稳定的`cp0-app-N`主账户；
- 一个 transient 的 systemd 服务和 cgroup v2;
- 一个systemd冲突与明确的24小时稳定性接受服务，
所以启动任何应用程序会终止并无效化空闲接受；
- `MemoryMax` 等于清单预算和 `MemorySwapMax=0`；
- 一个空的 bubblewrap 根, PID/mount/network/IPC/UTS/cgroup 命名空间；
- 一个只读包在`/app`和运行时在`/runtime`；
- 一个空的命名空间本地 `/data`；持久化数据仅可通过私有存储SDK代理访问。
- 一个空的私有 `/dev`, 私有 `/tmp`, 没有宿主 `/usr`, `/run` 或 D-Bus.

外部单元仅允许 `AF_NETLINK`，因为 bubblewrap 在构建其私有网络命名空间时需要 `NETLINK_ROUTE`。运行时 seccomp 策略拒绝 `socket()`，因此这个功能对运行中的应用程序不可用。`ProtectKernelTunables=yes` 由于记录在 ADR 0005 中的命名空间兼容性原因而故意省略。

## V0.6 验证

在使用512 MB V0.6设备上进行验证时，使用了Debian 13，内核`6.18.34+rpt-rpi-v8`，systemd 257和bubblewrap 0.11.0。

- 一个最小的WASM模块通过完整的systemd、bubblewrap、seccomp和WAMR路径完成，并且状态为0。
- 成功的单元峰值为9.3 MB，并未使用交换空间。
- 原始 seccomp 负探针确认了 `openat`，IPv4 `socket`、`mount` 和 `ptrace` 均返回 `EPERM`。扩展探针还涵盖了路径逃逸、设备节点、IPv6/netlink/socketpair、进程创建、执行和信号处理；其下一次物理运行将等待活跃的24小时稳定性监控。
- 一个分配了40 MB线性内存的模块被systemd结果终止 `oom-kill`; cgroup 最高达到了恰好 24 MB，并且使用了 0 字节的交换空间。
- `cardputerzero-compositor.service` 和
  `cardputerzero-system-shell.service` 在每次探测后仍然保持活动状态。

不需要重建或刷写镜像。开发生成文件安装在 `/usr/libexec/cardputerzero/app-runtime` 和根所有者 Hello 包路径；
稳定的测试身份是 `cp0-app-20000`（UID/GID 20000）。
