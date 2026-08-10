# Phase 3N: 恶意应用回归集

<!-- doc-locale: zh-CN -->
> [English](PHASE3N-MALICIOUS-APPLICATIONS.md) | **简体中文**

负向测试集使应用程序隔离合同变得独立可重复，而不仅仅依赖于一个成功的示例应用程序。

## 样本

`memory-hog.wasm` 提交 40 MiB 的线性内存并验证清单，内存控制组在不换页或不损坏 Shell 的情况下终止它。这个示例已经在 V0.6 硬件上被接受。

`ambient-authority.wasm` 故意绕过 SDK 并导入 WASI `path_open` 和 `sock_open`。构建测试检查二进制 WebAssembly 导入表，并将这些导入修复为预期的恶意行为。生产 Runtime 内置 WAMR libc 和 WASI 未启用，因此模块无法实例化这些导入；第三方模块没有文件系统或套接字 syscall ABI。

`path-escape-app.json` 使用 `../etc/passwd.wasm` 作为其入口点。表征解析器在注册或启动规划之前就拒绝它。生命周期测试也拒绝根拥有安装包内部的符号链接和身份变更。

## Runtime 被攻陷后的边界

WASM 隔离并不是唯一的边界。生成的 bubblewrap 计划测试后暴露了恰好三个只读主机源：可信的 Runtime、选定的不可变包和唯一的 appd 能力代理服务套接字。`/dev` 是一个新的私有设备树。没有主机`/usr`、D-Bus、Wayland 路径、DRM、输入、GPIO、ALSA 或其他应用程序的数据挂载。连接的 Wayland 流由 PID 1 打开，并作为唯一的`OpenFile`描述符传递。

静态 aarch64 seccomp 探针现在验证拒绝：

- `/etc` 和 `/proc/self/root` 路径访问；
- DRM, 输入, gpiochip 和 ALSA 设备打开；
- IPv4，IPv6 和 netlink 套接字以及`socketpair`；
- 挂载，ptrace，clone，exec 和进程信号。

一个 AF_UNIX 套接字仍然可用，因此可信运行时可以到达唯一挂载的能力代理端点。应用程序本身无法调用 `socket`，因为缺少 WASI 和本地 libc 导入；即使被篡改的运行时在其挂载命名空间中也看不到其他主机套接字路径。

每个瞬态应用单元也携带固定的60% CPU配额和较低的CPU权重。与现有的内存、交换空间和任务限制一起，这可以防止忙碌循环的WASM或被破坏的Runtime独占CM0的单个CPU。在受信任的Runtime中，显示提交独立地被限制在每秒30帧，因此重复的有效帧提交无法通过淹没 compositor 来绕过CPU控制。

## 验证

`tests/test-malicious-apps.sh` 重建了两个WASM示例，检查导入项并验证恶意清单被拒绝。`make app-runtime` 交叉编译扩展的静态seccomp探针。重新运行该aarch64探针和现有内存-cgroup示例将在V0.6上延迟到活跃的24小时稳定性监控完成。
