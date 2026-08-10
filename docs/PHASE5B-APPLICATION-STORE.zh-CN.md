# Phase 5B: 审查应用商店

<!-- doc-locale: zh-CN -->
> [English](PHASE5B-APPLICATION-STORE.md) | **简体中文**

## 信任链

商店从不直接将网络响应转换为安装的应用程序。完整的生产链是：

```text
developer-signed .capp
        |
exact review metadata (submission SHA-256, permissions, WASM imports)
        |
cp0ctl store publish: validate, store-sign, build signed catalog
        |
HTTPS catalog and package download through cp0-stored
        |
cp0-stored verifies catalog key, package size and SHA-256
        |
appd independently verifies source identity and both package signatures
        |
atomic version install and registry activation
```

开发者签名标识了源提交的来源。商店签名声明了精确的不可变提交通过了审核。一个包仅仅因为其应用ID、版本或开发者密钥与之前的审核匹配，并不代表它被批准。

Store目录键是安装在`/etc/cardputerzero/trust/store/<key-id>.pub`下的原始Ed25519公钥。该目录由root拥有，并以不跟随符号链接的方式读取。Store签名密钥仅存在于发布主机上，永远不会进入镜像或设备。

## 审阅和确定性发布

每次提交都有一个审核文件，命名为 `<app-id>-<version>.review.json`，并验证其是否符合 `schemas/store-review-v1.schema.json`。一个批准的记录绑定：

- 应用ID和语义版本；
- 完整开发者签名提交的SHA-256哈希值；
- 审阅者身份和审阅时间；
- 确切排序的manifest权限集；
- CardputerZero WASM 正确排序的导入集
- 一个受限制的用户面向摘要。

`cp0ctl store publish` 解码规范 `.capp`，验证其开发者签名，拒绝现有 Store 签名，验证 SDK 兼容性，使用 `wasmparser` 验证 WASM 模块，并拒绝非函数或不支持的导入。每个能力导入都必须有其对应的 manifest 权限。如果审核元数据与这些检查值中的任何一个不同，则发布失败。

出版商为每个批准的包进行商店签名，按ID排序应用程序，并将一个签名的`catalog.json`, 不可变包路径和`store.pub`写入一个新的输出目录。给定相同的输入、密钥和时间戳，输出字节是相同的。该命令拒绝合并到现有目录中。

```sh
cargo run -p cp0ctl -- store publish \
  submissions reviews public-store https://store.example.invalid \
  42 1800000000 1800600000 store.key
```

输出目录是一个静态的HTTPS源。该源的部署和审阅者工作流程授权在设备信任边界之外。

## 设备服务边界

`cp0-stored` 作为专用的 `cp0-store` 用户运行，带有 40 MiB cgroup 限制，没有能力或设备访问权限，并且仅拥有 `AF_UNIX`、`AF_INET` 和 `AF_INET6`。它只拥有自己的缓存和窄带 appd 阶段收件箱：

| 路径 | 所有者/模式 | 用途 |
|---|---|---|
| `/etc/cardputerzero/store.conf` | `root:root 0644` | HTTPS目录URL |
| `/etc/cardputerzero/trust/store` | `root:root`, 不可写 | 目录信任密钥 |
| `/var/lib/cardputerzero/store` | `cp0-store:cp0-store 0700` | 分类和部分下载 |
| `/run/cardputerzero-appd/store` | `cp0-store:cp0-store 0700` | 单文件 appd 手工交接 |
| `/run/cardputerzero-store/control.sock` | `root:cp0-control 0660` | 有界Shell协议 |

Shell 可以列出、刷新和请求安装。它不能提供 URL、包路径、哈希、版本或签名。`cp0-stored` 从验证过的目录中选择所有这些内容。Shell 从不下载应用程序。

