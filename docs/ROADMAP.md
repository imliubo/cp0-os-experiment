# CardputerZero OS Roadmap

Roadmap 以“每个阶段都有可在真机验证的交付物”为原则。日期在完成 CM0 基准测试后
再估算，当前只冻结依赖关系和完成条件。

## Phase 0：架构与开发基线（基础完成）

- [x] 建立独立 Git 仓库和 Rust workspace。
- [x] 冻结硬件约束、信任边界和内存预算。
- [x] 定义 app manifest v1、权限词表和校验器。
- [x] 建立 WIT SDK ABI 草案与示例应用清单。
- [x] 增加基础 CI、SDK/manifest 版本策略和架构测试。
- [x] 加入扁平 ABI 契约解析、三端生成和已发布签名兼容性测试。
- [x] 使用 `wasm-tools` 同源标准 `wit-parser` 完成 WIT 全量语法与名称解析。

完成条件：`make check` 可重复通过，manifest 和 ABI 的兼容规则有自动化测试。

## Phase 1：Board Support Package 与镜像

- [x] 采集 CM0 V0.6 当前镜像、内存、DRM、输入、音频、电池和服务基线。
- [x] 确认 ST7789V 已使用 DRM/KMS MIPI-DBI，验证 320x170 RGB565 connector。
- [x] 固定 BSP 源码提交、V0.6 overlay、kernel module 构建入口和启动参数。
- [x] 建立最小 Debian arm64 `pi-gen` 外部阶段和服务裁剪策略。
- [x] 建立可回滚启动配置安装器与真机 smoke test。
- [x] 在现有系统真机验证 memory cgroup、AppArmor 和全部设备接口。
- [x] 构建首个无 Launcher/桌面环境的精简镜像并完成离线内容验收。
- [x] 烧录精简镜像，验证 64 MB GPU/CMA，并测量启动时间、空闲内存和 SD 写入。
- [x] 加入 OverlayFS 只读底层根文件系统、initramfs fail-closed 启动和启动后挂载
  自检；三分区产品镜像默认启用，显式移除内核参数时进入可写恢复模式。

完成条件：冷启动进入 DRM 测试画面，所有输入输出设备通过自动化 smoke test，系统
识别至少 400 MB RAM，首页前的基础系统空闲内存低于 180 MB。

## Phase 2：Compositor 与 System Shell

- [x] 固定并裁剪构建 Weston，真机验证 DRM/Pixman kiosk 与单前台测试客户端。
- [x] 使用专用 seat 隔离内部 LCD/键盘，并建立 320x170 headless 测试后端。
- [x] 实现 Phase 2A 原生 Wayland System Shell：首页、状态栏、系统电源弹窗骨架、
  双缓冲 renderer、定时状态刷新和崩溃自动返回首页。
- [x] 实现 Phase 2B Weston policy 核心：独立 compositor/Shell UID、peer UID
  认证、可信层和全局 Home/Back/Tasks/Power 动作通道。
- [x] 将 Launcher、状态栏和权限弹窗迁移到 compositor 侧可信覆盖层，并实现沉浸模式。
- [x] 在 compositor 统一键盘焦点、全局 Home/Back/任务切换快捷键和屏幕休眠。
- [x] Launcher 从 appd 枚举已安装应用，支持启动后自动前台化，以及 Tasks 恢复/停止。
- [x] 实现可信通知横幅、应用焦点保持、权限弹窗优先级和通知像素回归。
- [x] 建立像素级截图回归测试并完成两个客户端的可靠切换与 200 轮压力测试。
- [x] 建立核心服务 SIGKILL 恢复测试和 RAM-backed 24 小时稳定性/内存采样工具。
- [x] 建立独立稳定性证据验证器，交叉检查原始服务/块写入样本、持续时间、PID、
  重启、内存增长、汇总字段和 64 MiB SD 写入边界，拒绝缺失或伪造的 `PASS`。
- [x] 完成 Launcher 启动、Tasks 停止及 F1/F2/F3/F4 全局动作的最终物理按键验收。
- [x] 完成 Home 五个可信系统视图的信息架构、本机遥测、详情交互和 320x170
  像素回归；分项计划见 `HOME-SYSTEM-APPS-ROADMAP.md`，真机部署延后到
  2026-08-02 00:45 CST 之后。
- [x] 在本机完成手机式系统设置、完整应用管理、硬件诊断和官方 Fn 快捷键所有权；
  compositor 所有的 ESC 长按 Home 和 Shell-only 背光 broker 已完成本地实现和测试，
  其余受限硬件写服务及真机验收按 `SYSTEM-EXPERIENCE-ROADMAP.md` 的 X4 阶段继续，
  当前未部署真机。
