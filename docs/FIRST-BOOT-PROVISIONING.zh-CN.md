# CardputerZero 系统首次启动配置计划

<!-- doc-locale: zh-CN -->
> [English](FIRST-BOOT-PROVISIONING.md) | **简体中文**

> 状态：已批准并完成宿主机/镜像实现。发布或部署前仍需完成 V0.6 新介质烧录和硬件验收。

## 目标

可分发的产品镜像必须不包含固定的用户名、密码、授权密钥或可重用的登录密钥。首次启动时，320x170 像素的 LCD 必须引导设备所有者完成每一个必需的选择，而无需使用 HDMI、SSH、串行控制台或预先存在的网络连接。

流程必须：

- 在设备上创建所有者身份和密码，而不要在镜像构建时创建；
- 收集区域设置、监管国家、时区和设备名称，并附带可见的解释和验证；
- 建立以太网连接，配置Wi-Fi，或记录明确的离线选择；
- 除非所有者明确启用，否则保持远程访问关闭；
- 在每一页和最终应用过程中 survive 功率损失；
- 在完成配置之前，阻止Home和第三方应用运行；
- 适配单前台、键盘驱动的 320x170 System Shell；
- 保持应用程序 UID 隔离和不可变根数据分区的设计；
- 在持久化状态不一致时提供可恢复的错误路径。

此计划适用于可分发的产品镜像。开发和恢复
 artefact 仍需明确标记为操作员工具，并且绝不能呈现为产品镜像。它们可以接受构建时操作员凭据，但任何发布的 artefact 不能继承它们。

## 定义和不变量

- **人类账户**：一个交互式的所有者、管理员或维护账户。
固定 `cp0-*` 服务身份是非交互式的系统账户，不是人类凭证。
- **所有者**: 本地创建的人类身份。桌面仍然会直接启动受信任的系统 shell；所有者在看到主页之前不会登录。密码用于远程访问和未来的特权用户操作。
- **配置完成**：所有者身份、区域设置和一个网络决策已被一致提交。无需活的互联网连接。
- 网络决策：其中一个以太网成功配置，一个 Wi-Fi 配置文件保存，或所有者明确选择 `Use offline`.
- **完成最后**：`COMPLETE` 状态只有在每个持久对象都被写入、同步和成功读回之后才被写入。

临时断开连接、配置后出现不可用的接入点或拔掉以太网线，不应重新打开向导。这些是正常的网络设置状态。只有在网络决策从未被提交、设置不完整或其持久状态不一致时，向导才会重新启动。

## 产品流程

### 入口和所有权

System Shell 在显示 Home 之前会查询 root provisioning 服务。
当状态缺失或不完整时，它会进入一个受信任的设置模式。在这种模式下， compositor:

- 仅允许 System Shell 的 provisioning 表面；
- 拒绝普通应用程序激活和任务切换；
- 保留全局电源管理并提供受控紧急关闭路径；
- SDK应用程序表面从不绘制设置内容。

在重启后，流程会在上次提交的非密文步骤处继续。允许在最终提交前返回。在提交期间，导航和常规电源操作会被保留，直到当前原子操作完成或失败到一个可恢复页面。

### 320x170 的页面

每一页都使用固定的页眉，最多四行可见内容，一个主要操作和一个简洁的帮助/错误区域。长列表可以滚动而不需要调整表面大小。
页眉只显示Setup身份和电池状态。网络地址有意保留在标题栏之外；它们在Network、Review和Complete页面出现，因为在那里它们的上下文是明确的。

