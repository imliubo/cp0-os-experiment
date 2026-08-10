# Store 扫描工作者

<!-- doc-locale: zh-CN -->
> [English](STORE-SCAN-WORKER.md) | **简体中文**

`cp0-store-scan-worker` 消费最终化的提交事件，并仅从前端 `processing` 移动到 `ready-for-review` 的验证内容。它是一个后台服务，并且永远不会包含在 CardputerZero 设备镜像中。

## 信任边界

工人不执行提交的WebAssembly。`cp0-store-scan`执行有界结构解析并验证精确的开发者签名的`.capp`、清单、SDK版本、主机导入、声明的权限、Store列表和PNG资产。列表默认区域必须匹配永久的应用程序注册表。包签名密钥和重新计算的指纹必须匹配App团队当前活动的密钥。密钥撤销在PostgreSQL中是一次性的。

对象读取器只接受数据库拥有的小写SHA-256标识符。它使用`O_NOFOLLOW`以只读方式打开块文件，验证文件类型和大小，重新散列每个块，重构连续部分，然后验证每个完整部分和最终的提交内容摘要。扫描器发现的结果是稳定的代码，并且最多为16位；解析器文本和文件系统路径从不保存在报告中。成功的报告还携带了在`STORE-RISK-POLICY.md`中定义的版本化确定性风险评估。

## 交付和恢复

最终化将`submission.scan-requested`写入事务输出队列。一个
工作者将一个事件移动到`submission_scan_jobs`，标记事件已分发，并以60秒的租约获取任务。扫描不持有数据库事务。随后的一个短的可序列化事务锁定租约和提交，插入一个只读结果和其风险评估，更新资源版本，并原子地写入审计和`submission.scan-completed`输出队列记录。

过期的租约会被返回到队列中，除非第八次索赔被耗尽，这时任务变为`failed`。对象或提交失败也会在最多八次索赔内重试；失败会使提交变为`processing`，等待操作员修复。唯一的事件和提交结果防止两个工人完成相同的扫描。

## 隔离配置

参考单位是
`crates/cp0-store-scan-worker/systemd/cp0-store-scan-worker.service`. 它在具有只读对象根、无设备、无IP网络命名空间、无权限、无可执行可写内存、任务/CPU/内存限制，并且只有
`AF_UNIX`. 因此，PostgreSQL 必须通过本地Unix套接字暴露，数据库角色仅限于工作表和必要的提交、对象描述符、审计和出箱语句。

环境文件需要：

- `CP0_STORE_DATABASE_URL` 使用 PostgreSQL Unix 套接字；
- `CP0_STORE_OBJECT_ROOT`，匹配控制服务器对象根；
- `CP0_STORE_SCAN_WORKER_ID`, 一个稳定的3-64字节服务标识。

`CP0_STORE_SCAN_ONCE=1` 至多执行一次受控任务的轮询和测试。
默认进程每 500 毫秒轮询一次，并支持 `SIGINT` 关机。

## 验证

```sh
cargo test -p cp0-store-scan -p cp0-store-scan-worker
cargo clippy -p cp0-store-scan -p cp0-store-scan-worker --all-targets -- -D warnings

# Requires a disposable PostgreSQL 17 database.
CP0_STORE_TEST_DATABASE_URL=postgres://... make store-control-db-check
```

数据库闸覆盖精确单一完成，并发工作者索赔，
过期租约恢复，活跃/失效开发者密钥，缺失对象，有界重试耗尽，
只追加结果，不可变任务和控制服务器迁移兼容性。

本切片不提供动态恶意软件签名、外部声誉服务、生产队列基础设施、审阅员决策、Store签名、目录发布或透明日志记录。
