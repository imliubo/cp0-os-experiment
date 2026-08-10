# Store 更新队列 v1

<!-- doc-locale: zh-CN -->
> [English](STORE-UPDATE-QUEUE-V1.md) | **简体中文**

S7B 添加了一个显式的、有界的所有更新操作，而不会引入并行下载或削弱用于安装的签名目录标识。单个应用程序安装和一个更新所有批次使用同一个守护进程拥有的序列化工作线程。

## 协议

协议版本 1 添加了严格命令：

```json
{
  "name": "install-batch",
  "authorization_id": 91,
  "app_ids": [
    "dev.cardputerzero.alpha",
    "dev.cardputerzero.beta"
  ]
}
```

S7D 要求此命令跟随相同的目录序列和相同的 ID 列表的成功 `preflight-install` 命令。授权是一次性的，并且还绑定版本、包校验和、大小、权限、策略和容量；参见 `STORE-INSTALL-PREFLIGHT-V1.md`。

`app_ids` 包含按字节递增顺序排列的1到8个有效的应用ID。
空的、过大的、重复的、未排序的、无效的或未知的ID将拒绝整个请求。成功的响应保留请求的确切顺序，并将每个接受的ID绑定到目录版本快照：

```json
{
  "kind": "install-batch-accepted",
  "apps": [
    {"app_id": "dev.cardputerzero.alpha", "version": "2.0.0"},
    {"app_id": "dev.cardputerzero.beta", "version": "3.1.0"}
  ]
}
```

Rust、CLI 和 C 客户端拒绝部分、重排序、重复或损坏的响应。接受是全或无的：在持有 Store 状态锁时，`cp0-stored` 验证每个 ID 对应最新的验证目录，快照每个版本和包的 SHA-256，预留全局变更任务，并将每个初始操作发布为 `queued`。在那一步原子接受之前，不会开始任何下载。

## 串行队列

守护进程拥有一个工人，并按请求顺序处理接受的应用程序。它保留全局变更预留状态，直到每个队列条目变为终端状态，因此刷新、媒体缓存变更、另一个安装和恢复不能在队列中交错不同的目录或包标识。

每个条目仍然有自己的S7A控制状态。暂停只保留该条目的以摘要命名的部分文件；取消只删除该条目的部分文件；网络、存储、验证或安装程序故障只记录在该条目上。然后工作者前进到下一个排队的应用程序。暂停的条目可以在当前批次释放全局任务后重新启动。取消未开始的条目是协作的，并在包数据传输前被观察到。

单个项目的`install`是通过相同的批次接受和工人路径实现的。这保持了忙碌行为、失败分类、控制竞态和目录绑定在单个项目和更新所有操作之间一致。

## System Shell

320x170 更新页面通过 `update`, `queued`, `downloading`, `paused`, `installing`, `failed` 和 `canceled` 状态显示每个具有较新目录版本的应用程序。一个单独的 `UPDATE ALL N` 命令行选择最多八个当前符合条件的条目：

- `update`
- `failed` 有可用更新
- `canceled` 有可用更新

激活的 `queued`, `downloading`, `paused`, 和 `installing` 条目不再重新提交。从“更新全部”下拉选择第一个应用程序行；从该行向上返回到“更新全部”。进入一个应用程序仍然打开其单独的详细信息和 S7A 控制。过时的目录继续渲染命令和更新行，但阻止“更新全部”。当不再有符合条件的条目时，该命令失去选择，因此激活的应用程序行仍然可导航。

C客户端API是：

```c
int cp0_store_install_batch(
    const char *const app_ids[],
    size_t app_count);
```

操作员可以在CLI中使用排序的规范化解析来行使相同的守护进程路径：

```sh
sudo cp0ctl store install-batch \
  --approve-permissions \
  dev.cardputerzero.alpha \
  dev.cardputerzero.beta
```

自动更新仍然禁用。每次批次都是显式的本地用户或操作员动作。

## 验证

协议测试涵盖空批次、超大批次、重复批次、未排序批次、畸形批次和身份不符批次。守护进程测试使用三个不同的包和屏障来证明原子接受、顺序排序、全局突变所有权、每项暂停和取消清理、终端状态后的继续以及后续恢复。

C 测试绑定响应的数量、顺序、ID、版本和精确对象形状。UI 行为测试覆盖八项绑定、活动项排除、失败/取消的包含、过时项拒绝、选择重置以及单独的详细信息。320x170 像素的快照验证命令行和更新列表。