1. **欢迎和区域区域设置**：首次发布时，Setup 显示为英文，并选择生成的 `en_US` 或 `zh_CN` 系统区域设置。完整的 Shell 本地化仍然是长期翻译像素验收关卡的一部分。
2. **国家和时区**：首先列出监管国家，然后是受限的时间区列表。国家控制合法的Wi-Fi频道。当可用时使用网络时间；手动时钟校正延迟到设置后。
3. 设备名称：可编辑主机名，包含示例和内置验证。
4. **所有者名称**：受信任系统UI中使用的显示名称。
5. **用户名**：交互式账户名称，可用性和预留名检查。
6. **密码**：掩码输入，强度/长度反馈和一个显示切换。
7. 确认密码：精确重输；两个值都不会作为设置状态保存。
8. **网络**：活动以太网状态和IPv4地址、Wi-Fi适配器状态，或 `Use offline`。一个连接但没有DHCP的链接显示为 `Waiting for IP`，并且不能被接受为配置的以太网。
9. **Wi-Fi网络**：扫描结果、信号/安全指示器和刷新。隐藏SSID条目在其键盘和错误状态有像素覆盖后才显示；它不会被默默地视为一个空扫描结果。
10. **Wi-Fi凭据**：掩码密码和连接进度。DHCP是初始支持的IP模式；高级静态地址设置将在初始发布后在设置中提供。
11. 远程访问：默认关闭；解释所选模式和本地风险。
12. 审阅：显示所有非密文选项。密码和Wi-Fi密文仅表示为`Configured`。
13. 应用于：确定性进度以确保身份、区域设置、网络、远程访问和验证。
14. **完整**: 主机名，当前IPv4（或离线），远程访问状态和一个单个`Start`操作。当前IP在设置后可以从网络系统应用中获取。

键盘行为在每一页上保持一致：上/下键移动焦点，左/右键改变值，回车键激活，删除键编辑，ESC键在安全时返回。V0.6 BSP 在按下和释放时立即报告物理Shift键。没有Shift键时字母为小写；只有在物理按住Shift键时字母才大写。Shell仅使用按下XKB Shift修饰符，原始键事件作为备用；物理锁定和锁定的Shift状态不会改变文本大小写。Sym层将V0.6键盘图例中的32个物理组合和`按键sym.csv`映射到不同的Linux evdev标识符。共享的System Shell和Runtime字符表将每个标识符直接转换为其打印符号，不使用合成Shift。因此，设置、系统文本字段和SDK应用程序接收相同的小写、大写和符号语义。
密码页面禁用键点击声，默认遮盖文本，并且绝不能在屏幕截图、崩溃报告或支持包中以明文形式出现。

低电量可能会在提交前产生警告，但缺失或不支持的电池遥测不得导致Setup死锁。LCD亮度和键盘/音频反馈应使用保守默认值，直到用户在设置中更改它们。

### 验证规则

- 用户名: `^[a-z_][a-z0-9_-]{0,31}$`；拒绝 `root`，所有现有系统身份，所有 `cp0-*` 名称和未来预留前缀。选定的 UID 从受控所有者范围 `1000-1999` 分配；应用程序 UID 保持在 `20000` 及以上。
- 主机名：长度为1-63的小写字母RFC 1123字符，不允许以破折号开头或结尾。
- 密码：第一版政策为10-64个可打印ASCII字符，精确无截断。这是最低要求，不是密码强度声明。
- Wi-Fi：验证SSID字节长度和安全特定凭证边界；支持NetworkManager提供的开放、WPA2和WPA3配置文件。在要求输入密码之前识别不支持的企业身份验证。
- 所有的自由文本和协议帧都有明确的字节和字符边界。

## 架构

```text
keyboard -> compositor -> trusted System Shell Setup UI
                              |
                              | bounded Unix SOCK_SEQPACKET protocol
                              v
                      cp0-provisiond (root)
                       |       |       |
                 owner NSS   locale   NetworkManager
                    data      config     keyfiles
                       \       |       /
                        cp0-data persistence
```

### 可信用户界面

Provisioning 模式是现有 System Shell 的一部分，而不是可安装的 SDK 应用。它复用当前的 RGB565 渲染器、键盘焦点规则、可信 compositor 表面和像素回归测试工具。这避免了授予应用身份权限，也避免在 512 MB 设备上再驻留一个 UI 进程。

Shell 保持无特权状态。它渲染受限数据并发送验证过的意图到`cp0-provisiond`；它从不直接编辑账户、阴影、NetworkManager 或 SSH 文件。

### 根配置服务

`cp0-provisiond` 是一个由 root 所拥有的，通过套接字激活的服务。它的 Unix 套接字 使用 DAC 权限来验证 `cp0-shell` 身份，并且每个连接还会通过 `SO_PEERCRED` 进行检查。UID 0（所有者的 UID）和所有应用程序的 UID 都被拒绝作为调用者；root 通过本地文件和 systemd 来管理该服务，而不是通过 Shell 协议。

