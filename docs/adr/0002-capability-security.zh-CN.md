# ADR 0002：能力权限与双层沙箱

<!-- doc-locale: zh-CN -->
> [English](0002-capability-security.md) | **简体中文**

- 状态：Accepted
- 日期：2026-07-30

## 决策

应用默认没有网络、共享文件、媒体输入或硬件权限。应用只能通过 SDK host call 请求
manifest 中声明的能力。WASM 运行时之外再使用 Linux UID、namespace、seccomp、
cgroup 和最小挂载视图限制承载进程。

系统根据已验证包 ID 和进程凭据识别调用者，不信任应用提交的身份字符串。设备节点
只对硬件 broker 开放；共享文件只通过 Document Portal 的文件描述符传递。

## 理由

单独使用容器仍允许过大的 Linux syscall 面，单独使用 WASM 又会把运行时漏洞直接
暴露给系统。两层边界可以降低任一实现缺陷造成完整突破的概率，并让权限控制统一
落在少数可信服务中。

## 后果

所有硬件功能都必须先设计 broker API。权限 API 变更属于 SDK 兼容性变更，需要
版本化、审计和恶意调用测试。
