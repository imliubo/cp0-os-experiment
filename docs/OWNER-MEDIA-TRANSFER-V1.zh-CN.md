# Owner USB媒体传输 v1

<!-- doc-locale: zh-CN -->
> [English](OWNER-MEDIA-TRANSFER-V1.md) | **简体中文**

Owner USB媒体传输是用于复制Camera和截屏照片到计算机并导入本地音乐的产品工作流程。它仅使用USB Mass Storage进行专用交换镜像。它从未导出挂载设备分区、根文件系统`cp0-data`、App包或App私有存储。

这是Owner工作流程。它与Developer Mode和Owner SSH Shell无关，并不会提供这两种模式。

## 所有者流程

1. 将CardputerZero的USB数据端口连接到计算机。
2. 打开 **设置 > 应用与隐私 > USB媒体传输**。
3. 输入当前所有者的密码并按 Enter。
4. 设备创建或验证交换镜像，导出照片副本，然后在本地卸载镜像，并将 `CP0-MEDIA` 呈现给计算机。
5. 从`PHOTOS/`复制文件进行备份。将支持的WAV文件放入`MUSIC/IMPORT/`进行导入。
6. 从计算机中弹出 `CP0-MEDIA`，然后在设备上按下 Enter。设备在检查 FAT32 文件系统并导入音乐之前会断开 USB LUN。

受信任的Shell不允许Home、Back或Tasks离开准备、连接或导入状态。意外的电缆拔除是可以恢复的，但Owner仍应停止设备上的传输，以便检查FAT文件系统并导入待处理的音乐。

## 不可协商的存储边界

唯一允许的 LUN 承载对象是：

```text
/var/lib/cardputerzero/usb-media/exchange.img
```

v1 版本的镜像正好是 512 MiB，并包含 FAT32。控制协议只有 `get-status`, `start` 和 `stop`；没有接受路径、设备名、LUN、挂载选项、容量或 ConfigFS 属性。

每次绑定之前，`cp0-usb-mediad` 证明支持对象：

- 在固定的交换目录下具有固定的`exchange.img`名称；
- 解析为那个确切的标准路径；
- 是一个普通文件，不是符号链接或块设备；
- 具有固定的容量；
- 在根唯一权限交换目录内部拥有。

守护进程创建一个ConfigFS `mass_storage.0` 函数，并仅将验证过的规范镜像路径写入 `lun.0/file`。`/dev/mmcblk*`、rootfs、`cp0-data`、`/var/lib/cardputerzero/data` 和调用者选择的路径永远无法到达协议或LUN设置代码。

连接到计算机时，交换文件系统从未被 Linux 挂载。设备端挂载使用 `nodev,nosuid,noexec,umask=0077`；顺序总是设备挂载 -> 阶段/导入 -> 设备卸载 -> USB 绑定，或 USB 解绑 -> 文件系统检查 -> 设备挂载。这防止了 Linux 和 USB 主机同时对文件系统的修改。

`exchange.img` 是临时且可重建的数据。`CP0 backup v1` 明确排除 `cardputerzero/usb-media`；它备份权威照片库和已导入的 Document Portal 文件。量产镜像不会预填充可变的 512 MiB `exchange.img`。

## 交换布局

```text
CP0-MEDIA/
  README.TXT
  manifest.json
  PHOTOS/
    IMG_<photo-id>.JPG
    SCREEN_<photo-id>.BMP
  MUSIC/
    IMPORT/
    IMPORT-RESULTS.JSON
```

相机条目是1280x720 JPEG原始文件的副本。截屏
RGB565帧以标准16位位场BMP文件无损编码，因此macOS、Linux和Windows可以在没有CardputerZero软件的情况下打开它们。
`manifest.json` 记录来源、尺寸、字节长度、捕获时间（如果可用），以及SHA-256。在`PHOTOS/`中删除或编辑一项内容仅更改交换副本；它从不删除或更改设备照片。

Music v1 接受符合文档门户合同的常规 `.wav` 文件，且文件内容为 48 kHz、立体声、16 位 PCM。每个文件受文档门户文件限制约束。符号链接、目录、损坏的 RIFF 块、不支持的格式、不稳定文件和多余条目均被拒绝。

接受的音乐被复制到根创建的临时文件中，重新检查其大小、inode和修改稳定性，`fsync`后，分配给文档门户账户，并发布而不会覆盖。名称冲突变为`name (n).wav`。只有在发布之后，才移除交换源。`MUSIC/IMPORT-RESULTS.JSON`报告导入并拒绝的名称和哈希。

## 服务和认证边界

System Shell 首先调用 `cp0-provisiond`，使用现有的 yescrypt 哈希验证当前的所有者密码。密码在协议和 C 客户端缓冲区中被清零。`cp0-usb-mediad` 不读取 shadow 数据。

媒体套接字仅可通过`cp0-shell`到`cp0-usb-media-control`写入，且守护进程独立验证对端UID。应用程序、App Runtime、Store、开发者模式会话和所有者SSH账号不能调用它。

`cp0-usb-mediad` 作为一个沙盒根服务运行，因为需要特权来支持循环挂载、ConfigFS、LUN 绑定和最终文档所有权。它的 systemd 单元仅授予 `CAP_SYS_ADMIN` 和 `CAP_CHOWN`，循环设备访问权限，固定的交换/文档目录，以及固定的 USB 设备 ConfigFS 树。照片内容通过 storaged 协议读取；守护进程不被授予直接访问应用私有存储或完整的 `cp0-data` 树的权限。

启动程序显式加载 `dwc2`、`loop` 和 `libcomposite`，才能启动服务，而服务需要配置文件系统和持久数据挂载。可信的Shell还显式地在其systemd单元中接收媒体控制套接字组，而不是仅依赖于account-database辅助组的初始化。

## 失败行为

- 一个新的镜像被分配了实际的存储预留，格式化，预置，检查，然后卸载，之后才进行USB绑定。SD空间不足会在曝光前失败；照片永远不会被无声省略。
- 在分配或重写512 MiB交换图像之前，启动检查ConfigFS，一个可用的UDC，以及可用的环回控制。所有者UI报告失败的硬件或文件系统阶段，而日志保留原始的操作系统错误。
- 现有的交换文件系统在恢复导入前会被检查。正常停止会首先解除 UDC 的绑定，并在导入前后都检查 FAT32。
- 服务停止和关机将执行紧急解绑、卸载和FAT32检查。之后的启动将恢复待处理的导入再重建交换。
- 路径非法、大小错误、符号链接、特殊文件、挂载状态冲突、文件系统检查失败或缺少 USB device controller 时，操作会按 fail-closed 原则拒绝。
- 如果主机破坏或删除了交换文件系统，权威的照片库和已导入的音乐将保持不变。

## 释放门

1. 获取并配置合法的量产 USB VID/PID。当前使用的 Linux Foundation 开发占位值
   `1d6b:0104` 不得发布。
2. 在V0.6中，验证ConfigFS，`dwc2`外设模式，循环设备策略，服务沙箱化，以及使用最终内核/systemd版本进行干净关机。
3. 验证枚举、读写、弹出、重新连接、畸形FAT、拔线、电源丢失、满SD卡和完整交换行为在当前macOS、Linux和Windows主机上的表现。
4. 往返哈希值用于Camera JPEG、Screenshot BMP和导入的WAV，然后证明主机删除从未改变原始文件，并且恢复备份从未包含`exchange.img`。
5. 在标记该功能为生产就绪之前，测量512 MB CM0 在准备/导入延迟、峰值内存使用率和SD写入次数。