该服务使用有界的`SOCK_SEQPACKET`消息，并带有严格的版本化JSON。
实现的命令有：

- `GetStatus` 和 `SetRegion`（区域、国家、时区和主机名）;
- `SetOwner`, `SetPassword` 和设置完成后 `ChangePassword`；
- `ListWifi`, `ConnectWifi`, `UseEthernet` 和 `UseOffline`；
- `SetSshEnabled` 和 `Commit`.

响应绝不回显密码或 Wi-Fi secret。未知字段、重复字段、超大 frame、无效状态转换，以及错误 peer 的调用都会按 fail-closed 原则拒绝。进入 `COMPLETE` 后，region、identity、initial password、Ethernet/offline choice 和 commit mutation 永久不可用。可信 Settings UI 仍可调用 `ListWifi`、`ConnectWifi`、`SetSshEnabled` 和 `ChangePassword`；其他 provisioning command 一律按 fail-closed 原则拒绝。`ChangePassword` 要求当前 Owner password。恢复出厂设置仍是单独的物理/recovery 流程。

守护进程拥有用于设置和受信任所有者维护的凭据承载 NetworkManager 扫描/连接路径。`cp0-connectivityd` 继续拥有设置后无线电和飞机模式切换。两者仍然保持为独立的最小权限服务，而不是将 NetworkManager 或 Wi-Fi 秘密暴露给应用程序。

### 资源限制

Setup renderer 继续位于现有的 32 MiB Shell cgroup 中。守护进程使用 64 MiB 内存上限、
零 swap 和较小的 task 上限，并允许在空闲或完成后退出。该一次性上限同时覆盖守护进程及
其 yescrypt 子进程；原先的 16 MiB 上限可能在 CM0 上杀死密码哈希过程。协议帧最大
16 KiB，Wi-Fi 结果最多 64 条，一次只执行一个扫描或连接尝试。不引入下载资源、WebView、
桌面工具包或额外的长期 UI 进程。候选镜像验收时，硬件证据必须保留实测峰值内存。

### 没有可变的人类身份 `/etc`

pi-gen 可能需要一个临时阶段账户。构建过程将使用一个明确命名的账户，如 `cp0-build`，然后在导出前删除该账户、其家目录、组、sudo 规则和所有拥有的文件。最终镜像中的任何普通交互 UID 将被挂载-rootfs 发行版网关拒绝。根用户保持锁定和固定，服务账户保留 `nologin` 环境。

运行时所有者记录存储在`cp0-data`上，与不可变OS服务身份分开。实现的后端是`libnss-extrausers`，包括：

```text
/var/lib/extrausers/passwd
/var/lib/extrausers/shadow
/var/lib/extrausers/group
/var/lib/extrausers/gshadow
```

NSS 从 `/etc` 解析不可变服务账户，并从这个持久数据库中解析所有者。Debian 13 的标准 `pam_unix` 使用 `libnss-extrausers` 暴露的阴影记录进行身份验证；这通过目标容器密码身份验证测试进行了验证，并避免了不可用的 `libpam-extrausers` 包。
完整持久保存 `/etc/passwd`、`/etc/shadow` 或 `/etc/group` 被拒绝，因为操作系统回滚/更新可能会与服务账户发生冲突。

`cp0-provisiond` 分配恰好一个所有者 UID，创建四个数据库，在独占锁下进行，并验证每个现有记录，通过同一文件系统的临时文件 `fsync` 提交，重命名和父目录 `fsync`。密码哈希使用守护进程中的平台 yescrypt/libxcrypt 实现。明文密钥从未通过 argv 或环境变量传递，而是写入临时文件，记录或存储在 `state.json` 中；使用后，其缓冲区明确清空。

### 运行时所有者密码更改

`Settings -> Security -> Change Password` 是一个受信任的 System Shell 流程，不是一个 SDK 应用程序。它会要求输入当前密码、一个 10-64 个字符的新密码以及精确确认。输入默认被遮掩，Right 临时切换可见性。ESC 逐页返回；在当前密码页面按 ESC 取消并返回到相同的选定 Security 行。Home、Tasks、Power、媒体、亮度、通知和应用程序提示不能中断或覆盖凭据流程。应用页面不接受输入。

