# M5Stack 启动屏固件

<!-- doc-locale: zh-CN -->
> [English](README.md) | **简体中文**

`start-m5stack-bootscreen.elf` 是 M5Stack 的 CardputerZero 镜像构建者分发的退役不透明 VideoCore 固件。它保留了 Raspberry Pi 的 `start_x` 摄像头功能集，并从 `/boot/firmware/splash.bmp` 添加了早期的 ST7789 滚动屏渲染功能。M5Stack 将此嵌入式 `start_x` 变体安装为 `/boot/firmware/start.elf`，而不设置 `start_x=1`，因此它保持与 `/boot/firmware/fixup.dat` 在出厂时配对的方式相同，与工作中的官方镜像相同。

- 源代码仓库: `https://github.com/CardputerZero/pi-gen`
- 由 `cc5a7375dfa903757b040e76a1e64e5b0dcf8e7f` 引入
- 审核过的快照: `554544921c1659f39bf296b7986715fdeac898c8`
- 源路径: `stage2/05-cardputerzero/files/start.elf`
- SHA256: `d1639763fa6714e2cd4544fb45b9d5e5d54e949eaa11d7e7057651b6d4d51efd`
- 配对的 `fixup.dat` SHA256: `b2d19b8c300b5a4ddbd0fcff3a0f7de61a171046269d8724e74f616058417d4b`
- 嵌入分支: `m5stack_bootscreen`
- 嵌入式变体: `start_x`
- 嵌入式上游版本: `85bf5729aa4fa558b105936b0841241dc4b9ee64 (tainted)`

该制品只作为来源留存，不会打包。V0.6 测试表明它忽略 `gpu_mem_512=64`，并强制将
GPU 内存设为 256 MB，导致 Linux 仅剩约 227 MiB。镜像构建和成品 rootfs 门禁现在会
拒绝该哈希。量产镜像和 Recovery 镜像使用由 `start_x=1` 选择的 `raspi-firmware`
`start_x.elf`/`fixup_x.dat` 组合；LCD framebuffer 可用后，由 Linux 渲染产品启动画面。