- [x] 完成本地 Store Today/Apps/Search/Updates、小屏物理键盘搜索、最近查询、
  有界分页、stale 浏览门禁和严格 SemVer 更新计算；真机部署等待当前稳定性周期结束。
- [ ] 完成 24 小时 compositor/Shell/appd 真机稳定性、内存泄漏及 SD 写入验收；
  先前运行因设备重启或应用安装失效；当前无前台应用的新周期从
  2026-08-01 00:43 CST 运行至约 2026-08-02 00:43 CST。

完成条件：两个测试 Wayland 客户端可可靠切换，应用无法读取非焦点输入或覆盖系统
权限弹窗，连续运行 24 小时无 compositor 泄漏。

## Phase 3：应用运行时与隔离

- [x] 集成 WAMR interpreter/AOT，App Runtime 成为受控 Wayland 客户端。
- [x] 实现 `appd` 生命周期、每应用 UID、namespace、seccomp 和 cgroup。
- [x] 冻结首版 `appd` 沙箱启动契约：稳定应用账户、systemd cgroup、bubblewrap
  namespace、只读应用包和唯一可写私有数据目录。
- [x] 固定并静态构建 WAMR 2.4.5，真机贯通每应用 UID、systemd cgroup、
  bubblewrap namespace、runtime seccomp 和 WASM 执行。
- [x] 实现 socket-activated `appd` 核心生命周期：可信安装注册表、规范路径解析、
  root/Shell peer UID 认证、单运行槽、分页 list 及 start/stop 真机闭环。
- [x] 将 appd、双 socket、静态 Runtime、稳定测试 UID/注册表和 SDK 示例集成到
  `pi-gen`，新开发镜像默认启动 compositor/Shell。
- [x] 实现 manifest 权限数据库、会话/持久决策和可信 Shell 提示控制协议。
- [x] 实现首个 `notifications.post` typed capability broker、peer UID 身份绑定和
  有界 Shell 通知队列。
- [x] 将 `notifications.post` 接入 WAMR host call，包含线性内存边界、Unix-only
  seccomp 和真机 WASM 调用闭环。
- [x] 将通知队列接入可信 System Shell，在标准/沉浸应用上显示有界横幅且不窃取
  应用键盘焦点。
- [x] 实现 `network.client` HTTPS-only broker、SSRF/DNS rebinding 防护、WAMR
  host call 和 Rust/C/C++ SDK 有界响应 API。
- [x] 实现无路径 Document Portal、可信文件选择器、`SCM_RIGHTS` 只读 FD 传递和
  Rust/C/C++ SDK 有界读取 API。
- [x] 实现 ES8389 有界 PCM 音频 broker、播放/录音分权和 Rust/C/C++ SDK API。
- [ ] 稳定性监控结束后完成音频播放、录音与拒绝权限的真机验收。
- [x] 实现固定 320x170 RGB565 帧的相机 broker、只读密封 FD 传递和三语言 SDK API。
- [ ] 连接兼容传感器后完成相机捕获、拒绝权限和画面方向的真机验收。
- [x] 实现 V0.6 四路逻辑输出 GPIO broker，排除所有板载关键引脚和原始 gpiochip API。
- [ ] 稳定性监控结束后完成 GPIO 读写、权限拒绝及 sysfs 权限收紧的真机验收。
- [x] 实现外接 SX1276 LoRa broker，固定 SPI/调制参数、地区频点边界、发送限速、
  `radio.lora` 授权和 Rust/C/C++ SDK API；镜像默认禁用。
- [ ] 连接 SX1276 模块并确认当地合法频点后完成收发、限速和拒绝权限真机验收。
- [x] 实现 Intent Broker：manifest 显式导出、唯一接收方路由、8 条有界队列、响应后
  单前台切换，以及 Rust/C/C++ SDK 一次性 `take` API。
- [x] 实现应用私有存储 broker 与 manifest 配额，移除 Runtime 的宿主数据目录挂载，
  提供原子有界 key/value Rust/C/C++ SDK API。
- [x] 建立经过真实应用 UID/cgroup/权限链路的音频、GPIO、存储配额与跨应用隔离
  真机验收工具，并用稳定性测试互锁避免污染 24 小时基线。
