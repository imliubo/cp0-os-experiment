# Phase 1：CM0 V0.6 BSP 与最小镜像

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

V0.6 冷启动真机还观察到早期 fbdev 初始化未被面板接受，而后续 compositor
disable/enable 会可靠发送完整的 ST7789 soft-reset、sleep-out 和 display-on 序列。
产品镜像因此在 System Shell 首次就绪后执行一次有界的 compositor 重启；服务为
oneshot，不在 recovery 镜像启用，也不会形成重启循环。2026-08-02 摄像头复验发现
1 秒重试仍可能早于面板稳定窗口，而稍后的手动 disable/enable 可立即恢复 Home；
默认重试延迟因此调整为 8 秒，最终冷启动门禁须在包含该改动的新镜像上完成。

## 构建最小镜像

构建默认使用 Docker，因此支持 macOS 和 Linux。Docker daemon 必须提供 Linux arm64
容器支持；Linux arm64 也可设置 `CP0_USE_DOCKER=0` 原生构建。

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

## 当前精简镜像候选（2026-07-30）

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
[immutable root and persistent data](PHASE6A-IMMUTABLE-ROOT.md)。

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

## 无调试接口时的首次启动

开发镜像不使用静默启动。LCD 驱动加载后会接管 `tty1` 并显示内核与 systemd 日志；
系统进入 multi-user target 后，屏幕显示不超过 8 行的启动摘要：

```text
CardputerZero OS DEV
Boot:     READY
LCD:      OK 320x170
Keyboard: OK
IPv4:     192.168.x.x
Login:    pi
```

随后出现 `CardputerZero login:`。出现摘要和登录提示代表内核、根文件系统、systemd、
LCD 及键盘输入路径已经工作。LCD 驱动本身加载前无法显示 Linux 日志，因此完全黑屏
仍需结合路由器 DHCP 租约或网络扫描判断设备是否已启动。

IPv4 会直接显示在摘要中。登录后也可查看所有接口：

```sh
ip -br -4 address
nmcli device status
```

如果首次烧录时没有注入 Wi-Fi 配置，可使用设备键盘在本地控制台连接：

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
