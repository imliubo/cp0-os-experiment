# Phase 1：CM0 V0.6 BSP 与最小镜像

<!-- doc-locale: zh-CN -->
> [English](PHASE1-BSP.md) | **简体中文**

## 真机基线（2026-07-30）

设备运行 Debian 13.6、Linux `6.18.34+rpt-rpi-v8`，固件报告 512 MB 总内存，但当前
划分为 ARM 256 MB、VideoCore 256 MB。Linux `MemTotal` 只有 233008 KiB，且启动
参数含 `cgroup_disable=memory`。这会同时破坏应用容量和 cgroup 内存隔离。

VC4 overlay 还请求 256 MB CMA；因为 ARM 区域不足，内核回退到 8 MB CMA，并持续
出现相机/DRM CMA 分配失败。Phase 1 配置因此采用：

- `gpu_mem=64` 与 `gpu_mem_512=64`：后者覆盖 CM0 固件的 512 MB 专用默认值，
  ARM 可获得约 448 MB；
- `dtoverlay=vc4-kms-v3d,cma-64`：为 DRM/相机保留合理 CMA；
- 在镜像构建时用 `fdtget`/`fdtput` 精确删除基础 CM0 DTB 内置的
  `cgroup_disable=memory` token，保留其他 firmware bootargs；
- `cgroup_memory=1 cgroup_enable=memory`：启用 memory cgroup controller；
- `apparmor=1 security=apparmor`：启用内核已编译的 AppArmor 主 LSM。

真机已经存在可用的 SPI DRM 设备：`/dev/dri/cardputer-zero-internal` 指向 `card1`，
connector `card1-SPI-1` 为 connected，模式为 320x170，fbdev compatibility 层为
RGB565。因此本阶段固定现有驱动，不再重新迁移显示栈。

## BSP 来源

驱动源码固定到：

```text
repository: https://github.com/m5stack/m5stack-linux-dtoverlays.git
commit:     c3b254819307c177a34100b66fe19e52059ce8c4
profile:    CONFIG_CARDPUTERO_V0_5=y
```

上游用 V0.5 编译开关生成 `cardputerzero-v5-overlay`，该 overlay 也是 V0.6 真机当前
使用的硬件描述。构建脚本必须校验 commit，不允许使用浮动 HEAD。镜像在这个固定提交
上应用上游 `origin/test` 的显示稳定性参数，将 LCD SPI 从 60 MHz 限制为 20 MHz，
同时保留主线更新的键盘修复。

V0.6 的 IMX219 电源使能接在 M5IOE1 的 P12（GPIO offset 11），不是旧版硬件使用的
SoC GPIO16。`cardputerzero-v5-overlay` 中的 `powerfail_suo` 已以 active-low 语义持有
P12，并在正常运行时将物理电平保持为高；额外加载 `camera-py12-high-overlay` 会用
GPIO hog 抢占同一条线。启动配置因此只按顺序加载板级 overlay 和 `imx219`，并拒绝
P12、GPIO16 两个独立 camera power overlay。

IMX219 的首次自动 probe 可能早于 M5IOE1 和 `powerfail_suo` 就绪。
`cardputerzero-camera-probe.service` 会先有限重试 `powerfail` 绑定，等待 P12 稳定，
再对 `10-0010` 做五次有限重试。结果和过滤后的 Camera 内核信息保存在
`/run/cardputerzero-camera-probe/`。日志只保留 Camera、M5IOE1、powerfail 和 Unicam
相关的前 100 行，不包含应用数据、网络标识或通用内核日志；开启 SSH 后 Owner 可读取。

V0.6 profile 设置 `start_x=1`，使用 `raspi-firmware` 的
`start_x.elf`/`fixup_x.dat`。M5Stack 官方镜像曾使用不透明的
`m5stack_bootscreen` 变体提供内核前 splash，但真机证明它会无视 64 MB 配置并强制
256/256 MB 内存划分，因此不再打包。镜像门禁明确拒绝该旧固件哈希；probe 状态记录
实际固件模式、变体和哈希。product splash 改由 initramfs direct-SPI、Linux
framebuffer 和受信任 Wayland surface 连续交接，详见 ADR 0008。

V0.6 冷启动真机还观察到早期 fbdev 初始化未被面板接受，而后续 compositor
disable/enable 会可靠发送完整的 ST7789 soft-reset、sleep-out 和 display-on 序列。
早期实现因此在 System Shell 前主动重启一次 compositor，但这会清除已经显示的
splash，并在约 8 秒处产生可见黑屏。当前实现让 display retry oneshot 只等待到该
稳定窗口，在等待期间保留 panel RAM 中的 framebuffer splash，之后只启动一次
Weston。服务按 `/proc/uptime` 只等待剩余时间，而不是从服务启动时再固定睡 8 秒；
晚启动时可避免把整段等待叠加到启动关键路径。Weston 内的受信任 splash 随后保持到
Setup/Home 首帧，最终冷启动门禁须在包含该改动的新镜像上完成。

## 构建最小镜像