Shell将两个密码通过现有的根拥有的配置分配套接字发送过去。`cp0-provisiond`仅在`COMPLETE`接受此命令，读取单个所有者阴影记录，通过系统libxcrypt实现验证当前密码的yescrypt哈希，使用`mkpasswd`生成一个新的yescrypt哈希，并原子地替换阴影文件。验证不通过外部命令行传递盐；Debian 13的`mkpasswd`故意拒绝显式的yescrypt盐。
当前密码错误返回不同的`authentication`结果；UI在显示错误前清空所有三个密码缓冲区，并要求重新进行完整的流程。成功、取消、服务失败和Shell销毁也会清空完整的分配秘密区域。包含凭证的JSON请求/响应帧在解析或使用后显式覆盖。

业主家园存储在`cp0-data`上，并挂载在`/home/<username>`处，模式为`0700`。首次发布依赖于现有的`cp0-data`容量，而不是单独的业主家园配额。SSH授权密钥应使用根控制的`/etc/cardputerzero/authorized_keys/<username>`路径，这样被劫持的业主Shell不能默默地重写策略管理的密钥。如果启用了交互式SSH Shell，家园仍然需要，并且必须包含在文档化的备份/工厂重置策略中。

### 持久状态和断电恢复

非机密状态位于：

```text
/var/lib/cardputerzero/provisioning/state.json
```

它是根拥有的，模式`0600`, 方案版本化的，严格解析并通过临时写入原子替换的，文件`fsync`, 重命名和目录`fsync`。
实现的持久状态包括：

```text
UNPROVISIONED -> OWNER -> PASSWORD_READY -> NETWORK
              -> REMOTE_ACCESS -> REVIEW -> COMMITTING -> COMPLETE
                                                     \-> REPAIR_REQUIRED
```

`SetRegion` 执行 `UNPROVISIONED -> OWNER` 转换；协议的 `REGION` 值保留用于未来的独立持久化区域页面。每次转换都是幂等的。`PASSWORD_READY` 只记录密码哈希被提交，从不记录秘密。在启动时，服务会交叉检查状态与所有者密码/阴影/组/家目录记录，完成标记和 SSH 同意标记。区域设置在启动时幂等地重新应用；显式的网络决策在之后的链接/配置更改后仍然有效。缺少身份先决条件或矛盾的完成记录进入 `REPAIR_REQUIRED`；它们必须不能绕过设置，创建第二个所有者或自动擦除数据。

最终提交应用并验证区域设置、SSH 同意标记和所有者组成员身份后，再持久化 `COMMITTING`。然后，它原子地写入完成标记，通过其标记控制的 systemd 单元激活实时 SSH 选项，并最终持久化 `COMPLETE`。标记必须存在才能进行实时激活，因为生产 sshd 网关拒绝预配置启动。使用 `COMMITTING` 重启但没有完成标记会安全地返回到 `REVIEW`；使用最终副作用和标记会自动完成事务并在启动时应用 SSH 选项。`REVIEW` 接受预提交或 idempotently 应用的 SSH 成员身份，因此早期提交失败可以重试而不是被误分类为身份损坏。

如果`cp0-data`无法安全挂载，Shell将显示一个持久存储错误，并提供关机/重试指导。它必须不在易失性覆盖层中创建凭据。

### 网络

选择国家前会启用 Wi-Fi 连接。Wi-Fi 配置文件保留在现有持久化的 NetworkManager 关键文件路径中，模式为 `0600`，根所有权且不向 Shell 暴露密钥。设置支持扫描、刷新、连接重试和明确的离线选择。实时设置状态区分 NetworkManager 未可用、无 Wi-Fi 接口、以太网链路无 DHCP 以及可用的 IPv4 地址。以太网仅在 NetworkManager 在设置过程中获取到可用的 IPv4 配置后才被视为配置完成。可选择开放、WPA2 和 WPA3 网络；WEP、802.1X/EAP 及其他不支持的安全模式可见但会在要求输入不兼容密码前被拒绝。

