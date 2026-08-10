# Phase 3F: 受限的HTTPS客户端代理服务

<!-- doc-locale: zh-CN -->
> [English](PHASE3F-NETWORK-BROKER.md) | **简体中文**

## 范围

`network.client` 提供一个同步且带限制的 HTTPS GET 操作。它不是一个原始套接字 API，并且不向应用程序暴露 DNS、TCP、TLS 配置、请求头、流式主体或监听套接字。

第一个ABI故意很窄：

- URL：仅 HTTPS，最多 1024 个 UTF-8 字节，无控制字符；
- 重定向限制：两次，并再次应用HTTPS验证和DNS策略；
- 全局超时：五秒包括DNS解析、重定向和响应读取；
- 响应体：最多2048字节，以不透明字节返回；
- 响应元数据：仅HTTP状态码；
- 代理服务和网络服务框架：以换行符分隔的严格JSON，4096字节。

Rust 和 C/C++ 的 SDK 使用调用者拥有的响应缓冲区。Runtime 主机调用在一个打包的 64 位值中返回状态和主体长度，因此 WAMR 可以在受信任的本地代码运行之前验证两个指针/长度对。

## 信任边界

应用程序Runtime仍然受限于`AF_UNIX`。它向现有的appd能力代理服务发送`http-get`。appd从`SO_PEERCRED`中获取应用程序身份，验证对端PID是否在活动应用程序的systemd cgroup中，加载根拥有的清单并评估`network.client`。应用程序从未提供自己的身份。

授权后，appd 释放其共享生命周期/权限互斥锁，并通过根用户唯一的 Unix 套接字转发请求。因此，缓慢的网络操作无法阻塞启动器列表/启动/停止，受信任的权限 UI 或通知检索。

只有 `cp0-networkd` 拥有 `AF_INET` 和 `AF_INET6`。它以无特权的 `cp0-network` 账户运行，没有任何能力，没有设备访问权限，cgroub 限制为 24 MiB，并且任务限制为八任务。其激活的套接字仅接受 UID 0，匹配当前根 appd 服务。

## SSRF 和 绑定策略

`cp0-networkd` 禁用所有环境代理发现，并在 HTTPS 传输中安装自定义解析器。每个连接目标，包括每个重定向目标，都减少为解析器批准的套接字地址列表。连接器永远不会收到被拒绝的地址。

该策略拒绝IPv4和IPv6环回、私有/唯一本地、链路本地、多播、未指定、保留、文档和基准测试范围。它还拒绝IPv4映射IPv6、站点本地IPv6、Teredo、众所周知/本地使用NAT64前缀以及其他IPv4兼容的IPv6形式。返回混合公共和被拒绝地址的主机名只能使用其公共结果。返回没有公共地址的主机名会失败并返回`blocked-address`。

TLS 使用 Rustls 与 WebPKI 根证书。证书验证不能被禁用。
HTTP 重定向不能降级为明文，因为代理为完整的重定向链配置了 `https_only`。

## 稳定的 SDK 行为

SDK 将能力拒绝和被阻止的目的地映射到 `Denied`，待授权或暂态 DNS/TCP/TLS/超时故障映射到 `Unavailable`，无效的 URL 映射到 `InvalidArgument`，以及过大的响应映射到 `ResourceLimit`。成功的调用返回 `{ status_code, body_length }`；响应体占据调用者缓冲区的前缀，并可能包含任意字节。

Hello Card 声明了两个 `notifications.post` 和 `network.client`. 按下 Linux `KEY_N` 键仅在用户操作后请求 `https://example.com/`。绿色、黄色、红色和洋红色指示灯分别表示 2xx 响应、待处理/不可用、拒绝/非 2xx 和内部故障。

## 验证

本地覆盖包括：

- 严格请求/响应解析，标准 base64 和最大帧测试；
- 公共地址策略测试IPv4、IPv6、映射和过渡范围；
- 直连前的字面环回和明文HTTP拒绝；
- 一个可选的公开 DNS，Rustls 证书和 HTTPS 响应测试；
- 网络服务成功/错误分发；消息已清理；
- appd-to-networkd Unix协议交换和最大代理响应大小；
- C 运行时二进制响应解码和稳定错误映射；
- Rust 和 freestanding C11/C++17 SDK 的编译测试；
- 本地工作区测试和AArch64 appd/networkd/Runtime 构建。

在实施这一阶段时，24小时Phase 2G稳定性接受已经生效。在那次运行中没有替换任何核心服务。设备HTTPS、权限提示、私有地址拒绝和cgroup内存测量必须在监控完成后记录。