- [ ] 稳定性监控结束后完成存储持久化、配额拒绝及应用间读取隔离真机验收。
- [x] 建立恶意应用测试集，覆盖 WASI ambient authority、路径逃逸、设备访问、任意
  IPC、seccomp 旁路和 cgroup 资源耗尽，并纳入 `make check`。

完成条件：测试应用只能使用被授予的能力；拒绝权限后没有旁路；OOM/崩溃不会影响
Shell 和其他应用数据。

## Phase 4：SDK 与开发体验

- [x] 建立首个 `no_std` Rust SDK，封装系统时钟、事件等待、通知 capability 和
  稳定错误类型；Hello 示例已移除私有 FFI。
- [x] 建立 freestanding C11/C++17 SDK 头文件并加入 wasm32 双语言编译测试。
- [x] 发布完整 Rust 和 C/C++ SDK，从统一 ABI 契约生成 WAMR/C/Rust bindings，
  并提供 LVGL 9 320x170 适配层。
- [x] 实现 `cp0ctl new/build` 的 SDK-only 项目生成、Cargo metadata 解析和规范产物树。
- [x] 实现 `cp0ctl run/package/sign/install/logs`，包含 PC 到真机的受限 SSH
  forced-command 安装/日志路径，不依赖 scp、sudo 或完整 Shell。
- [x] 建立 PC WASM 模拟器、权限模拟、evdev 输入映射和 JSON 性能分析工具。
- [x] 迁移 Calculator、Camera 示例，不提供传统 Linux 应用兼容层。
- [x] 冻结 SDK 1.0 ABI、精确 legacy 0.1 兼容策略、权限词表和开发者文档。

完成条件：新开发者可只使用 SDK 在 PC 编写、调试、签名并安装一个应用到真机。

## Phase 5：应用包与商店

- [x] 冻结 `.capp` v1 可复现容器，实现开发者签名和独立商店审核签名。
- [x] 完成 root-owned 信任目录、双身份吊销和显式开发者模式验签策略。
- [x] 实现 `.capp` 原子安装、升级历史、断电恢复式重试和双向回滚。
- [x] 实现设备端 320x170 商店列表/详情、安装进度、已安装版本对账和离线状态。
- [x] 实现独立 `cp0-stored`、HTTPS 公网限制、签名目录、防回滚和断点下载恢复。
- [x] 建立 WASM 静态扫描、权限/import 审查元数据绑定和确定性发布工具。
- [x] 扩充商店协议/下载器确定性变异测试与针对目录、Range 响应的恶意样本集。
- [x] 增加家长/组织策略和面向用户的开发者模式/恢复模式开关。
- [x] 实现默认关闭、策略独立控制、无设备身份的 Store 周聚合指标，覆盖安装、启动、
  崩溃计数、精确重试去重和 20 批次公开阈值。
- [x] 实现非生产内容治理技术切片：无身份/自由文本的结构化举报、有界 SLA 队列、
  Team 隔离开发者通知、一次性申诉和 append-only PostgreSQL 审计；自动下架、正式 SLA、
  政策批准和外部值班仍按 Store Roadmap S8 的生产门禁推进。
- [x] 建立开发者签名双版本测试商店、精确审核元数据、受控公共 HTTPS 验收源和带
  稳定性互锁的真机验收器，覆盖刷新、HTTP Range 续传、安装、升级、离线缓存及
  过期拒绝。
- [ ] 配置测试商店端点后完成刷新、断点续传、安装、升级和离线目录真机验收。

完成条件：商店应用只能由可信签名安装，断电不会产生半安装状态，旧版本可回滚。

## Phase 6：产品化与后续安全

- [x] 为应用 cgroup 增加固定 CPU quota/weight，在 Runtime 强制 30 FPS，并建立只写
  RAM 的启动、空闲 CPU/内存、短时 SD 写入与电池遥测性能验收器。
- [ ] 稳定性监控结束后运行性能验收器，并使用校准的外部 USB 功率计完成定义工况下
  的整机功耗验收。
- [x] 将 journald、临时目录和稳定性报告保持在 RAM，增加 64 MiB/24h SD 写入验收。
- [x] 加入内核 sysctl 与 compositor/Shell/appd systemd 产品安全加固基线。
- [x] 实现独立 `cp0-data` 分区、initramfs 幂等扩容、持久路径白名单和默认不可变
  底层根文件系统；loopback 扩容/重入与最终镜像发布门禁已通过。
- [ ] 烧录三分区候选，完成首次扩容、重启持久性、断电恢复和 24 小时 SD 写入真机
  验收后关闭不可变根产品化条目。