Shell 请求超时是操作特定的，而不是单一的三秒截止时间：系统变更和状态操作使用20秒，因为它们的响应包括实时网络探测；扫描使用45秒，CM0密码哈希、Wi-Fi激活和最终提交使用75秒。NetworkManager本身对扫描的超时限制为30秒，对激活的超时限制为45秒。UI 在阻塞这些有界调用之前渲染显式的`Securing password`、`Scanning Wi-Fi`和`Connecting Wi-Fi`等待状态。

`state.json` 只记录网络决策类型和稳定配置文件 ID，而不记录 SSID 密码。设置后的网络可用性是信息性的。忘记或替换最后一个 Wi-Fi 配置文件后不会破坏所有者身份或重新打开向导；`Settings -> Connectivity -> Wi-Fi Networks` 通过相同的绑定配置代理扫描并连接，同时保持设备在 `COMPLETE`。

网络时间不可用时，Setup 可以接受手动时间，并在 `cp0-data` 中保存最后一个已知合理
时间戳。后续 NTP 同步可能前移或校正墙钟时间，但 Store/更新的安全决策不得把未经验证的
NTP 同步前时间视为权威。V0.6 硬件验收必须确认 RTC 是否可用，并记录冷启动行为。

### 远程访问

远程访问默认为 Off。产品分离了两个控制：

- **关机**：没有SSH监听器和没有生成的主机密钥；
- 开发者模式：仅支持受限 `cp0-dev` 部署，具有独立的十分钟设备配对窗口和根管理的Ed25519密钥；
- **所有者 SSH Shell**：仅所有者可以使用完整的 Bash，root 登录和 sudo 仍然禁用。

所有者始终接收受限的 `cp0-developer-access` 身份；
只有在选择了独立所有者 SSH Shell 选项时才会添加 `cp0-ssh` 成员身份。sshd 在 `AllowGroups`、`PermitRootLogin no` 中使用两个组，并且使用根控制的授权密钥路径。主机密钥仅在两种访问模式之一启动 SSH 时生成，并写入持久的 `/etc/ssh` 绑定。设置完成除非所有者选择了 shell，否则永远不会打开端口；开发者模式在后来的设置中物理启用前保持关闭状态。

生产配置会初始禁用 SSH 而不永久隐藏它。
实现的所有者选择使用此根目录唯一标记：

```text
/var/lib/cardputerzero/provisioning/ssh-enabled
```

开发者模式标志是独立的：

```text
/var/lib/cardputerzero/registry/developer-mode
```

sshd 和主机密钥准备使用一个 systemd 条件或生成器，在配置完成后接受标志。 `/usr/libexec/cardputerzero/owner-shell`
路由 `cp0-dev` 进入受限守护进程并在登录过程具有 `cp0-ssh` 由所有者授予的组 SSH Shell 设置。
守护进程在每次请求时检查根唯一的开发者模式标记。配对
的公钥总是携带一个强制 `cp0ctl dev-session` 命令后，sshd 禁用所有转发。完整页面显示主机名、IP 和 Owner SSH Shell 状态，而不显示密码。

### 运行时所有者网络和SSH维护

`Settings -> Connectivity` 将 Wi-Fi 无线电与网络选择分离。
无线电和飞行模式保持 `cp0-connectivityd` 操作。打开
`Wi-Fi Networks` 请求 `cp0-provisiond` 进行有界扫描，最多显示 64 个开放/WPA 网络，拒绝不支持的企业安全，并仅连接所选的 SSID。WPA 输入接受可打印的 ASCII 字符，默认被遮罩，并在成功或取消后清除。连接失败将留在凭据页面进行修正，而不保存尝试的秘密。

`Settings -> Security -> SSH Shell` 独立地启用或禁用 Owner SSH Shell。在 Setup 后，根配置守护进程更新同意标记、所有者 `cp0-ssh` 组成员身份、实时 `ssh.service` 状态和持久配置状态作为一个逻辑操作。同步失败会恢复之前的标记、组成员身份和服务状态。该设置从不启用 root 登录或 sudo，也不会扩展受限 Developer Mode 的部署渠道。

