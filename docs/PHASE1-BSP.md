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
使用的硬件描述。构建脚本必须校验 commit，不允许使用浮动 HEAD。

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
和 CardputerZero 自定义阶段，不包含 `stage2`。自定义阶段生成 `-cp0-os-dev`
镜像，编译固定 BSP 后删除编译器、内核头文件、2712 内核及桌面无关组件，并安装
真机诊断脚本。构建代理只存在于构建 chroot，导出的系统中不得残留代理地址。

若下载或构建临时失败，构建容器会保留 `work/` 和 apt 缓存。恢复网络后续建：

```sh
CP0_FIRST_USER_PASSWORD='development-password' \
CP0_RESUME_BUILD=1 ./image/build-image.sh
```

设置 `CP0_KEEP_BUILD_CONTAINER=1` 可在成功后保留构建容器用于诊断。默认成功后自动
清理容器，失败时保留。产物写入 `deploy/`，包括压缩镜像、包清单、构建日志和
`SHA256SUMS`。

## 首个精简镜像候选（2026-07-30）

首个可烧录候选已在 macOS + Docker Linux/arm64 环境完整构建：

- 压缩镜像 215 MB，解压后 1.5 GB，根文件系统实际内容约 695 MB；
- 只保留 `6.18.34+rpt-rpi-v8` 内核和 16 个 CardputerZero BSP 模块；
- 不含 Launcher、LightDM、Wayfire、PCManFM、PipeWire、PackageKit、GTK 输入工具、
  编译器、内核头文件或 2712 内核；
- 默认启动到 `multi-user.target`，启用 NetworkManager、SSH 和 AppArmor；
- 屏蔽上游等待原厂 Launcher 的 `fb_load.service`，由 `tty1` framebuffer console
  接管 LCD；显示启动日志、硬件摘要、IPv4 和本地登录提示；
- CM0 DTB 不再包含 `cgroup_disable=memory`；启动配置固定 64 MB GPU、64 MB CMA、
  memory cgroup 和 AppArmor；
- journald 使用最大 16 MB 的易失日志，zram 固定 192 MB，不启用写回。

离线检查已通过，但 RAM 是否恢复为至少 400 MB、显示/输入/音频等驱动是否在干净
系统上正常加载，仍必须烧录后由真机确认。

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
模块被显式加入 initramfs，按键通过 Linux input 子系统输入到 `tty1`；登录后可直接
执行 shell 命令。V0.6 的 Fn 层及全部组合键仍需在新镜像真机验收。

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

设备使用 192 MB zram，禁止 zram writeback 和磁盘 swap，避免持续写 SD 卡。
