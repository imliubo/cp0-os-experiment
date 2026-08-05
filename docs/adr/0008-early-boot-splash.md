# ADR 0008：V0.6 early boot splash 与静默启动

- 状态：Accepted
- 日期：2026-08-05

## 决策

CM0 V0.6 product 镜像使用 M5Stack 发布的 `m5stack_bootscreen` VideoCore 固件，
固件按官方镜像原样安装为 `start.elf`，不设置 `start_x=1`，并与
`raspi-firmware` 的 `fixup.dat` 配对。启动分区同时安装固定的
`splash.bmp`。该 BMP 是 170x320、16-bit RGB565；固件按 ST7789 原生方向读取后，
LCD 上显示为 320x170 横屏图像。

固件固定到 CardputerZero/pi-gen 提交
`554544921c1659f39bf296b7986715fdeac898c8` 的
`stage2/05-cardputerzero/files/start.elf`，SHA256 为
`d1639763fa6714e2cd4544fb45b9d5e5d54e949eaa11d7e7057651b6d4d51efd`。
配套 `fixup.dat` 的 SHA256 为
`b2d19b8c300b5a4ddbd0fcff3a0f7de61a171046269d8724e74f616058417d4b`。
构建阶段和最终 rootfs 门禁都校验两个精确哈希。该供应商二进制的 embedded variant
虽然是 `start_x`，但 M5Stack 官方可用镜像将其作为 `start.elf` 使用；改名为
`start_x.elf` 并改配 `fixup_x.dat` 不属于供应商验证过的布局。

product cmdline 使用 `quiet loglevel=3 logo.nologo`、
`vt.global_cursor_default=0`、`fbcon=map:off`、`systemd.show_status=false` 和
`rd.systemd.show_status=false`，且不启用 LCD console banner。recovery 镜像保留
`loglevel=6 fbcon=map:1` 与启动摘要。运行时 Recovery Mode 通过固定 helper 将 tty1
映射到 udev 管理的 `/dev/fb_lcd`，不依赖 HDMI/LCD 的 framebuffer 枚举顺序。

## 理由

Plymouth、systemd splash 或普通 framebuffer 应用都必须等待 Linux DRM 驱动和
用户空间启动，无法阻止更早的 LCD framebuffer console 输出。M5Stack 的固件在 ARM
内核执行前直接初始化 ST7789 并读取 splash，是真机官方镜像已经使用的最早可用路径。
该二进制的 embedded variant 为 `start_x`，同时满足 IMX219 所需的 Camera-capable
firmware 路径。

Circle bare-metal splash 可以从源码构建，但当前稳定实现需要重命名内核、写 FAT32
并二次重启。它增加冷启动时间和掉电窗口，不作为 product 默认链路。仅用
`quiet fbcon=map:off` 虽能隐藏日志，却不能提供内核前 splash。

## 后果

该固件是不可从本仓库复现的供应商二进制，embedded version 还标记为 `tainted`。
因此每次替换都必须记录来源与哈希，并完成 V0.6 冷启动、无 HDMI/有 HDMI、IMX219、
LCD 方向与色彩、正常重启、意外掉电和 recovery console 验收。Raspberry Pi 固件包
更新不能静默替换它；镜像门禁会直接失败。

product 的正常启动不再在 LCD 显示 IP、登录提示或内核错误。诊断依赖 Home、网络
租约、mDNS、Owner 授权的 SSH，或显式 recovery 镜像/Recovery Mode。recovery 镜像
仍保留完整可见控制台，避免 splash 策略消除本地修复路径。
