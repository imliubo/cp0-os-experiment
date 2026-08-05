# ADR 0008：V0.6 early boot splash 与静默启动

- 状态：Accepted
- 日期：2026-08-06

## 决策

CM0 V0.6 product 镜像设置 `start_x=1`，使用 `raspi-firmware` 提供的
`start_x.elf`/`fixup_x.dat`。Linux 加载 ST7789 framebuffer 后，受限的
`cardputerzero-early-splash.service` 查找驱动名为 `panel-mipi-dbid`、模式为
320x170、16 bpp 的设备，将固定的 108800-byte `splash.rgb565` 写入一次，然后才允许
compositor 启动。服务不向 App 暴露 framebuffer，找不到 LCD 时有界退出且不阻止 Home。

不再打包 M5Stack `m5stack_bootscreen` 固件。其历史 SHA256
`d1639763fa6714e2cd4544fb45b9d5e5d54e949eaa11d7e7057651b6d4d51efd`
由构建阶段和最终 rootfs 门禁明确拒绝；标准固件版本则记录在每份镜像的软件包清单中。

product cmdline 使用 `quiet loglevel=3 logo.nologo`、
`vt.global_cursor_default=0`、`fbcon=map:off`、`systemd.show_status=false` 和
`rd.systemd.show_status=false`，且不启用 LCD console banner。recovery 镜像保留
`loglevel=6 fbcon=map:1` 与启动摘要。运行时 Recovery Mode 通过固定 helper 将 tty1
映射到 udev 管理的 `/dev/fb_lcd`，不依赖 HDMI/LCD 的 framebuffer 枚举顺序。

## 理由

供应商固件可以在 ARM 内核前初始化 ST7789，但 2026-08-06 真机证明它忽略
`gpu_mem_512=64` 并强制 256 MB VideoCore，Linux 只获得约 227 MiB。这个代价破坏
512 MB CM0 的 App 容量、性能门禁和 ADR 0003，不能接受。product 已用
`quiet loglevel=3 logo.nologo fbcon=map:off` 隐藏内核和 console 输出，因此在 Linux
framebuffer 出现前保持黑屏、随后显示 splash，不会把启动日志暴露到 LCD。

Circle bare-metal splash 可以从源码构建，但当前稳定实现需要重命名内核、写 FAT32
并二次重启。它增加冷启动时间和掉电窗口，不作为 product 默认链路。Linux early
framebuffer 服务不是内核前 splash，但保留了标准可维护固件、正确内存划分和单次启动。

## 后果

每次 `raspi-firmware` 更新仍须完成 V0.6 冷启动、无 HDMI/有 HDMI、IMX219、LCD
方向与色彩、64/448 MB 划分、正常重启、意外掉电和 recovery console 验收。镜像门禁
验证标准 `start_x` 选择、固件文件存在、旧供应商哈希不存在，以及 splash 的精确大小
和哈希。

product 的正常启动不再在 LCD 显示 IP、登录提示或内核错误。诊断依赖 Home、网络
租约、mDNS、Owner 授权的 SSH，或显式 recovery 镜像/Recovery Mode。recovery 镜像
仍保留完整可见控制台，避免 splash 策略消除本地修复路径。
