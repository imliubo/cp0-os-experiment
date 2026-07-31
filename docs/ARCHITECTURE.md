# CardputerZero OS 技术架构

## 1. 目标与约束

目标硬件固定为 CardputerZero V0.6：Raspberry Pi CM0、512 MB RAM、SD 卡和
320x170 LCD。系统只支持单个前台应用，不兼容传统 Linux 桌面应用；第三方开发者
必须使用 CardputerZero SDK。

安全目标是让恶意应用无法直接访问其他应用、硬件设备和未经授权的系统能力。
共享 Linux 内核无法提供数学意义上的“绝对隔离”，因此系统采用 WASM 运行时与
Linux 进程沙箱两层防护。

## 2. 总体分层

```text
Third-party application (.capp / WASM)
        |
Cardputer App Runtime (WAMR, one process per running app)
        |                         \
Cardputer SDK host calls          Wayland surface
        |                           |
Capability services         System Shell + Compositor
        |                           |
appd + permissiond + hardware brokers
        |
systemd + Debian arm64 + DRM/KMS + kernel drivers
        |
LCD / keyboard / audio / battery / camera / LoRa / GPIO
```

## 3. 基础系统

- 初期使用 Debian arm64 minimal 和 systemd，镜像构建复用现有 `pi-gen` 经验。
- 不安装 X11、完整桌面环境、浏览器和本机编译工具链。
- 固定现有 ST7789V DRM/KMS MIPI-DBI 驱动版本；用户应用不能访问 framebuffer。
- 键盘由 libinput/evdev 交给 compositor，应用只收到当前焦点窗口的输入事件。
- 使用 zram，不在 SD 卡上启用常规 swap 或 zram writeback。
- 系统根分区最终设为只读；A/B OTA 和安全启动暂不进入首版范围。

## 4. 图形与窗口模型

首个可运行版本使用 Weston kiosk shell 验证 DRM、Wayland 和应用生命周期。产品
策略由小型 `cardputerzero-policy.so` 模块下沉到 compositor：该模块验证专用 Shell
UID、保留可信系统层并持有全局快捷键。只有在这个受限模块无法满足产品交互时，才
评估维护基于 wlroots 的专用 compositor。

窗口策略固定如下：

- 同一时间只有一个前台应用 surface；后台应用不渲染；
- 普通模式为 320x150，上方约 20 px 系统状态栏；
- 沉浸模式使用完整 320x170；
- 权限、音量、通知和任务切换器由受信任的 System Shell 覆盖显示；
- 渲染目标为 RGB565、最多 30 FPS，并使用 damage region 降低 SPI 刷屏量。

私有 Shell 协议 v4 将覆盖策略固定为 `full`、`status`、`hidden` 和
`notification`。标准应用在可信 21 px ARGB 状态栏下保持焦点；通知模式显示顶部
88 px 可信横幅但不转移应用键盘焦点；权限提示会强制切回不透明 `full` 模式。
沉浸应用只在没有系统覆盖层时使用完整屏幕。全局系统键和显示休眠/唤醒始终由
compositor 掌握，应用无法拦截或伪造这些状态转换。

Weston 与 System Shell 使用不同 UID。两者仅通过 `cp0-wayland` 组共享 Wayland
socket；第三方应用不加入该组。可信 Shell 协议以 Wayland peer UID 认证，不能用
客户端可伪造的 app-id 或 RPC 字符串代替。

## 5. 应用运行时

设备端采用 WAMR AOT，原因是其常驻内存比完整 JIT/Component Model 运行时更适合
512 MB 设备。WIT 是 SDK 的源级类型描述；机器可读的 flat ABI 契约生成 WAMR
注册表与 C/Rust 私有导入，并逐项映射回 WIT 公共操作；
首版不要求设备运行时原生实现 WASM Component Model。

每个运行中的应用由 `appd` 启动到独立沙箱：

- 独立 UID、PID/mount/network namespace 和 cgroup；固定 60% CPU quota 和较低
  CPU weight 防止单核 CM0 被恶意应用完全占用；
- `no_new_privs`、capability 全部移除、seccomp syscall allowlist；
- 只读应用包、一个带配额的私有数据目录、空的设备目录；
- 不暴露 system D-Bus、Wayland socket 路径、evdev、DRM、ALSA 和 GPIO；
- PID 1 只把预连接的 Wayland FD 3 交给受信任的 App Runtime，由它代理 SDK 调用。
- Runtime 仅在 `wl_keyboard.enter/leave` 焦点区间向有界 SDK 队列写入按键事件。

