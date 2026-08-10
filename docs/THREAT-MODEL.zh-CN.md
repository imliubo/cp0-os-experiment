# CardputerZero OS 威胁模型

<!-- doc-locale: zh-CN -->
> [English](THREAT-MODEL.md) | **简体中文**

## 范围和主张

该模型涵盖 V0.6 CM0 设备、CardputerZero OS 镜像、可信本地服务、WASM 应用平台、Store 内容和离线 recovery data。它假定固定的构建输入和发布二进制在分发前已经过审核。

应用隔离意味着不信任的SDK应用不会收到宿主的任何权限，并且不能访问其他应用的数据或未授予的能力。这并不能绝对保证内核、WAMR、本地代理、编译器或硬件的安全。物理SD卡的替换也超出了当前验证的安全边界，因为V0.6没有展示出不可变启动信任锚点。

## 资产和安全目标

- 应用程序私有状态、文档、网络凭证和设备身份在应用程序 UID 之间保持保密；
- 权限决策、设备策略、Store 根和已安装应用 注册表保持完整性；
- 可信的 compositor 拥有全局密钥、焦点和权限表面；
- package、Catalog、recovery bundle 和 IPC frame 使用固定边界解析，解析失败时按 fail-closed 原则拒绝。
- 一个应用的失败或资源耗尽不会影响 compositor、System Shell、appd 或其他应用的状态；
- 正常的更新路径不接受操作系统或应用程序降级；
- 恢复是明确的，离线进行且不能在活动根上默默操作。

破坏性物理损坏后的可用性、侵入式硬件攻击者的保密性、RF 法规合规性和恶意编译器或上游签名权威的防护超出了本版本的范围。

## 行为和信任边界

| 行为者或边界 | 可信权威 | 攻击者控制的输入 |
| --- | --- | --- |
| SDK 应用 | 仅限授予的能力代理调用 | WASM 指令、主机调用参数、UI 和意图载荷 |
| 应用运行时 | 验证的WASM执行和固定的宿主ABI | 模块内存、陷阱和CPU/内存压力 |
| appd 和能力代理服务 | 包装标识、UID 映射、策略和硬件调解 | 来自认证对等方的 Unix 帧、网络/媒体/设备故障 |
| compositor 和 System Shell | 焦点、全局快捷键、可信 overlay 和用户决策 | 普通 Wayland surface 和有界应用元数据 |
| Store | 配置好的 Ed25519 根和单调递增的目录序列 | HTTPS 响应、目录、包和中断的下载 |
| 恢复操作员 | 显式根仪式和选定的可移动介质 | 备份文件内容和块设备选择 |
| USB媒体主机| 文件被复制进或出隔离交换镜像| 威胁FAT元数据、名称、WAV字节、断开连接和电源丢失|
| 启动链路 | Raspberry Pi 固件、内核、初始内存文件系统和底层根 | 可变 SD 卡启动/根内容直到存在验证启动 |
| 构建/发布 | 固定的源代码版本和签名密钥 | 依赖供应链和构建宿主 |

根账户、内核、initramfs、 compositor 策略模块、System Shell、appd、App Runtime、能力代理服务和生产签名密钥在受信任计算基中。网络对等体、Store 载荷、恢复捆绑包、应用程序和应用程序拥有的数据是不可信的。

## 主要威胁分析