构建默认使用 Docker，因此支持 macOS 和 Linux。Docker daemon 必须提供 Linux arm64
容器支持；Linux arm64 也可设置 `CP0_USE_DOCKER=0` 原生构建。

GitHub Release workflow 运行在 x86_64 runner 上。它会安装
`binfmt-support` 和 `qemu-user-static`，然后在启动特权 pi-gen 容器前向 runner
主机注册 `qemu-aarch64`。pi-gen 执行 ARM64 阶段命令依赖这个主机级注册；只在容器内
配置相关软件包是不够的。

```sh
export CP0_FIRST_USER_PASSWORD='development-password'
./image/build-image.sh
make verify-image
```

也可设置 `CP0_SSH_PUBLIC_KEY`，生成只允许公钥登录的开发镜像。正式镜像不得设置
默认密码或启用 SSH。

构建固定使用 `pi-gen` arm64 分支提交
`ca8aeed0ae300c2a89f55ce9617d5f96a27e99e5`，只执行官方 `stage0`、`stage1`
和 CardputerZero 自定义阶段，不包含 `stage2`。构建入口将 Debian 和 Raspberry Pi
软件源固定为 HTTPS，并在 `stage0` 只安装 CM0 所需的 `rpi-v8` 内核，不安装 Pi 5
的 2712 内核与头文件。
自定义阶段生成 `-cp0-os-dev` 镜像，编译固定 BSP 后删除编译器、内核头文件及桌面
无关组件，并安装真机诊断脚本。构建代理只存在于构建 chroot，导出的系统中不得残留
代理地址。

若下载或构建临时失败，构建容器会保留 `work/` 和 apt 缓存。恢复网络后续建：

```sh
CP0_FIRST_USER_PASSWORD='development-password' \
CP0_RESUME_BUILD=1 ./image/build-image.sh
```

设置 `CP0_KEEP_BUILD_CONTAINER=1` 可在成功后保留构建容器用于诊断。默认成功后自动
清理容器，失败时保留。产物写入 `deploy/`，包括压缩镜像、包清单、构建日志和
`SHA256SUMS`。

## 历史精简镜像候选（2026-07-30）

当前可烧录候选已在 macOS + Docker Linux/arm64 环境完整构建并通过只读挂载验收：

- 压缩镜像 224 MB，解压后 1.5 GB，根文件系统使用 724 MB，bootfs 使用 49 MB；
- 只保留 `6.18.34+rpt-rpi-v8` 内核和 16 个 CardputerZero BSP 模块；
- 不含 Launcher、LightDM、Wayfire、PCManFM、PipeWire、PackageKit、GTK 输入工具、
  编译器、内核头文件或 2712 内核；
- 包含裁剪的 Weston 14.0.2 DRM/Pixman kiosk 基线，compositor 默认关闭，继续保留
  `tty1` 恢复控制台；
- 默认启动到 `multi-user.target`，启用 NetworkManager、SSH 和 AppArmor；
- 屏蔽上游等待原厂 Launcher 的 `fb_load.service`，由 `tty1` framebuffer console
  接管 LCD；显示启动日志、硬件摘要、IPv4 和本地登录提示；
- V0.6 真机在 HDMI 接入时将 HDMI 枚举为 `fb0`、LCD 枚举为 `fb1`，因此使用
  `fbcon=map:1`；smoke test 按 `panel-mipi-dbid` 驱动名查找 LCD，不依赖编号；
- CM0 DTB 不再包含 `cgroup_disable=memory`；启动配置固定 64 MB GPU、64 MB CMA、
  memory cgroup 和 AppArmor；
- journald 使用最大 16 MB 的易失日志，zram 固定 192 MB，不启用写回。
- 安装并强制启用 `raspberrypi-sys-mods` 提供的 `rpi-resize.service`，首次启动将
  根分区扩展到 SD 卡剩余空间；构建时服务缺失会直接失败。

以上扩根行为仅描述 2026-07-30 的两分区基线。Phase 6A 三分区产品镜像已停用
`rpi-resize.service` 和 `resize` 内核参数，由 initramfs 只扩展最后一个
`cp0-data` 分区，并默认使用不可变根；见
[immutable root and persistent data](PHASE6A-IMMUTABLE-ROOT.zh-CN.md)。

2026-07-30 的 V0.6 精简镜像真机验收结果：

- `MemTotal` 为 424756 KiB，最终 Phase 2 候选的空闲系统约使用 151 MiB，zram 为
  192 MiB；
- 首次扩容启动到 `multi-user.target` 为 27.7 秒，后续稳定启动为 18.1 秒；
- LCD、RGB565 framebuffer、TCA8418 键盘、ES8389 音频、电池、memory cgroup
  和 AppArmor smoke test 全部通过，`failures=0`；未接相机记录为非阻塞警告；
- 2026-08-02 复验确认内核 I2C-1 总线及 6 个从设备正常，产品不暴露通用
  `/dev/i2c-1`；smoke 以 sysfs 总线存在为门禁，并把 raw access disabled 记录为
  安全状态而不是硬件警告；
- 首次扩容后 ext4 会运行一次 `ext4lazyinit` 初始化新增 inode table；该线程退出后，
  20 秒稳定采样窗口仅写入约 16 KiB，journald 保持 volatile；