System Shell 通过认证后的 appd 控制 socket 获取已安装 manifest 摘要，不从 Wayland
surface 推断安装状态。Launcher 的 app ID、显示名称和标准/沉浸策略来自 root 控制
的 manifest；compositor token 只表示一个临时映射 surface。启动由 appd 完成，Shell
等待两种身份在可信事件中匹配后才激活 token。Home 只隐藏单运行槽，Tasks 可恢复
或通过 appd 停止它。

App Runtime 在应用线性内存与 Wayland surface 之间传递 RGB565 帧，并转换到
Weston 的 XRGB8888 SHM buffer。标准应用只能提交 320x150，沉浸应用可提交
320x170；可信 shadow frame 和两个 compositor buffer 的总量仍低于 700 KiB。

## 6. 权限与能力服务

应用身份来自经过签名验证的包 ID 和运行进程凭据，不能由 RPC 参数自行声明。
私有存储、当前窗口输入和渲染是隐式能力；其他能力必须写入 manifest。

首版权限词表包括网络客户端、文档选择、音频播放/录音、相机、LoRa、GPIO、
剪贴板读取和通知。敏感能力由 `permissiond` 在首次使用时显示系统弹窗，可选择
本次允许、始终允许或拒绝。

当前实现由 `appd` 内部权限协调器承担 permissiond 职责。System Shell 只能通过
经过 `SO_PEERCRED` 认证的控制 socket 读取和解决单个待处理提示；诊断控制器可将
持久决策原子重置为“下次询问”，第三方应用没有此管理接口。

通知 broker 同样从 `SO_PEERCRED` 和当前 systemd cgroup 绑定应用身份。`appd` 只
向可信 Shell 返回规范应用名称和有界内容；Shell 决定横幅布局与四秒显示周期。
权限提示优先于通知，Home、Tasks、Power 和应用退出会撤销当前横幅。

网络能力不向应用暴露裸 socket。Runtime 和 appd 仅允许 `AF_UNIX`，独立的
`cp0-networkd` 是唯一允许 `AF_INET/AF_INET6` 的组件。首版 SDK 只提供 1024 字节
URL、5 秒总超时、2 次重定向和 2048 字节响应体的同步 HTTPS GET。networkd 禁用
环境代理，并在每次连接和重定向解析时过滤环回、私网、链路本地、多播、保留地址、
NAT64/Teredo 等非公网目标，TLS 证书校验不可关闭。appd 在网络 I/O 前完成调用者
UID/cgroup、manifest 和权限验证并释放共享状态锁。

文件共享不暴露路径。`cp0-documentd` 以专用账户只读枚举最多 16 个共享文档，可信
Shell 显示单前台选择器；应用请求不包含路径或文档 ID。选择结果先绑定到 appd 的
可信快照，再由 documentd 使用 `openat(O_NOFOLLOW)` 和 device/inode 二次校验打开。
只读 FD 通过两段 `SCM_RIGHTS` 传入 Runtime，WASM 只得到单个代际句柄、文件长度和
每次最多 4096 字节的偏移读取 API。首版文档上限为 16 MiB。

音频能力不暴露 ALSA 设备或 mixer。`cp0-audiod` 是唯一可访问 `char-alsa` 的服务，
固定打开 ES8389 的 `hw:ES8389Audio,0`，只接受 16 kHz、单声道、S16_LE PCM。
`audio.playback` 和 `audio.capture` 分别授权，每次最多 1024 帧（64 ms、2048 字节）；
协议、appd broker、Runtime 线性内存和 SDK 四层都重复验证长度与偶数字节边界。
服务使用专用账户、root-only Unix socket、空 capability 集和 systemd 设备白名单。

相机能力不暴露 V4L2、Media Controller、dma-heap、VideoCore 设备或捕获进程。
`cp0-camerad` 以专用账户运行并通过 systemd 白名单独占这些设备类，固定调用系统
`rpicam-still` 捕获 320x170 RGB888 后转换为 RGB565_LE。每次调用只返回一个
108800 字节的密封 memfd；appd 验证只读访问、普通文件类型、精确长度和全部写入
封印后才转发，Runtime 将内容复制进 WASM 线性内存，应用永远看不到原生 FD。

GPIO 能力不暴露 `/dev/gpiochip*`、BCM 引脚编号、sysfs 路径或任意方向/复用设置。
V0.6 首版只提供 overlay 明确定义的四个逻辑布尔输出：Grove 功能、外部 USB 功能、
Grove 5V 电源和外部 5V 电源。`cp0-gpiod` 是唯一可写对应四个 LED-class 属性的
账户；app-platform 阶段将 BSP 原有的全局 `0666` 模式覆盖为 `0660 root:cp0-gpio`。
LCD、SPI 片选、音频、红外、键盘、耳机检测和系统电源相关 GPIO 永不进入 SDK。

