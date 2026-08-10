# Store Publication 透明日志

<!-- doc-locale: zh-CN -->
> [English](STORE-TRANSPARENCY.md) | **简体中文**

S5H 为每个提交的目录快照添加了一个有界追加只读透明记录。其目的是使发布历史独立重算：发布、暂停、恢复和移除一个版本都会产生一个叶子节点，最新的签名检查点会提交到每个先前的叶子节点。

这是后台发布基础设施。它不是设备镜像的一部分，并且不会替代设备执行的签名目录或`.capp`验证。

## 对象和顺序

`cp0-store-transparency` 定义了标准 JSON v1 对象。一个叶绑定：

- 连续的基于零的透明树索引；
- 单调的 Catalog 序列，摘要和字节长度；
- Store 签名密钥 ID 和 发布时间戳；
- 源事件，Release，任务类型和Release状态。

目录序列在保留发布被取代或失败时可能会包含故意的间隙。这样的任务不会创建快照或透明度叶。树索引保持连续，因此目录序列`1, 2, 4`映射到树索引`0, 1, 2`而不重用序列`3`。

每次成功的快照还会创建一个检查点，包含树的大小、Merkle根、最新的目录序列和发布时间戳。检查点使用隔离发布者仓库密钥签名。PostgreSQL将精确的标准叶和检查点字节以及独立查询的摘要和排序元数据一起存储。

## 哈希和签名构造

树遵循 RFC 6962 的分裂规则：一个非平凡树在其叶子计数小于其最大二进制幂时分裂。CardputerZero 使用显式的版本化域而不是 RFC 6962 的一位前缀：

```text
leaf_hash = SHA-256(leaf_domain || uint64_be(length) || canonical_leaf_json)
node_hash = SHA-256(node_domain || left_hash || right_hash)
```

签名的检查点消息同样包括检查点特定的域、规范的JSON长度和规范的检查点字节。Ed25519密钥ID使用与Store包和Catalog签名相同的约定。

所有的JSON解码器都拒绝未知字段、非规范重新编码、无效ID、无效状态/任务组合以及超出其固定大小范围的对象。一个v1树最多包含1,000,000个叶子。

## 原子发布

发布者首先编写一个包含以下内容的不可变候选生成：

```text
generations/<catalog-sequence>/catalog.json
generations/<catalog-sequence>/transparency/leaf.json
generations/<catalog-sequence>/transparency/checkpoint.json
generations/<catalog-sequence>/store.pub
```

包子目录仅存在于初始发布时。
数据库事务随后提交包记录、目录快照、叶节点、检查点、发布过渡、审计事件、出箱事件和已完成任务。PostgreSQL 拒绝不按顺序插入、修改或删除叶节点/检查点，以及未包含所有三个发布记录的完成操作。

`current` 只有在提交的目录、叶子、检查点和公钥字节对字节匹配时才会切换。启动时会执行相同的检查并在修复 `current` 之前进行。数据库回滚后留下的未引用的生成版本永远不会变为当前版本，只有在其确定的字节完全匹配重试的任务时才能重用。

## 验证模型

出版者启动时会解码每个叶子，重新计算每个叶子摘要，与目录快照和源任务进行交叉检查，并使用存储公钥验证每个检查点签名，重新计算每个完整树前缀。快照、叶子和检查点的数量必须一一对应。

公共crate暴露了观察者通过新检查点拥有所有叶子时的完整前缀验证。紧凑的一致性证明、包含证明服务、外部见证和八卦在S5H中未实现，且不应从签名检查点格式中推断出来。

## 升级和密钥限制

迁移 `0007_transparency_log.sql` 有意不为 S5H 前创建的 Catalog snapshot 虚构历史。snapshot 如果没有匹配的 leaf 和 checkpoint，Publisher 会按 fail-closed 原则拒绝继续。启用新 Publisher 前，operator 必须执行经审核的 backfill，按 sequence 重建并签名每个 snapshot；或者恢复干净的预发布数据库。

S5H 使用与参考 Store Publisher 相同的隔离原始 32 字节文件密钥来签署检查点。生产 HSM 集成、密钥仪式、轮换声明、见证部署和灾难恢复程序仍然是单独的基础架构关卡。

## 验证

```sh
cargo test -p cp0-store-transparency -p cp0-store-publisher
cargo clippy -p cp0-store-transparency -p cp0-store-publisher --all-targets -- -D warnings

# Requires a disposable PostgreSQL 17 database.
CP0_STORE_TEST_DATABASE_URL=postgres://... make store-control-db-check
```

PostgreSQL套件验证完整历史记录和序列间隙，文件系统恢复，数据库和生成器篡改拒绝，只追加SQL保护，以及目录、透明性、发布、审计和出箱状态的原子回滚。
