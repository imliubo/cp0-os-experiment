# ADR 0003：CM0 内存划分与 BSP 固定策略

- 状态：Accepted
- 日期：2026-07-30

## 决策

CM0 V0.6 使用 64 MB VideoCore、448 MB ARM 和 64 MB VC4 CMA。镜像构建阶段精确
删除基础 DTB 内置的 `cgroup_disable=memory` token，系统启用 unified cgroup v2 的
memory controller 与 AppArmor。BSP 固定到 M5Stack dtoverlays 提交
`c3b254819307c177a34100b66fe19e52059ce8c4`，使用上游 V0.5 编译开关及
`cardputerzero-v5-overlay`。V0.6 相机供电固定使用该 BSP 提供的
`camera-py12-high-overlay`，并在 `imx219` 前加载；旧版
`camera-gpio16-high-overlay` 不得用于 V0.6。

## 理由

真机默认把 512 MB 平分给 ARM 和 VideoCore，Linux 只剩约 227 MiB。通用
`gpu_mem=64` 被 CM0 的 512 MB 专用默认覆盖，因此同时设置 `gpu_mem_512=64`。
VC4 同时请求
256 MB CMA 并分配失败，造成持续的 DRM/相机错误。全 KMS 不需要 256 MB 固件堆，
64 MB GPU 与 64 MB CMA 为当前相机、DRM 测试提供了保守余量。

基础 `bcm2710-rpi-cm0.dtb` 直接携带 `cgroup_disable=memory`，追加 enable 参数无法
撤销已经执行的 disable 操作。不能用 overlay 覆盖整个 `bootargs`，因为 firmware 在
应用 overlay 前已经合并 `cmdline.txt`；覆盖会同时删除 rootfs 参数并导致启动失败。
因此在镜像构建时直接修补匹配固件版本的基础 DTB，且只删除目标 token。

应用资源隔离依赖 memory cgroup；AppArmor 则是该 Raspberry Pi 内核已经编译、无需
更换内核即可启用的主要 LSM。当前内核没有 Landlock、Yama 和 BPF LSM，首版隔离
设计不能把它们作为前提。

## 后果

相机 broker 必须使用 libcamera/V4L2 路径，不能重新依赖大块 VideoCore legacy heap。
Phase 1 真机测试需要覆盖连续相机预览、DRM 刷屏和音频播放，确认 64 MB CMA 足够。
若不足，只增加 CMA，不增加 `gpu_mem`。
