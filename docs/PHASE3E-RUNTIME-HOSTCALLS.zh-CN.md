# Phase 3E: 运行时能力宿调用

<!-- doc-locale: zh-CN -->
> [English](PHASE3E-RUNTIME-HOSTCALLS.md) | **简体中文**

## WAMR 边界

第一个实现的SDK导入是：

```c
int32_t cp0_post_notification(
    const uint8_t *title, uint32_t title_length,
    const uint8_t *body, uint32_t body_length);
```

它在WAMR中注册为`(*~*~)i`。WAMR将两个偏移量转换为本地地址，并在受信任的Runtime代码接收之前验证每个长度是否符合应用程序的线性内存。Runtime还对标题和正文的字节长度进行限制，拒绝ASCII控制字符，并执行有界JSON字符串编码。无效的UTF-8或字符计数溢出由Rust broker协议解析器拒绝。

主机调用返回SDK错误值：成功、拒绝、不可用（包括待用户响应的提示）、无效参数、资源限制或内部失败。它从不暴露代理文件描述符、应用程序标识或权限决策API给WASM。

## 运行时系统调用封装

aarch64 seccomp 程序现在只允许参数 0 为 `AF_UNIX` 的 `socket` 调用。创建 IPv4、IPv6
或 netlink 套接字仍返回 `EPERM`。`connect` 只能访问空 bubblewrap 根目录中可见的路径，
其中唯一的套接字是 `/run/cardputerzero/broker.sock`。发送和接收沿用现有 I/O allowlist，
一秒的套接字超时可防止停滞的能力代理服务无限期阻塞应用。

外部应用单元仍然限制地址族为 Unix 和 netlink；netlink 只有在 bubblewrap 创建私有网络命名空间时才需要，并且被 Runtime 的 argument-filtered `socket` 规则所禁止。

## V0.6 验证

更新无需重启或刷写即可热部署。

- 静态运行时 SHA-256：
  `8cb76b9e34309a5a85adb0999d132d8a2eaf50975ea66854d75dc407cd9aeccd`.
- Seccomp 探针 SHA-256：
  `54976a1afb29b61782d39d640753e880c23dc540a77ade7598d2261cda94c9e0`.
- Hello WASM SHA-256：
  `d1830261bec651deb3cabc35f05e8bf524a97fd136c61b9cefc68da87d91eff6`.
- 探针确认在允许 `AF_UNIX` 后，禁止的系统调用检查仍然通过；IPv4 套接字仍然被拒绝，而 Unix 套接字成功。
- Hello 调用了`cp0_post_notification`从WASM，保持活跃并生成了SDK集成运行中的通知ID 4。Shell 控制通道接收到标准应用ID/名称，标题`Hello Card`和正文`Runtime host call is active`。
- Hello 停止得干净利落，而 appd、compositor 和 System Shell 仍然保持活动状态。
