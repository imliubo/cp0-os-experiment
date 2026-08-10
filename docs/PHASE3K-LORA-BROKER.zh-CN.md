# Phase 3K: 限制的LoRa代理

<!-- doc-locale: zh-CN -->
> [English](PHASE3K-LORA-BROKER.md) | **简体中文**

CardputerZero V0.6 不包含内置的LoRa无线模块。此阶段支持SPI0芯片选择1上的外部SX1276系列模块。显示保持在芯片选择0；应用程序不能选择任一端点。

## 冻结的API

`radio.lora` capability 只暴露有界的数据包发送和接收操作。

- payloads 包含 1 到 64 字节；
- 接收从1到1000毫秒的等待请求；
- 接收的元数据包含以dBm为单位的-signed RSSI和以quarter-dB为单位的-signed SNR；
- 应用程序不接受任何原始SPI路径、GPIO、寄存器、频率、电源或调制设置；
- 成功传输之间必须至少间隔15秒。

无线电参数固定为125 kHz带宽，扩展因子7，编码率4/5，启用CRC，前导码8个符号，私有同步字`0x12`和14 dBm发射功率。Rust、C11、C++17和WIT SDK合同暴露相同的边界。Hello Card的`L`动作仅用于接收。

## 信任路径

```text
WASM radio SDK call
  -> Runtime validates linear-memory ranges and fixed bounds
  -> appd binds peer UID/cgroup to the running installed application
  -> appd verifies the root-owned manifest and radio.lora decision
  -> root-only cp0-radiod socket accepts only appd
  -> cp0-radiod serializes operations on fixed /dev/spidev0.1
  -> SX1276 fixed-register driver
```

`cp0-radiod` 作为专用的 `cp0-radio` 账户运行，并具有补充成员身份 `spi`。其 systemd 单元使用 `DevicePolicy=closed`，仅允许 `/dev/spidev0.1`；它没有任何能力、网络访问或可写的系统路径。应用程序保持在其现有的 Unix 只读沙盒内，并从未收到设备描述符。

## 监管配置

该镜像安装 `/etc/cardputerzero/lora.conf` 为 `0644 root:root`，使用：

```text
enabled=false
```

启用无线电需要支持的区域和在其编译范围内的一個频率，例如：

```text
enabled=true
region=eu868
frequency_hz=868100000
```

支持的区域标识符为`cn470`, `eu868`, `us915`, `au915`, `as923`, `in865`, `kr920` 和 `ru864`. 此范围验证不能替代当地的占空比、信道计划、天线或认证要求。量产设置必须选择合法适用的区域和频率。

## 验证

工作区测试涵盖严格的协议帧格式、标准Base64、负载和超时边界、区域/频率验证、速率限制、包元数据、代理授权路由、Runtime JSON 解码、SDK 编译以及图像/服务加固。Linux SX1276 路径还为AArch64 交叉编译。

物理接收/发射接受保持开放，直到连接一个SX1276模块并确认适用的合法频率。默认镜像不能传输，因为服务配置已禁用。
