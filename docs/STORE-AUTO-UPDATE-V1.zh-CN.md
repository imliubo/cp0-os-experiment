# Store 自动应用更新 v1

<!-- doc-locale: zh-CN -->
> [English](STORE-AUTO-UPDATE-V1.md) | **简体中文**

S7F 添加了一个明确启用的、失效关闭的自动更新路径，适用于经过审核的应用程序。它不添加无人值守的应用程序安装：只能考虑已安装的应用程序，并且默认用户偏好是关闭的。

## 政策和偏好

`/etc/cardputerzero/device-policy.json` 是独立的
`store_auto_update_allowed` 上限。该字段对于策略v1是可选的，因此部署的策略仍然可读，但省略的字段会解码为 `false`产品的政策文件中明确规定了这一点。 `store_install_allowed` 并且应用白名单仍然适用。

用户的偏好由`cp0-stored`拥有，并存储在`/var/lib/cardputerzero/store/auto-update.json`。缺少文件表示禁用。
该文件是一个严格的、有界的模式，必须是一个实际的私有文件，由服务UID拥有，并通过原子重命名替换为`0600`、`fsync`和父目录`fsync`。它仅包含启用位和最后检查时间。

Store IPC 增加以下严格命令：

```json
{"name":"get-auto-update"}
{"name":"set-auto-update","enabled":true}
{"name":"run-auto-update"}
```

状态返回 `enabled`、`policy_allowed`、`charging`、`unmetered_network`、`due` 和 `checking`。System Shell 在 `Settings -> Apps & Privacy -> Auto App Updates` 提供开关；locked、checking、waiting for power、waiting for wired、due 和 enabled 状态适配 320x170 四行视图。

## 调度和网络网关

守护进程最多每六小时检查一次。最后一次检查时间在开始网络工作之前写入，因此失败的端点不会导致请求或SD写入循环。向后的时间跳动允许一次新的检查，然后记录较低的时间。启用该偏好设置可能会立即启动一次应检查；否则，守护进程调度器每分钟评估一次。

每次自动检查都需要两者：

- 在线外部电源，或电池报告充电、充满或未充电
- 一个主表默认路由，其输出接口是以太网，有载波，并且没有Linux无线标记。

路由直接从有界的`NETLINK_ROUTE`快照中读取。Wi-Fi 被保守地视为不合格，直到操作系统有一个可信赖的计量网络源。在目录刷新后和发布队列前再次检查条件。

## 候选人和安装边界

`cp0-stored` 使用一个专用分页 appd 命令，该命令仅返回已安装的 App ID、版本和声明的权限。Store UID 仍然无法使用正常的启动器列表、设置、生命周期、日志、开发者安装或代理命令。

从刚刚下载并验证的目录中，守护进程按规范 App ID 顺序选择最多八个应用程序。候选者必须：

- 已经安装；
- 在目录中具有严格较大的 SemVer；
- 请求一个权限集，它是已安装权限集的子集；
- 通过当前 Store 切换，自动更新切换，允许列表，存储，
  仓库标识，签名，校验摘要和大小检查。

新的应用、相同版本、降级以及任何新的权限都不是自动候选。现有的digest命名的恢复点、串行队列、失败、暂停、取消和交接恢复路径会被重用。

最终的 appd 手递明确标记为自动。appd 重新加载其活动策略边界，并在重新验证 SemVer、包字节、清单标识、SDK 兼容性、两个签名和精确摘要之前需要独立的自动更新权限。暂停的自动任务保留此模式，因此重新启动不能将其转换为手动策略安装。

## 验证

Rust 测试覆盖了遗留策略的 fail-closed 行为、私有偏好重启持久化、六小时的速率限制、电源/网络门、严格的权限子集选择、版本过滤、自动 appd 手动传递，以及 Store UID 命令隔离。C 测试覆盖了精确的响应形状和不一致状态的拒绝。UI 行为和 320x170 等待电源像素快照是系统 Shell 门的一部分。AArch64 编译覆盖了 Linux Netlink 实现。
