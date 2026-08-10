# Phase 4C: 模拟器, 部署和日志

<!-- doc-locale: zh-CN -->
> [English](PHASE4C-DEVELOPER-TOOLS.md) | **简体中文**

## PC 运行循环

`cp0ctl run` 只构建 SDK 应用，并使用捆绑的 Node WebAssembly 模拟器执行其 WASM 模块：

```sh
cp0ctl run ./my-app \
  --duration 1500 \
  --permissions allow \
  --keys left,enter,c \
  --output frame.ppm \
  --profile profile.json
```

应用程序在worker中运行，因此无限设备事件循环不能阻塞模拟器控制器。控制器在限定时间内终止它，将最后一个完整的RGB565帧写入便携式PPM图像，并发出一个包含WASM大小、线性内存页面、帧计数、键计数、通知记录、总主机调用次数和每能力调用次数的JSON配置文件。

模拟的表面对于标准应用是320x150，对于沉浸式应用是320x170。脚本输入名称映射到由实际Runtime提供的相同的Linux evdev代码。权限模式故意是二进制和确定性的：`deny` 拒绝所有敏感操作，而`allow`只允许在清单中声明的能力。网络、文档、音频、摄像头、GPIO和存储使用有界的确定性配置；模拟器从不授予WASM访问宿主机文件系统或套接字的权限。

模拟器是开发辅助工具，不是安全边界。设备准入仍然依赖于`.capp`签名和appd沙盒强制执行。

## 设备安装和日志

本地维护安装仍然可供root使用：

```sh
sudo cp0ctl install my-app-store.capp
sudo cp0ctl logs dev.example.my-app 50
```

对于正常SDK工作流程，在设备上启用开发者模式，打开十分钟的**新电脑配对**窗口，并使用Ed25519 SSH密钥注册开发者签名密钥：

```sh
cp0ctl pair developer.pub ~/.ssh/cardputerzero_ed25519.pub workstation \
  --device OWNER@DEVICE_IP
cp0ctl install my-app.developer.capp --device OWNER@DEVICE_IP
cp0ctl logs dev.example.my-app 50 --device OWNER@DEVICE_IP
```

远程安装在传输前验证开发者签名，并通过`ssh -T`和`cp0-dev`流式传输有界包体。强制命令键不能运行 shell，并且传输中不含`scp`、sudo 或通用上传回退。`cp0-devd`在每次请求时检查开发者模式和策略，验证包签名密钥仍处于配对状态，在根目录仅可访问的运行时目录中准备该包，并调用相同的 appd 承认路径。

日志由appd从根用户拥有的注册表解析到稳定的systemd单元。调用者不能提供任意单元。结果限制为100行，每行256个字符和有界JSON帧，并移除了控制字符。配对和撤销描述在`DEVELOPER-ACCESS.md`中。

`tests/test-simulator.sh` 构建并运行 Hello Card，允许使用摄像头、GPIO 和存储输入，然后检查 PPM 头部和配置计数器。CLI 测试还拒绝注入 SSH 选项、设备目标中的空白字符和 shell 元字符。
