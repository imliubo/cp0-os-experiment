# Store 下载控制 v1

<!-- doc-locale: zh-CN -->
> [English](STORE-DOWNLOAD-CONTROL-V1.md) | **简体中文**

S7A 在不削弱目录、包或 appd 验证的情况下，增加了 Store 安装的用户可见的控制范围。设备仍然一次只运行一个 Store 变异任务。

## 协议

协议版本 1 添加了严格命令：

```json
{
  "name": "control",
  "app_id": "dev.cardputerzero.example",
  "action": "pause"
}
```

`action` 是 `pause`, `resume` 或 `cancel`. 成功返回
`operation-accepted` 绑定到请求的应用 ID 和操作，以及操作版本。接受表示请求已被记录；客户端通过检查目录摘要来获取最终状态。

目录摘要添加 `paused` 和 `canceled` 状态。只有 `failed` 摘要
携带一个 `failure_reason`，选择自此封闭词汇表：

- `network`
- `storage`
- `verification`
- `installer`
- `catalog-changed`
- `internal`

未知字段、状态、动作、原因、不一致的进度、缺少失败原因或非失败摘要上的原因都会被 Rust 和 C 客户端拒绝。`invalid-state`与`busy`不同：前者表示该动作不能应用于操作，而后者表示先前被接受的动作尚未达到其终端状态或另一个 Store 变动拥有该任务。

## 状态和文件生命周期

守护进程在接受安装之前记录应用程序版本和签名包的SHA-256。工作者在传输前、每次限制下载块以及在appd移交前立即检查协作控制。

```text
available/update -> queued -> downloading -> installing -> installed
                         |          |
                         +----------+-> paused -> queued (resume)
                         |          |
                         +----------+-> canceled
                         |
                         +------------> failed

paused/failed -> canceled
canceled/failed -> queued (new install/retry)
```

暂停会保留名为digest的私有`0600` `.part`文件，并报告进度限制。恢复只在暂停时被接受，并且当前验证过的目录仍然具有相同的应用程序版本和包digest。网络重试可以重用相同的digest绑定字节；包验证会在报告失败前截断不良的完整下载。

取消排队/下载是协作的。取消从暂停/失败保留全局Store任务，同时同步移除`.part`，因此并发恢复、重试、刷新或媒体任务不能竞争清理。清理失败变为`failed/storage`；成功变为`canceled`。一旦状态为`installing`，暂停和取消将被拒绝，因为appd拥有原子安装移交。被接受的取消是单调的：后续的暂停不能在工作者到达下一个控制边界前替换它，而重复的取消请求保持幂等性。

一个验证过的目录刷新会协调可恢复的操作。如果暂停或失败操作的版本或摘要不再匹配，该操作仍然可见为`failed/catalog-changed`：恢复被拒绝，取消可以移除旧的摘要文件，并从新的签名身份开始新的安装。已完成的操作不会被误报为目录失败。

## System Shell

320x170 详细概览暴露一个主要操作，并在有效时暴露一个取消操作：

| 状态 | 主要 | 次要 |
| --- | --- | --- |
| 可用/更新 | 安装 | - |
| 排队/下载中 | 暂停 | 取消 |
| 暂停 | 恢复 | 取消 |
| 失败/取消 | 重试 | 仅失败取消 |
| 安装/已安装 | - | - |

Up/Down 选择操作，Enter 执行。stale Catalog 仍允许暂停和取消，因为它们减少活动，但会阻止安装、恢复和重试。更新成员资格独立于已安装版本推导，因此即使处于 queued、downloading、paused、failed 或 canceled 状态，更新仍显示在 Updates 页面。Catalog 轮询是权威状态；daemon 重启不能让陈旧的本地暂停状态卡在 System Shell 中。

Shell 不显示关闭失败原因（`FAIL NETWORK`, `FAIL STORAGE` 等）而只渲染这些原因。CLI 操作员有相同的限制路径：

```sh
sudo cp0ctl store pause dev.cardputerzero.example
sudo cp0ctl store resume dev.cardputerzero.example
sudo cp0ctl store cancel dev.cardputerzero.example
```

## 验证

协议测试涵盖严格的控制命令、接受响应绑定、新状态、进展和失败原因一致性。守护进程测试使用屏障来证明暂停确认、恢复忙行为、精确的部分重用、协作取消删除、appd 手递拒绝、目录摘要变化，以及稳定的网络/存储/验证/安装器故障分类。

C 测试覆盖所有新状态、畸形失败原因以及动作绑定的接受响应。UI 行为测试覆盖正常和过时的 Catalog 动作、权威守护进程合并、更新成员资格、选择重置和失败渲染。像素快照验证精确 320x170 下的紧凑下载和失败详细布局。
