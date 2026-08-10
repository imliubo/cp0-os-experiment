# Phase 5D: 设备仓库接受

<!-- doc-locale: zh-CN -->
> [English](PHASE5D-DEVICE-STORE-ACCEPTANCE.md) | **简体中文**

这个门在CM0设备上验证完整的量产Store路径。它使用同一个应用的两个SDK专用版本，从不给设备提供包路径、哈希、版本或签名的覆盖。

## 安全边界

测试必须保持生产网络策略不变。源必须是一个真实的可公开访问的 HTTPS 端点，具有可路由的地址和通过正常验证的证书。私有局域网服务器、 literal IP 地址、HTTP 端点、禁用 TLS 验证或环境代理都不是可接受的替代品。

设备在 `cardputerzero-stability-acceptance.service` 活跃时不会运行。它只在 `/run/cardputerzero-store-acceptance` 之下写入证据，不会重新启动或重新配置设备，并在恢复测试期间只杀死 `cardputerzero-stored.service` 的主进程。套接字和 `Restart=on-failure` 策略必须启动一个新的服务进程。

接受包、开发者密钥和Store签名密钥位于被忽略的主机`target/test-store`目录下。它们没有嵌入在产品镜像中。仅在测试设备上提供了32字节的Store公钥。

## 受控的公共源

推荐的接受来源是 behind a Cloudflare Quick Tunnel 后面的仓库的回环服务器。运行器优先选择 `target/tools/cloudflared-2026.7.3/source/cloudflared` 的仓库本地源构建，然后是 `CP0_CLOUDFLARED` 中的绝对路径，最后是 `PATH` 中的 `cloudflared`。确认选择的可执行文件而不启动隧道：

```sh
./scripts/run-test-store-origin.sh --print-cloudflared
```

仓库本地的接受二进制文件是从 vendored 依赖项在 Cloudflare 标签 `2026.7.3`、提交 `3a2b45c2a511fcdd81b68c190938e4ffadbea5dc` 构建的，使用了验证过的 Go 1.26.5 darwin/arm64 工具链。其 SHA-256 是 `0a59c7b61dedf9096d3df3ee52c7cef81ab31614e8fc8457e864506eae7aa672`。生成的二进制文件和源代码检出保持在忽略 `target/` 中，并不是镜像输入。运行器验证确切的摘要，如果仓库本地路径上的文件不匹配则停止；`CP0_CLOUDFLARED` 和 `PATH` 是显式的操作员信任选择。在继续之前需要构建或选择一个可信的可执行文件；运行器不会下载或更新 `cloudflared`。

在设备接受前不久，在专用主机终端中运行此操作：

```sh
./scripts/run-test-store-origin.sh 18080 1800 524288
```

跑步者会自动执行顺序敏感的设置：

1. 创建一个仅根用户可控制的文件，并选择 throttled v1；
2. 仅绑定起源到`127.0.0.1`；
3. 获取一个随机的公共`https://*.trycloudflare.com` URL；
4. 构建并签署两个目录，针对该确切URL和当前时间；
5. 保持在前台，因此停止运行程序也会停止两个进程。

公共端点仅提供活动的签名目录、其公钥和匹配的测试包。它从不提供开发人员密钥、Store 签名密钥、审核文件、控制文件或任意路径，并发送 Cache-Control: no-store。生成的目录和包是非敏感的测试构件，但它们通过 Cloudflare 中转，并且随机 URL 在运行器运行期间对公众可达。

跑步者打印出精确的控制命令。最初 v1 限制在 512 KiB/s，这为一个 8 MiB 的包提供了可靠中断窗口，同时保持在 `cp0-stored` 的 45 秒下载超时内。在 v1 恢复测试之后，切换到另一个主机终端的未限制 v2：

```sh
node scripts/test-store-origin.mjs set \
  target/test-store/origin-control.json v2
```

在离线缓存操作之前，将相同的HTTPS端点切换到受控的HTTP 503故障：

```sh
node scripts/test-store-origin.mjs set \
  target/test-store/origin-control.json offline-v2
```

这保持了DNS和TLS的稳定，并确保刷新达到真实的公共起源故障。在打印的`target/test-store-origin/<run-id>`目录下保留主机端请求日志、隧道日志、URL、进程ID和有效性时间。通过过期目录动作保持运行器存活。

## 手动静态源点

选择一个公共的HTTPS基础URL，其文档根目录可以在两个静态目录之间原子地切换。仅在完整的在线序列可以在过期前完成时使用短生命周期。

```sh
published=$(date +%s)
CP0_TEST_STORE_PAD_BYTES=8388608 \
  ./scripts/build-test-store.sh \
  https://store.example.com/cardputerzero-acceptance \
  "$published" 1800
```

该命令创建：

