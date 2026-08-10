# Phase 4B: 可重复的 `.capp` 包和签名

<!-- doc-locale: zh-CN -->
> [English](PHASE4B-CAPP-FORMAT.md) | **简体中文**

## 安全模型

A `.capp` 仅包含普通文件。它不能编码所有者、模式、设备节点、链接、时间戳或平台原生可执行文件。解析器最多接受256条记录，每个条目的240字节规范化UTF-8路径，每个条目16 MiB，并且编码后的负载为32 MiB。绝对路径，空组件， `.`/`..`，反斜杠，
重复项、未排序项、尾随字节和未知头部标志在manifest解析或提取之前被拒绝。

该包有两个独立的 Ed25519 角色：

- 开发者签名规范负载并嵌入其公钥；
- 商店在审核后签署开发人员身份、开发人员签名和规范负载。仅嵌入商店密钥的SHA-256 ID。

重新签名总是清除之前的商店签名。商店不能在不无效其自身签名的情况下替换开发者的身份。量产设备需要来自根拥有信任目录的商店密钥。开发者模式是一个单独的可选政策，其永远不会仅仅因为签名在数学上有效就将嵌入的公钥视为可信。

## 标准格式 v1

所有整数都是小端序。固定头部包含：

```text
magic[8] = "CP0CAPP\0"
format_version: u16 = 1
flags: u16
entry_count: u32
payload_length: u64
developer_public_key[32]
developer_signature[64]
store_key_id_sha256[32]
store_signature[64]
```

未使用的固定签名字段必须全部为零。负载是按字节路径排序的条目连接起来的结果：

```text
path_length: u16
content_length: u32
path[path_length]
content[content_length]
```

没有非确定性元数据存在，因此相同的文件和密钥生成字节级相同的包。签名使用不同的、以NUL结尾的领域字符串来防止跨协议重用，分别对应开发者和商店角色。

## 命令行工作流

```sh
cp0ctl key generate developer.key developer.pub
cp0ctl key generate store.key store.pub
cp0ctl package ./my-app my-app-unsigned.capp
cp0ctl sign developer my-app-unsigned.capp my-app-developer.capp developer.key
cp0ctl sign store my-app-developer.capp my-app-store.capp store.key
cp0ctl verify my-app-store.capp store.pub
```

秘密密钥是32个原始字节，并且使用模式`0600`创建。公钥是32个原始字节。输出文件使用创建新文件的语义：CLI从不默默地覆盖密钥或包。

`cp0-package` 测试固定了规范 round trip、签名篡改检测、路径遍历拒绝和重复项拒绝。
CLI 集成测试也覆盖 SDK-only 的 Hello Card 应用。