LoRa 能力仅面向外接 SX1276 系列模块；V0.6 本身没有板载 LoRa。`cp0-radiod` 是
唯一可访问 SPI0 CS1 (`/dev/spidev0.1`) 的账户，应用和 Runtime 不能选择设备节点、
频点、调制参数、发射功率或寄存器。镜像默认 `enabled=false`，只有 root-owned 配置
能选择受支持地区及该地区范围内的频点。首版固定 125 kHz、SF7、CR4/5、CRC、
8 字节前导码、私有同步字 `0x12` 和 14 dBm；报文最多 64 字节，每次发送至少间隔
15 秒，单次接收等待不超过 1000 ms。

应用间调用通过 appd 内部 Intent Broker 路由，接收方必须在 root-owned manifest
显式导出受限反向域名 action，不提供任意应用间 socket、目标应用 ID 或路径。发送
payload 上限为 1024 字节，全局队列最多 8 条；没有接收方或存在多个接收方都会拒绝，
不会静默选择。appd 先把接受响应写回发送方，再停止发送方、启动唯一接收方；接收方
使用经过 UID/PID/cgroup 认证的一次性 `take` 获取绑定到自身的消息。响应写入失败会
撤销对应队列项，从而保持消息接受与单前台切换顺序一致。

应用私有数据只通过 SDK key/value API 访问。`cp0-storaged` 独占
`/var/lib/cardputerzero/data`，appd 根据经过 UID/cgroup 认证的调用者和 root-owned
manifest 传递应用 ID 与 `storage_mb` 配额。每个值最多 8 KiB、每个应用最多 256 个
键，写入先核算替换后的总逻辑字节数，再使用同目录临时文件、`fsync` 和原子重命名。
Runtime 的 `/data` 是沙箱内空目录，不再绑定任何宿主可写路径；即使 Runtime 被攻破，
也不能绕过 broker 读取其他应用数据或直接消耗 SD 空间。

## 7. 包与应用商店

`.capp` 是签名的不可变应用包，至少包含 `app.json`、WASM/AOT 模块、资源和签名。
商店签名与开发者签名分离：开发者负责来源身份，商店在审核后为可安装产物签名。

发布工具将审核记录绑定到开发者签名提交的完整 SHA-256、manifest 权限和实际 WASM
imports，再生成确定性的商店签名包和 Ed25519 签名目录。设备端 `cp0-stored` 只接受
HTTPS 公网地址，验证目录序列、有效期和签名，支持有严格 `Content-Range` 校验的断点
续传，并在下载后校验大小与 SHA-256。

`cp0-stored` 使用独立 `cp0-store` UID，只能写自己的私有缓存和
`/run/cardputerzero-appd/store` 交接目录。Shell 只能发送 list/refresh/install-app-ID，
不能指定 URL、路径、哈希或版本。appd 只接受固定 Store UID 的交接命令，并再次独立
校验文件身份、manifest、双签名和严格 SemVer 升级，随后原子安装到
`/var/lib/cardputerzero/apps/<app-id>/<version>`。目录默认未配置且镜像不内置生产
信任根；详细边界见 `docs/PHASE5B-APPLICATION-STORE.md`。

root-owned `device-policy.json` 为家长/组织管理提供本地策略上限：可锁定开发者/恢复
模式、禁用 Store 安装、限制可启动应用并全局拒绝 SDK 权限。appd 在安装、启动和
每次 capability 请求处执行策略；全局拒绝优先于用户已有的持久允许。System Shell
只能在策略允许时切换两个固定模式，不能提交路径、应用白名单或权限文本。

开发者模式仍要求受信任开发者密钥和有效签名。恢复模式使用持久 root marker 在下次
启动同时阻止 compositor 并拉起 `getty@tty1`，可从本地键盘控制台通过
`sudo cp0ctl device recovery off` 关闭。详细配置和恢复步骤见
`docs/PHASE5C-DEVICE-POLICY.md`。

## 8. 512 MB 内存预算

| 模块 | 目标上限 |
|---|---:|
| 内核、systemd 和基础服务 | 100 MB |
| compositor、Shell、字体和图形缓冲 | 55 MB |
| appd、权限及硬件代理 | 30 MB |
| 单个 App Runtime 与应用 | 96 MB |
| 文件缓存、zram 和突发余量 | 231 MB |