- [x] 建立默认不联网的本地脱敏支持包、显式同意的原始日志模式和 RAM-only
  诊断结果，排除应用数据、网络/设备身份与密钥。
- [x] 为工厂、性能、能力/持久性和六步 Store 真机验收建立独立离线证据验证器，
  从原始 TSV/JSON 重算关键指标并拒绝伪造 `PASS`、跨启动或乱序结果。
- [x] 建立 V0.6 只读量产验收器，检查固定硬件、不可变根、数据分区、核心服务、
  socket 权限和 appd 控制路径；真机执行随三分区镜像烧录验收进行。
- [x] 增加独立 `recovery` 镜像构建 profile：可写维修根、tty1/LCD/SSH、禁用
  compositor 与全部应用入口、独立产物名和双 profile 最终镜像门禁。
- [x] 实现有界 `CP0 backup v1`、双遍完整性校验、只向空目标恢复、分区/维护模式门禁
  和与产品信任根绑定的恢复出厂设置流程。
- [ ] 烧录恢复介质并完成备份/恢复/出厂重置、真机启动及量产工位物理输入输出测试。
- [x] 完成系统威胁模型、量产阻断项和 dm-verity、RAUC A/B、U-Boot/FIT、硬件信任根
  的条件式架构评估；当前开发镜像不宣称 verified boot。
- [x] 为 manifest、`.capp`、Store、appd 控制帧和恢复备份建立 libFuzzer/ASan 入口、
  有界本地 smoke 和定期 CI。
- [x] 增加独立 production access profile：拒绝构建时共享密码和 SSH key，锁定
  getty/恢复模式；个人 Owner 可物理开启受限 Developer Mode，完整 Owner SSH Shell
  继续独立且默认关闭，root 维护仍以 recovery SD 为物理入口。
- [ ] 在可擦写介质上启动 production access 候选，确认默认无监听、Developer Mode
  forced-command、Owner SSH Shell 独立开关、tty/root/sudo 拒绝和恢复入口锁定。
- [x] Phase 6I-A：冻结首次开机状态机、`cp0-provisiond` 有界协议、320x170 Setup
  页面及像素测试；详细方案见 `FIRST-BOOT-PROVISIONING.md` 和 ADR 0007。
- [x] Phase 6I-B：从最终 product rootfs 清除 pi-gen 临时人类账户，引入位于
  `cp0-data` 的 extrausers/PAM owner 身份、持久 home，并增加零固定凭据镜像门禁。
- [x] Phase 6I-C：实现仅接受精确 Shell UID 的 root provisioning daemon、原子状态机、
  yescrypt 密码处理、断电恢复和 `REPAIR_REQUIRED` 一致性检查。
- [x] Phase 6I-D：实现以太网/Wi-Fi/明确离线三种网络决策、NetworkManager 扫描连接
  后端，以及默认关闭、由持久 marker 控制的 owner-only SSH。
- [x] Phase 6I-E：在 trusted System Shell 实现全图形化开机引导；未完成时阻止 Home、
  Tasks 和普通 App 激活，完成后临时断网不得重新进入引导。
- [ ] Phase 6I-F：通过协议 fuzz、秘密泄漏检查、每个原子写入点的断电故障注入、全部
  页面像素回归、PAM/NSS 和 product/development/recovery 镜像分类门禁。
  当前 `make check`、代表性 Setup 像素、Linux/arm64 顺序包/peer credential、完整
  Shell 链接及 Debian 13 NSS/PAM 密码认证已通过；全写入点故障矩阵、最终 product
  rootfs 挂载检查和全部页面/长文本像素仍待候选镜像阶段关闭。
- [ ] Phase 6I-G：在全新 SD 卡完成无 HDMI/SSH 的 V0.6 首次启动、全部键盘路径、
  三类网络、SSH 拒绝/允许、逐阶段断电、十次冷启动和出厂重置验收报告。
- [x] Phase 6J-A：实现 root `cp0-devd` 有界协议、Owner UID 与 trusted Shell UID
  分权、逐请求 Developer Mode/policy 检查、配对签名 key 和 forced-command SSH key。
- [x] Phase 6J-B：将 PC 端 `cp0ctl pair/install/logs/app` 改为 `ssh -T cp0-dev`
  流式协议，移除 scp、sudo、通用上传和远程 Shell 依赖。
- [x] Phase 6J-C：在 320x170 Security UI 实现 Developer Mode、10 分钟
  `PAIR NEW COMPUTER`、最多 8 台配对电脑列表以及单个/全部撤销；Owner SSH Shell
  保持独立且默认关闭。