| ID | 威胁 | 已有控制措施 | 剩余风险/所需行动 |
| --- | --- | --- | --- |
| APP-01 | 应用读取宿主机或其他应用的数据 | WAMR、唯一 UID、bubblewrap namespace、无宿主路径、seccomp、brokered storage | 原生 Runtime/内核逃逸仍有可能；保留恶意 App 和 fuzz 测试 |
| APP-02 | 应用冒充另一个 App | `SO_PEERCRED`、root 拥有的注册表、稳定且不回收的 UID | appd 或内核被攻陷会绕过身份验证 |
| UI-01 | 应用覆盖或伪造权限提示 | compositor 拥有的顶层、通过 peer UID 认证的私有协议 | App surface 内仍可能模仿系统视觉；提示必须明确标识 App |
| IPC-01 | 畸形或超大 frame 耗尽守护进程资源 | 换行 frame 上限、严格 serde schema、有界队列和超时 | 解析器缺陷仍可能存在；所有公共 decoder 都是 fuzz 目标 |
| PKG-01 | 包或 Catalog 被篡改或回滚 | 双重 Ed25519 签名、key ID、Catalog sequence、原子安装 | 签名密钥泄露后需要外部撤销和发布响应 |
| NET-01 | broker 被用于 SSRF 或本地网络探测 | 仅 HTTPS、公网地址验证、DNS rebinding 检查、响应上限 | 公网端点仍可返回恶意但有界的内容 |
| DATA-01 | 跨 App 持久数据访问 | storage broker 从 UID 推导调用方并执行每 App quota | 离线物理访问仍能看到未加密的 `cp0-data` |
| REC-01 | 备份路径逃逸或创建特殊文件 | 固定布局 allowlist、禁止链接/设备、Owner/mode 检查、两遍哈希、空目标目录 | `CP0 backup v1` 未加密且未认证；操作员介质被视为可信 |
| DOS-01 | App 耗尽 RAM/CPU/进程 | 单前台槽、cgroup 内存/进程限制、WAMR 上限、服务重启策略 | 持续的内核级 I/O 争用仍需硬件 soak 测试 |
| BOOT-01 | SD 攻击者替换 kernel/initramfs/rootfs | OverlayFS 只限制运行时写入 | OverlayFS 不是完整性控制；verified boot 和 dm-verity 仍是发布阻塞项 |
| UPDATE-01 | 中断或恶意 OS 更新使设备无法启动 | 当前未启用远程 OS updater | 只有通过启动链硬件验证后，才能采用 ADR 0006 的签名 A/B 设计 |
| MGMT-01 | 共享开发凭据或固定人类身份暴露 SSH/sudo | 量产 rootfs 移除 pi-gen 账户；可信 Setup 在持久 extrausers 中创建一个 Owner；保持无 sudo；SSH 由显式 marker 控制且默认关闭 | 新介质镜像检查和 V0.6 SSH 拒绝/允许测试仍是发布门禁；物理 SD 访问仍可改写未加密 Owner 数据 |
| DEV-01 | Developer Mode 变成不受限远程 Shell | sshd Owner dispatcher、forced-command 配对密钥、禁止 forwarding、每次请求重新检查 mode/policy、无 sudo/root、独立 `cp0-ssh` Owner Shell 组 | 真实 OpenSSH argv/environment 行为和关闭模式后断开连接仍需 V0.6 验收 |
| DEV-02 | 未授权电脑或签名密钥安装 App | 物理开启的十分钟配对窗口、Owner 密码、Ed25519 SSH key、已配对 developer key、签名 `.capp`、root registry、appd 复验 | Owner 密码或可信原生服务被攻陷后可授权主机；物理 SD 访问可改写未加密配对状态 |
| USB-01 | 电脑直接访问活动设备数据 | 单个固定 regular-file LUN、严格无路径 IPC、规范 path/type/size 检查、bind 前卸载设备侧文件系统 | Host 可破坏一次性 FAT 镜像；kernel ConfigFS/MSC 缺陷仍属于 TCB |
| USB-02 | Host 构造文件逃逸导入边界或覆盖文档 | FAT 以 nodev/nosuid/noexec 挂载、`O_NOFOLLOW`、有界严格 WAV parser、稳定 inode/size 检查、create-without-overwrite 发布 | 可信 decoder 有意只支持固定 PCM WAV |
| USB-03 | Exchange 镜像泄露进完整备份或量产 seed | Recovery 遍历排除 `cardputerzero/usb-media`；rootfs 门禁拒绝预置 `exchange.img` | 权威照片和导入文档仍位于未加密的 `cp0-data` 备份中 |
| SUPPLY-01 | 依赖或构建链被攻陷 | 固定 BSP/pi-gen revision、锁定 Rust 依赖、镜像门禁 | 可复现构建比对、SBOM 签名和独立审核仍未关闭 |

## 发布门禁

以下条件即使开发镜像通过功能测试也会阻止生产安全声明：

1. 任何名称或配置为 development 的制品都可能包含操作员设置的密码和已启用 SSH，
   不得作为量产镜像重新分发。量产 profile 已移除临时构建身份，通过可信设备端 Setup
   创建 Owner，并默认关闭远程访问。mounted-image 零凭据检查和 V0.6 新介质硬件验收
   仍是发布门禁。由操作员单独配置的 Recovery SD 卡仍属于物理维护流程。
2. 可变 FAT boot 分区和未签名 root hash 意味着当前镜像无法抵抗物理 SD 卡篡改。
   Verified Update 的决策和限制见 ADR 0006。
3. `cp0-data` 和 `CP0 backup v1` 的静态数据未加密。设备丢失和可移动介质机密性需要
   单独设计的密钥层级。
4. 24 小时稳定性、破坏性恢复流程、外设测试和 OS rollback 必须在 Roadmap 指定的
   硬件条件下完成。
5. 内部威胁模型和 fuzz 工具不能替代独立评估。第三方安全审核仍未完成；做出量产安全
   声明前，必须跟踪并关闭其发现。
6. USB Media Transfer 不得使用 Linux Foundation 开发 VID/PID 发布。合法量产 VID/PID、
   V0.6 跨主机 MSC 验收和故障验收仍是发布阻塞项。

## 验证映射

- `make check` 包括模式、协议、包、沙箱、权限、恶意应用、恢复、开发者模式配对/撤销以及静态镜像策略测试。
- `make fuzz-check` 使用夜间 Rust 版本编译所有 libFuzzer 目标。
- `make fuzz-smoke` 运行有界 AddressSanitizer 活动，用于 manifest、package、Store、appd 控制和恢复输入。
- 计划中的 fuzz 工作流会将任何崩溃样本保存为 CI 构建产物。
- 镜像发布需要通过压缩镜像 checksum 门禁和 mounted-rootfs/initramfs profile 门禁。
- `cp0-os-update` 测试单调发布策略，启动前持久化状态转换，三重失败回退，冗余记录选择和100次中断更新周期。`verify-os-release-artifacts.sh` 将根文件系统，verity树和FIT字节绑定到认证元数据并调用`veritysetup verify`；这两种机制本身不认证RAUC CMS或FIT。
- 硬件验收结果只有在保留其完整运行目录、持续时间、失败次数以及重启/内存/SD写入总结时才是证据。

此文档必须在信任边界、公共输入格式、生产访问路径、启动链或签名根发生变化时进行修订。
