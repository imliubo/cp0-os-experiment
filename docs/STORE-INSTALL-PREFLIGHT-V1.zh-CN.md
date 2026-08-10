# Store 安装 预检 v1

<!-- doc-locale: zh-CN -->
> [English](STORE-INSTALL-PREFLIGHT-V1.md) | **简体中文**

S7D 要求在接受 Store 下载前强制检查权限同意、设备策略和存储容量。System Shell 是可信同意界面；应用不能连接 `cp0-stored`，root 仍是显式管理权限主体。

## 两步协议

Shell 首先提交精确验证过的 Catalog 序列和一个到八个排序的应用程序 ID：

```json
{
  "name": "preflight-install",
  "app_ids": ["dev.cardputerzero.example"],
  "catalog_sequence": 42
}
```

守护进程会拒绝过期或不匹配的 Catalog、未知或仍在运行的应用、已禁用的 Store、设备
allowlist 之外的应用，以及不足的持久存储或交接容量。成功时返回一次性授权、准确的签名
身份和权限：

```json
{
  "kind": "install-preflight",
  "authorization_id": 91,
  "catalog_sequence": 42,
  "required_bytes": 50331648,
  "available_bytes": 201326592,
  "apps": [{
    "app_id": "dev.cardputerzero.example",
    "version": "2.0.0",
    "permissions": ["camera.capture", "network.client"],
    "policy_denied_permissions": ["camera.capture"]
  }]
}
```

Shell 将每个返回的 ID、版本和权限位与当前 Catalog 视图进行比较。如果安装或更新增加了权限或策略阻止了任何请求的权限，它会呈现一个受信任的确认提示，默认选择是取消。最终请求包含授权 ID：

```json
{
  "name": "install",
  "app_id": "dev.cardputerzero.example",
  "authorization_id": 91
}
```

`install-batch` 携带相同的授权 ID 和预飞行 ID 列表。授权在 60 秒后失效，在首次安装尝试时被消耗，并且不能重放。在发布队列状态之前，守护进程再次检查策略、容量、目录序列、完整的目录应用对象、版本、包摘要、大小和权限。不同的 ID、重新排序的批次、后来的目录、过期的授权或重放将被拒绝。

## 政策行为

`/etc/cardputerzero/device-policy.json` 仍然是 root 所有的上限。
预加载通过与 appd 使用相同的严格模式和安全文件检查来加载它。`store_install_allowed=false` 和允许列表的缺失拒绝在发生网络或存储变更之前。appd 在原子传递时重复 Store/允许列表 的强制执行，因此 Store 客户端不能削弱最终安装边界。

全球被拒绝的SDK权限被返回为排序后的被请求权限的子集。它们不会使包字节变得不安全，因此不会阻止安装，但Shell会将它们标记为策略阻塞。即使在先前的应用程序级决策允许这些权限的情况下，appd仍然会在任何运行时用户权限提示之前拒绝这些能力。

## 能力模型

持久数据检查为系统操作预留了16 MiB，为每个接受的应用完整新提取版本，以及每个不在有效摘要命名部分文件中的包字节。完成的包文件仍然可供验证和重试使用，因此这是一个保守的批次结束状态约束，而不仅仅是下一个网络块。

appd 入箱单独检查，因为它通常位于 `/run`。它必须容纳批次中最大的包加上 8 MiB 的额外空间。两个检查都使用未受特权 `cp0-store` UID 可用的文件系统块。缺少、符号、公共、过大或非常规的部分文件应闭合而不是减少估算。

预检返回持久的`required_bytes`和`available_bytes`用于确认UI。`insufficient-storage`与网络、验证、安装程序、策略和目录失败不同。恢复保留其原始批准的目录身份，但在返回到队列状态之前重复当前策略和两个容量检查。

## 用户和操作界面

320x170确认显示应用程序数量、新请求的不同权限数量、被策略阻止的不同权限数量以及有界必需/免费存储值。安装和取消使用固定尺寸；取消选项最初被选中。策略、存储、目录或服务预飞行错误使用封闭的信任消息，从不显示守护进程控制的文本。

`cp0ctl` 需要显式的操作器断言，并执行相同的列表/预检/授权安装序列：

```sh
sudo cp0ctl store install dev.cardputerzero.example --approve-permissions
sudo cp0ctl store install-batch --approve-permissions \
  dev.cardputerzero.alpha dev.cardputerzero.beta
```

## 验证

Rust 测试覆盖有界严格请求/响应解析、Catalog sequence 和完整对象绑定、精确的策略拒绝权限子集、Store 策略拒绝、容量不足、错误授权、成功消费和重放拒绝。C 测试拒绝不匹配的 sequence、ID、权限、策略子集、形状和错误词汇。UI 测试覆盖默认取消、明确确认、错误驳回和 320x170 确认/存储快照。最终的 AArch64 System Shell 将警告视为错误。