## 安全和隐私要求

- Setup 不执行任何数据分析，Store 登录或互联网调用，除非由所有者选择网络操作。
- 秘密不会包含在journald、内核命令行、进程列表、环境变量、支持包、屏幕截图和崩溃输出中。
- compositor 继续拥有所有输入和可信表面；应用不能显示、观察或争抢 Setup。
- 所有的设置文件拒绝链接、非常规文件、意外的所有者/模式以及路径替换。
- 超时和重试是有限制的。恶意接入点无法永久持有UI，也无法导致无界的结果列表和分配。
- 工厂复位必须一起擦除所有者NSS数据、家目录、Wi-Fi配置文件、SSH密钥和 provisioning 状态，然后返回到`UNPROVISIONED`。
- 恢复和OS回滚必须理解配置方案，并且必须不恢复没有其引用身份/配置的`COMPLETE`标记。

所有者 **没有** 被授予 sudo 权限。开发者模式现在仅以分离和可见的方式授予受限的 `cp0-devd` 操作，并且这些操作在 `DEVELOPER-ACCESS.md` 中有文档说明。将每个所有者都变为不受限制的 Linux 管理员会削弱类似 Android 的应用程序和代理边界。

## 验证计划和发布门禁

生产接受直到以下所有项通过才完成：

### V0.6 发现：缺少持久化服务目录

第一个量产候选版本在V0.6上启动了受信任的设置界面，但报告了`Provisioning service is unavailable`。检查确切的镜像显示，工厂`cp0-data`载荷省略了`/var/lib/cardputerzero/provisioning`，而守护进程的systemd沙盒将该路径命名为必需的`ReadWritePaths`条目。因此systemd在执行之前拒绝了该服务。

修正后的镜像会在 factory payload 和 tmpfiles 中创建 root 拥有、模式为 `0700` 的目录，
在 mounted-rootfs 门禁中验证该目录，让 System Shell 排在 provisioning socket 之后启动，
并重试暂时不可用的 socket，而不会让 Setup 卡住。守护进程还保留为 UID 1000 分配持久
Owner home 所需的最小 `CAP_CHOWN`。仍需新介质烧录才能关闭该硬件发现。

下一次 V0.6 新烧录介质测试进入了 Owner 创建阶段，但返回
`provisioning state could not be updated`。provisioning unit 同时使用
`ProtectHome=read-only` 和 `ReadWritePaths=/home` 例外；在这种组合下，systemd 仍将受
保护的 home 层次保持为只读，因此即使持久数据 bind mount 本身可写，创建 Owner home
仍会失败。该 unit 现在使用 `ProtectSystem=strict`、显式的 `/home` 可写 allowlist 和
`ProtectHome=no`；守护进程看不到其他 home 路径。源码门禁和已挂载镜像门禁会拒绝恢复
到冲突策略的回归。

随后一次新烧录介质测试在提交 Device Name 时失败。区域配置还负责设置 hostname、
locale、时区和无线监管国家。服务原先启用了 `ProtectHostname=yes`，与其 hostname 管理
职责冲突；同时，设备缺少无线 PHY 会导致 `iw reg set` 使整个操作失败。该 unit 现在使用
`ProtectHostname=no`，并保留其他沙箱控制。只有 `/sys/class/ieee80211` 下不存在 PHY 时
才跳过监管配置。系统命令失败时，service journal 会记录工具、退出状态、固定操作标签和
长度受限的 stderr；Setup UI 只接收固定操作标签。

候选人接受小写和 Sym 输入，但按住 Shift 不生成大写字母，并且密码确认返回 `Provisioning service is unavailable`。修复后的 V0.6 BSP 报告普通按住 Shift 直接和 Shell 之前丢弃了 compositor 的 XKB 修改器屏蔽。Shell 现在显式地消耗了按下的 XKB Shift 修改器。
密码哈希和后续网络操作也共享了一个三秒的 Shell 套接字超时，而 yescrypt 在一个大小不足的 16 MiB cgroup 内运行。候选人现在使用了上述描述的特定操作的有界超时和设置时 64 MiB 的守护程序上限。相同的审核还添加了实时以太网 IPv4 报告，有界的 NetworkManager 等待，不支持的 Wi-Fi 安全分类，系统身份冲突拒绝和 SSH-On commit 恢复。