- `catalog-v1`: 版本1.0.0，在启动后显示绿色；
- `catalog-v2`: 版本1.1.0，在启动后显示蓝色；
- `store.pub`: 测试目录信任密钥；
- `acceptance.json`: 精确的 URL, 密钥 ID, 序列和有效性窗口。

它还验证开发者和Store的签名。审核记录绑定没有权限和恰好这些SDK导入：

```text
cp0_display_dimensions
cp0_present_rgb565
cp0_wait_event
```

使用手动管理的源时，首先将 `catalog-v1` 作为基础 URL 根。在恢复操作之前配置包响应，以实现低但稳定的吞吐量；默认的 8 MiB 填充必须部分下载足够长的时间，以便框架可以观察并中断它，但恢复的传输必须在 45 秒内完成。源必须支持正确的 `Range` 请求并返回 HTTP 206 以及匹配的 `Content-Range`。

## 配置测试设备

从没有安装过`dev.cardputerzero.store-test`和没有Store部分文件的设备开始。在获取到24小时稳定性结果后安装当前app平台部署。将`/etc/cardputerzero/store.conf`以根所有者模式0644和生成的目录URL进行配置，并安装恰好一个测试密钥：

```text
/etc/cardputerzero/trust/store/<store-key-id>.pub
```

钥匙必须是生成的`store.pub`，根用户拥有，权限模式0644。配置并启动一次`cardputerzero-stored.service`后，使其重新加载配置和信任根。这是仅用于测试的信任配置，不是生产Store注册。

## 有序运行

从`acceptance.json`中读取`sequence_v1`和`sequence_v2`。以root身份运行每个命令，并在任何重新启动之前获取其报告的目录。

1. 在 `catalog-v1` 在线时，刷新并验证其确切身份：

   ```sh
   /usr/libexec/cardputerzero/device-store-acceptance \
     refresh-v1 <sequence_v1>
   ```

2. 保持v1包的限速，然后证明部分文件的存活，一个新的`cp0-stored` PID，验证HTTP 206续传，安装并启动：

   ```sh
   /usr/libexec/cardputerzero/device-store-acceptance resume-v1
   ```

3. 原子地将相同的公共源根切换到`catalog-v2`, 移除包限制, 刷新, 升级并启动:

   ```sh
   /usr/libexec/cardputerzero/device-store-acceptance \
     refresh-v2 <sequence_v2>
   /usr/libexec/cardputerzero/device-store-acceptance upgrade-v2
   ```

4. 将公共源下线，同时保留签名目录的有效性。刷新必须在`cp0-stored`内失败，而缓存的v2目录仍然可浏览：

   ```sh
   /usr/libexec/cardputerzero/device-store-acceptance \
     offline-v2 <sequence_v2>
   ```

5. 保持原始设备离线，等待直到`expires_unix_seconds`，然后证明缓存的目录已过时且无法授权另一个安装：

   ```sh
   /usr/libexec/cardputerzero/device-store-acceptance \
     stale-v2 <sequence_v2>
   ```

该夹具为每个安装版本启动两秒。操作员可以在LCD上观察到v1显示绿色和v2显示蓝色，但视觉颜色是支持证据；appd报告的有符号安装版本是权威的。

## 证据和通过标准

每次操作都会生成一个只读RAM目录，包含 `status`，`checks.tsv`，`summary.env` 和该操作使用的有界命令响应。重启前请先获取完整目录，因为 `/run` 是易失的。

所有六个动作必须报告`PASS`。最终证据集必须显示：

- 在在线测试中使用 `stale=false` 精确匹配 v1 和 v2 的目录序列；
- 一个非空的部分，小于带符号包字节计数；
- 目标杀掉后的不同 `cp0-stored` 主PID；
- 经过验证的简历日志标记和成功的 v1 安装；
- 严格从v1升级到v2并成功启动每个版本；
- 一个真实的原始失败但缓存的目录没有丢失；
- `stale=true` 过期后和 `Untrusted` 安装拒绝；
- root-owned Store 配置/信任和私有`cp0-store` 缓存模式。

在检索到所有六个目录后，独立验证它们的原始 JSON，
重启细节，目录序列绑定和时间顺序：

```sh
./scripts/verify-device-acceptance-evidence.sh store \
  PATH_TO_REFRESH_V1 PATH_TO_RESUME_V1 PATH_TO_REFRESH_V2 \
  PATH_TO_UPGRADE_V2 PATH_TO_OFFLINE_V2 PATH_TO_STALE_V2
```

验证器拒绝一组单独自我报告的`PASS`运行，如果v2序列没有前进，离线/过时的证据绑定到另一个序列，恢复文件的大小不小于带签名的包，或者动作顺序错误。

如果当前 app-平台部署是在稳定性运行之后安装的，则此门不需要刷写。仅在先前尝试留下了测试数据时，才重新刷写以获取所需的最新 Store/应用状态。