经过验证下载后，`cp0-stored` 创建一个私有移交文件，并要求 appd 执行 `StoreInstall`。appd 只接受来自固定 `cp0-store` UID 的该命令。它独立检查文件类型、所有者、模式、字节计数、SHA-256、目录标识符、清单标识符、开发者签名和商店签名。商店安装只接受严格的语义版本升级；根调解回滚是一个单独的操作。

## 网络和回滚保护

两个目录和包的URL都必须使用HTTPS，不包含凭据或片段。
与`networkd`相同的公共地址解析器在每次解析时都会拒绝循环回环、私有、链接本地、多播、保留和过渡地址。
环境代理被禁用，重定向被限制，响应大小被限制，并且TLS验证不能被禁用。

目录具有非零单调序列和有界有效区间。
设备拒绝过期或未来日期的目录、较低序列以及当前序列重复使用不同内容。成功验证的目录会原子替换。过期的缓存目录可能会显示为过时，但不能授权安装。

包下载到名为SHA-256的`.part`文件中。恢复需要匹配的HTTP范围响应和内容范围。在移交前，通过已打开的描述符检查最终字节计数和SHA-256，因此路径名替换不能改变验证的内容。

## Shell 行为

320x170 System Shell 有一个专用的 Store 入口。列表最多保留协议所限定 64 个 Catalog 应用中的 32 个，一次最多显示四行。Enter 打开详情视图，其中包含版本、审核摘要、状态和所有已批准的权限；再次按 Enter 请求安装。Right 请求刷新 Catalog，Escape 从详情视图返回列表，再返回 Home。

Shell 将目录条目与 appd 的已安装版本列表进行协调：

- 确切安装版本: `INSTALLED`;
- 另一个安装版本：`UPDATE`；
- 没有安装版本: `GET`;
- 队列、下载和安装进度：来自`cp0-stored`并从未被同步覆盖。

版本顺序故意没有包含在Shell中。appd 是严格升级语义的安全权威。

硬件验收无需给操作员提供 URL、包路径、哈希或签名覆盖：相同的有界控制路径仍然可用：

```sh
sudo cp0ctl store list
sudo cp0ctl store search notes
sudo cp0ctl store search notes 8 8
sudo cp0ctl store refresh
sudo cp0ctl store install dev.cardputerzero.example --approve-permissions
```

`cp0ctl` 拒绝一个种类与请求不符的响应，例如不同应用ID的安装响应、不同查询或页面的搜索结果、无关的请求ID或任何格式错误的协议字段。搜索由 `cp0-stored` 在验证过的目录库中本地进行；查询永远不会离开设备。这些命令不会更改产品的 Store 配置或信任根。

## 离线且未配置的操作

量产镜像默认提供空的 `catalog_url`，且不包含生产 trust key。
Store 屏幕报告 `NOT CONFIGURED`，直到操作员为两者都进行配置。
这防止了一个开发端点成为隐含的产品信任根。

当网络不可用时，之前验证且未过期的缓存目录仍然可浏览。部分包字节保持私有状态，并在连通性恢复后可以继续下载；离线安装不被支持。过期的目录会明显标记，并不能安装。如果没有验证过的缓存，Shell 会报告 Store 为不可用。Store 的失败不会阻止本地安装的应用程序启动。

## 验证

自动化覆盖包括签名目录边界、到期和序列回滚、同序列抵赖、HTTPS/公共地址强制执行、可恢复下载、恶意Content-Range变体、确定性目录和协议帧变异、包哈希失败、对等UID授权、严格升级强制执行、审查/导入/权限绑定、畸形Shell响应、Store导航和320x170屏幕截图回归。

AArch64 Store 控制组件已于 2026-07-31 热部署。量产配置故意保持为空；
`sudo cp0ctl store list` 成功到达 `cp0-stored` 并返回预期的结构化 `Unconfigured` 错误，
没有启动下载或改变 App 状态。这验证的是设备控制路径，不是下文所述的在线 Store 验收。

在启用产品端点之前，完成涵盖刷新、中断下载/恢复、安装、启动、更新、过期目录、离线缓存目录以及应用d交接过程中断电的实物设备运行。