下一次新介质测试进入 Welcome，能够输入小写字母和 Sym `-`，但按住 Shift 仍产生小写，
提交 Device Name 又以 `system locale could not be configured` 失败。固定版本 V0.6 键盘
驱动会在同一个 input sync frame 中发送合成 `KEY_LEFTSHIFT` 按下事件和随后的字母，因此
compositor 可能在客户端可见的 modifier 状态变化前交付字母。镜像现在在固定 BSP 上增加
补丁，用普通的按住 Shift 行为替代 ASMUX 单击/双击/长按状态机。后续 App 输入回归表明，
为 Sym 合成 Shift 会把字符不必要地绑定到 compositor modifier 时序，因此最终映射直接
使用 V0.6 keymap 中独立的符号标识符。Shell 侧 XKB 和原始事件跟踪仍分别保护物理 Shift
和字母大小写行为。

本地化文件存在，并且两个支持的本地化环境都已经生成；失败的边界是受限的能力代理服务的 `localectl` D-Bus 变化。
同一依赖项在后续的时间区步骤中仍然存在。代理现在写入
验证过的 `/etc/default/locale`, `/etc/timezone`，和 `/etc/localtime`
内容本身使用持久临时文件 `fsync`，并重命名语义。
用常规的 `/etc/localtime` symlink 修改链接本身，并且从不通过它写入 `/usr/share/zoneinfo`. 交互代理
和 完成状态启动应用 接收匹配 `/etc` 可写
在它们整体只读的系统视图中包含允许列表。宿主测试断言
确切的区域设置/时区内容，替换行为，以及保留原来的符号链接目标。

在提交了设备/所有者字段之后，以下候选者报告了配置命令在当前状态下无效。Shell 从事件发送的页面而不是从守护进程返回的持久阶段的页面进行前进。因此，一个完成的请求后跟一个丢失的/迟到的响应、服务重启、重复的 Enter 或先前保留的步骤可能会使两个状态机停留在不同的页面上。现在，Shell 在每次成功突变后应用权威状态并执行立即 `GetStatus`
对账后 `InvalidState` 或者 `RepairRequired`. 区域、所有者和密码转换在完成之后是幂等的，并且不能回滚后续阶段；明文缓冲区在返回阶段跨越其持久边界时会被清空。现在，主机测试会在所有六种以太网/Wi-Fi/离线和SSH开/关组合跨步骤重启守护进程。

早期的一个诊断候选者暂时携带了一个物理武装的一靴ED25519维护路径，以便在不重复进行完整介质烧录的情况下隔离配置缺陷。它从未是用户功能。在V0.6的完整Setup流程之后，该路径、启动标记、根sshd、易失性更新器和固定维护mDNS身份都被从量产中移除。现在，挂载的镜像门现在拒绝任何这些文件或单元。在完成之前，无论启动分区的内容如何，端口22都保持关闭；之后，普通sshd需要持久完成标记和显式的SSH同意。

V0.6热更新在`192.168.20.66`运行后，重现了设备名称失败，表现为`invalid provisioning state: symbolic link is not allowed`. Raspberry Pi OS 通常将`/etc/default/locale`安装为指向`../locale.conf`的链接；守护进程的安全写入策略在应用区域设置之前拒绝了该有效目标。现在，区域持久性使用与`/etc/localtime`相同的原子替换原语：它直接替换链接本身而不跟随它，并且回归测试证明前目标未改变。修正后的AArch64守护进程在设备上完成了区域、所有者、yescrypt密码、真实Wi-Fi扫描、以太网选择和SSH同意，并达到了审核阶段。

那次运行暴露了一个最终的交易顺序问题。`ssh.service`需要完成和SSH启用的标记，而旧提交实现则在发布完成之前就开始了它。systemd 正确地跳过了开始，然后临时诊断SSH停止，即使两个标记最终都是持久的，端口22也被关闭了。现在提交会持久化`COMMITTING`，发布完成，激活受控SSH，最后持久化`COMPLETE`。恢复测试涵盖了标记发布前后出现的崩溃情况。那条诊断路径仅保留作为历史测试证据，而不在后续的产品镜像中出现。