首页空闲常驻内存目标低于 220 MB，应用运行时总使用目标低于 360 MB。超出 manifest
资源上限的应用由 cgroup 限制并由系统 Shell 报告终止原因。

CM0 固件内存划分固定为 64 MB VideoCore、448 MB ARM，VC4 CMA 固定为 64 MB。
memory cgroup 必须启用，否则 `appd` 无法执行 manifest 内存上限。

## 9. 信任边界

内核、compositor、System Shell、App Runtime、appd 和能力服务属于可信计算基。
第三方 WASM、应用资源、网络响应和应用商店内容均视为不可信输入。原生第三方
可执行文件不属于支持范围；开发模式也只安装未上架的 WASM 应用。

## 10. 故障恢复与稳定性

appd 和 compositor 使用 `Restart=on-failure`，System Shell 使用
`Restart=always` 并通过 `BindsTo` 跟随 compositor 生命周期。恢复验收必须同时确认
新 PID、预期重启计数、Shell 对新 Wayland socket 的重绑以及 appd 控制路径，不能只
检查 systemd 的 `active` 字符串。

24 小时验收按分钟将核心服务状态、重启数、cgroup 内存和 socket/ping 健康写入
`/run` 的独立结果目录，避免持续写入 SD 卡。监控工具有固定的 32/32/24 MiB 内存
上限和结束增长阈值，但不会在产品启动时常驻。

Phase 6 性能门禁同样只写 `/run`，并在无前台应用时统一记录 systemd 单调启动时间、
空闲内存、核心服务 CPU/内存、短时 SD 写入和 BQ27220 电池侧遥测。电池 gauge 在 USB
供电时不代表整机功耗，因此只作为信息，不替代校准的外部功率测量。

Phase 6 的设备诊断同样不常驻且不自动上传。默认支持包只导出无设备/用户标识的
硬件存在性、服务属性、资源和挂载状态；原始 journal 必须由 root 使用独立参数显式
加入，并在包内标为敏感。量产验收只读检查不可变根、`cp0-data`、固定服务和 socket
权限，结果仅写入 `/run`。详细数据边界见
`docs/PHASE6B-DIAGNOSTICS-FACTORY.md`。

独立恢复镜像使用 root-owned `image-profile=recovery` 标记，显式移除 OverlayFS
参数并 mask compositor、System Shell、appd 和全部 capability socket，只保留 tty1、
LCD 启动摘要、网络和 SSH。恢复启动不会自动扩容、挂载或绑定 `cp0-data`，避免把
待修复的安全状态隐式带入可写维修根；详见 `docs/PHASE6C-RECOVERY-IMAGE.md`。

完整持久数据迁移使用版本化 `CP0 backup v1` 流格式，而不是通用归档解包。格式只接受
`cp0-data-layout-v1` 白名单，记录权限/所有者并对每个文件及完整 payload 做 SHA-256，
拒绝链接、特殊文件、路径逃逸、危险权限和非空恢复目标。设备包装器仅在独立 recovery
或 product lower-root 维护启动下挂载分区；恢复在完整校验和固定确认词之后才重建
`cp0-data`。产品镜像带自身可信 factory seed，恢复镜像不复制不完整的产品信任根。
详细边界见 `docs/PHASE6D-RECOVERY-DATA.md`。

系统级安全声明由 `docs/THREAT-MODEL.md` 约束：应用隔离不等于抵抗内核、可信原生
服务或物理 SD 攻击。当前 OverlayFS 是运行期写保护，不是启动完整性机制；开发镜像
的 SSH/显式密码也不是量产身份方案。`production` access profile 因而拒绝外部密码
和 SSH key，锁定产品内账户、SSH/getty、Developer Mode 与 Recovery Boot；管理员只
能通过独立、显式插入的 recovery SD 维护，移除介质即撤销访问。未来 OS 更新采用独立
签名根、A/B boot/root、dm-verity 和健康确认后提交。Phase 6H 已实现不接入启动链的
发布策略、verity 产物校验和三次失败回滚状态机；启动前必须先把递减后的双副本状态
持久化，启动后只有 compositor、appd 和 `cp0-data` 都健康才能确认新槽。校验和只能
检测撕裂写入，只有在更早的不可变阶段能够认证 U-Boot/FIT 时才形成可验证启动链；详见
`docs/PHASE6G-PRODUCTION-ACCESS.md` 与
`docs/PHASE6H-VERIFIED-UPDATE-GROUNDWORK.md` 以及
`docs/adr/0006-verified-updates-and-rollback.md`。