- [x] Phase 6J-D：将 devd、SSH gate/dispatcher/path unit、持久配对状态和信任目录
  纳入 product 镜像，同时在 recovery profile 显式 mask；增加 Rust/C、像素和镜像门禁。
- [ ] Phase 6J-E：在 V0.6 production 候选完成密码首次配对、key 复用、窗口超时、
  单个/全部撤销、Developer Mode Off、Owner Shell 独立开关、正常重启持久性和真实
  OpenSSH `SSH_ORIGINAL_COMMAND` 行为验收。
- [x] Phase 6K-A：实现 root cp0-powerd、严格有界的 restart/power-off 协议、
  Shell UID 双重认证、固定 systemctl 参数、System Shell 客户端及 product/recovery
  镜像门禁，不向 Shell 或应用授予通用 systemd 权限。
- [ ] Phase 6K-B：在 V0.6 新 product 镜像从确认 UI 分别验证正常重启、新 boot ID、
  返回 Home，以及完整关机后只能通过物理上电恢复。
- [x] Phase 6L-A：冻结单前台/十 task、FIFO 容量淘汰、MRU 切换、五态生命周期和
  checkpoint/resource 安全边界；实现 appd protocol v2、多 session 状态机、F3 卡片
  模拟 UI、SDK lifecycle ABI 及随机模型测试。
- [x] Phase 6L-B：把多任务候选整合到最新 main，保留首次开机、Developer Access、
  Store 和 cp0-powerd 行为；关闭 UI 64 KiB 超限、非驻留 task 包变更及旧 surface
  token 三项合入回归，并形成 `MULTITASKING-MERGE-REPORT.md`。
- [ ] Phase 6L-C：接线原子 TaskJournal、appd 重启 reconciliation、Runtime 认证控制
  socket 与 compositor `(task_id, runtime_generation)` surface 绑定；故障时保持可信
  Shell，不得按 app-id 或 UID 猜测代际。
- [ ] Phase 6L-D：实现 compositor 密封 RGB565 缩略图、2 Hz 限频和 Shell 只读接收，
  完成 0/1/3/10 task 像素、旧代际、伪造身份和内存上限测试。
- [ ] Phase 6L-E：接入 WAMR 8 KiB/250 ms/fuel-bounded checkpoint/restore、私有 broker
  namespace、第 11 App FIFO 淘汰及升级版本兼容策略；无回调 App 必须 clean restart。
- [ ] Phase 6L-F：在 CM0 测量 1/3/10 App 的 RSS、CPU、SPI、SD 写入和切换延迟，确定
  background/freeze/checkpoint 阈值并验证前台能力租约撤销。
- [ ] Phase 6L-G：经授权部署同版本 appd/Shell/compositor/Runtime bundle，正常重启后
  验证 F3、Intent、开发者安装/停止/卸载、11th-App、appd 重启和断电恢复；通过后才
  进入新镜像发布验收。
- [x] Phase 6M-A：实现 Photo Library v2 分页索引、v1 原位迁移、单帧原子 blob、
  appd 原子导入/删除事务、断电尾部恢复、storaged 启动清理、SD 系统保留空间和
  Gallery 八张分页缓存；取消 32 张淘汰及 Shell PNG 重复副本。
- [x] Phase 6M-B：在 appd 生命周期层保护 Camera/Gallery 不可卸载，并向 Shell 暴露
  `removable` 元数据；Store 签名升级保持可用。
- [ ] Phase 6M-C：实现独立于 Developer Mode/Owner Shell 的 Owner Photo Transfer，
  包含独立配对密钥、限时 UI、只读分页/断点协议、`cp0ctl photos pull`、PNG/hash 和
  V0.6 大图库验收；协议冻结见 `PHOTO-TRANSFER-V1.md`。
- [x] 实现不接入当前启动链的 OS 发布元数据策略、rootfs/hash tree/FIT 摘要门禁、
  dm-verity 离线验证、三次启动回滚状态机、双副本撕裂写检测和 100 轮断电模型；
  RAUC CMS、签名 FIT 与硬件信任根仍是独立启用门禁。
- [ ] 在备用硬件/可擦写 SD 上实现并验证 A/B、verity、签名启动元数据、故障注入和
  自动回滚；不得在唯一 V0.6 设备上写入不可逆 OTP 状态。
- [ ] 委托第三方安全评审，跟踪并关闭或由产品负责人明确接受全部发现。