对应的量产候选镜像是
`image_2026-08-03-firstboot-stable-d12383c-cp0-os-production.img.xz`；其 mounted-rootfs、
initramfs 门禁和 checksum 验证均通过。设备运行和剩余的新介质检查详见
`FIRST-BOOT-DEVICE-TEST-REPORT-20260803.zh-CN.md`。

### 宿主机和镜像测试

- 安装的产品根目录不包含任何人类UID、固定用户名、密码哈希、授权密钥或构建用户残留物；
- 开发/恢复构件不能被误标为产品构件；
- 状态机测试覆盖了每一种合法和非法转换，密码/SSH 后端失败，以及最终完成标志交易的双方。
- 故障注入中断每个持久写入之前/之后的写入，`fsync`，然后重命名，接着证明恢复或`REPAIR_REQUIRED`行为；
- 协议测试和模糊测试覆盖调用者身份、帧边界、重复字段、无效Unicode/字节以及秘密遮蔽；
- 账户测试证明固定UID范围、预留名称拒绝、PAM/NSS登录，
组成员身份、所有者主目录隔离和OS回滚兼容性；
- NetworkManager 测试涵盖以太网 IPv4 就绪性，开放/WPA2/WPA3 无线局域网，
不支持的企业/WEP 分类，隐藏 SSID，
错误的密码，DHCP 失败，离线模式以及后续链接丢失；
- SSH 测试证明 Off 没有监听/主机密钥，启用的模式只允许所有者访问，root 和应用程序被拒绝，并禁用会移除监听器；
- 无头320x170像素测试覆盖每个页面，长翻译，滚动，焦点，错误，密码遮罩和重启恢复渲染；
- `make check`、镜像门禁和 `git diff --check` 均保持通过。

### V0.6 硬件验收

- 冷启动不带HDMI，网络或先前凭证，在LCD上显示欢迎界面；
- Setup所需的所有键都正常工作，包括编辑、显示/隐藏和返回。
- 以太网、Wi-Fi和离线路径各自独立完成；
- 电源在每种状态下都被移除，并且在提交过程中反复移除而不会创建一个不完整的第二身份或一个无法使用的设备；
- 完成设置后，在连续10次冷启动和普通网络丢失后直接启动Home；从未重新打开设置。
- 选择的密码仅在启用SSH密码模式时才进行身份验证；
- 内存使用、进程数量、启动时间和SD卡写入次数均保持在现有预算之内；
- 工厂复位和OS回滚返回一致状态，诊断过程中无密钥泄漏。

在主机/图像测试通过并且所有者批准烧录/测试窗口之前，不应将任何实现部署到唯一的V0.6设备上。

## 阶段边界

1. **6I-A，合同**：模式，状态机，威胁模型测试和UI模拟渲染器；无特权写入。
2. **6I-B，镜像身份**：移除临时构建用户，集成 extrausers/PAM，提供持久 home 和
   mounted-image 门禁。
3. **6I-C，守护进程**：认证协议，原子状态/身份写入和密钥处理测试。
4. **6I-D，网络和SSH**：共享NetworkManager后端，明确的网络决策和标记控制的远程访问。
5. **6I-E, System Shell**: 可信设置模式，所有页面、错误、恢复和 compositor 激活锁定。
6. **6I-F，韧性**：fuzz、故障注入、镜像构建和宿主机像素验收测试。
7. **6I-G, 硬件**：新介质刻录和全V0.6验收报告。

## 批准的决定

产品负责人于2026年8月2日批准了以下内容：

1. 拥有者默认没有 sudo 权限；开发模式是一个单独的受限提升路径。
2. `Use offline` 允许并视为已完成的网络决策。
3. SSH 默认关闭；密码登录是可选的，并且公钥设置可能在初始密码/离线流程之后进行。
4. 用户名政策，密码最小长度和固定的所有者UID范围以上。
5. 持久所有者家目录包括，而受管理的授权密钥生活在
   `/etc/cardputerzero`.
6. 工厂重置是唯一支持的替换原始所有者的途径；首批发布中不支持多用户账户。