- 32 GB 卡的根分区和 ext4 在首次启动自动从 976 MiB 扩展到 28.2 GiB；服务完成后
  自停用，第二次启动根分区保持 `rw,noatime`。

## 产品与恢复启动显示

product 镜像使用固定的 early splash，并设置 `quiet loglevel=3 logo.nologo`、
`vt.global_cursor_default=0`、`fbcon=map:off` 和 systemd status suppression。LCD 不再
显示内核、initramfs、systemd 日志或启动摘要，`cardputerzero-console-banner.service`
也不会启用。initramfs `init-top` 中的静态 helper 参考官方 `ci/early-splash` 的直接
SPI 路径，但使用固定 BSP 已由 DRM 路径验证的 `MADCTL=0xa0`、power/gamma 和 display
inversion 配置，并通过有界 TX/RX FIFO 流式泵送写入用户提供的固定图片；ST7789
framebuffer 出现后，initramfs 中的非阻塞 root worker 再将同一份
320x170 RGB565 帧写入 LCD，不等待数据分区扩容、OverlayFS 切根或 systemd；最终 root
中的 oneshot 负责有界重试。worker 与 oneshot 共享 `/run` 中的完成标记和原子锁，DRM
接管后只允许一次 framebuffer 写入。该画面保持到 compositor 和 System Shell 接管显示；
在此之前 LCD 不显示启动日志。

recovery 镜像继续使用 `loglevel=6 consoleblank=0 fbcon=map:1`，启用启动摘要和
`getty@tty1`。product development 镜像运行时进入 Recovery Mode 时，受限 helper 会
用 `/usr/bin/con2fbmap` 将 tty1 映射到 `/dev/fb_lcd` 的实际编号，因此 HDMI 是否连接
不会改变本地恢复终端目标。

product 启动时，出现 splash 证明 Linux 已进入 initramfs、SPI LCD 和 splash 资源
可用；出现 Home 才证明根文件系统、systemd、compositor 和 System Shell 已完成启动。
若长期停留在 splash，需结合路由器 DHCP 租约、mDNS 或已授权 SSH 诊断。登录后可查看：

```sh
ip -br -4 address
nmcli device status
```

若使用 recovery 镜像且首次烧录时没有注入 Wi-Fi 配置，可使用设备键盘在本地控制台连接：

```sh
sudo nmcli device wifi list
sudo nmcli device wifi connect 'SSID' password 'PASSWORD'
```

键盘使用 BSP 的 `tca8418_keypad_m5stack.ko` 和 `tca8418_m5stack.dtbo`。驱动与 LCD
模块被显式加入 initramfs；专用 hook 同时复制面板初始化固件
`cardputerzero,st7789v_lcd.bin`。按键通过 Linux input 子系统输入到 `tty1`；登录后
可直接执行 shell 命令。V0.6 的 Fn 层及全部组合键仍需在新镜像真机验收。

## 在现有真机验证启动参数

先从设备读取匹配当前固件的 `bcm2710-rpi-cm0.dtb`，用 `patch-cm0-dtb.sh` 生成修补
版本，将它与两个脚本上传到设备，再以 root 执行配置安装器：

```sh
./scripts/patch-cm0-dtb.sh bcm2710-rpi-cm0.dtb
sudo ./apply-dev-boot-profile.sh
sudo reboot
./device-smoke.sh
```

安装器在 `/boot/firmware/cardputerzero-os-backup/<UTC timestamp>` 保存原文件，不会
自动重启。若验证失败，可从 SD 卡或 SSH 恢复备份的 `config.txt` 和 `cmdline.txt`。
DTB 必须来自同一设备和固件版本，脚本拒绝修补不包含目标 token 的文件。

如果开发配置导致设备无法启动，在电脑上挂载 bootfs 后恢复最后一个已确认可启动的
备份：

```sh
cp cardputerzero-os-backup/20260730T075924Z/config.txt ./config.txt
cp cardputerzero-os-backup/20260730T075924Z/cmdline.txt ./cmdline.txt
```

这个备份保留已经验证可用的 AppArmor 和 `cma-64`，但不加载失败的 bootargs overlay。

## 最小化策略

开发镜像保留 NetworkManager 和 SSH。Launcher、LightDM、Wayfire、PCManFM、
PipeWire、PackageKit、Cloud Init、Avahi、RPC/NFS、UDisks、ModemManager、
Raspberry Pi Connect 和自动 apt 定时任务均不属于基础系统。Bluetooth 服务和工具
暂不安装，后续由权限 broker 接管时再加入。

2026-08-02 真机确认 SDIO BCM43439 可被 `brcmfmac` 探测，但旧镜像遗漏固件，
导致 `wlan0` 未创建。产品镜像现在强制安装 Raspberry Pi 仓库的
`firmware-brcm80211`，成品镜像门禁也要求该包存在；Wi-Fi 控制仍须通过后续的
Shell-only network settings broker，不能直接暴露 NetworkManager 权限。

设备使用 192 MB zram，禁止 zram writeback 和磁盘 swap，避免持续写 SD 卡。
